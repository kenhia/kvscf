//! kvscf app — a tall/thin "nav rail" of open VS Code windows.
//! Click a row to foreground+focus it. Optional "maximize on focus" (WI #465).
//!
//! Two window modes:
//! - **Floating** (default): a normal, non-always-on-top, resizable window that remembers its
//!   geometry; "Auto-hide after focus" self-minimizes ~2s after a click.
//! - **Docked** (WI #468): an AppBar reserving the **primary monitor's left edge** so maximized
//!   windows don't cover it (borderless, always-on-top). Ken only docks on the primary monitor.
//!
//! Live left-aligned list (name build-colored + real bold, host italic, name truncated but host
//! always kept), click-to-focus, and settings ("maximize on focus", "auto-hide", "docked")
//! persisted to HKCU\Software\kenhia\kvscf.
//!
//! This is a library so two bin crates can build it with different features: `kvscf`
//! (default, `remote` on → kdeskdash channel) and `kvscf-local` (`remote` off → no comms
//! code at all, for `kwork`). See WI #471.
//!
//! Module map (decomposed from this file in sprint 013, WI #496):
//! `rows` (the one row painter) · `theme` (colors) · `fonts` · `settings` · `probes`
//! (headless verification flags) · `apps` / `winset` / `dock` (domain) · `single_instance` /
//! `userreg` (Windows plumbing) · `remote` (kdeskdash channel, feature-gated).

mod apps;
mod dock;
mod fonts;
mod probes;
mod rows;
mod settings;
mod theme;
mod winset;

#[cfg(windows)]
mod single_instance;
#[cfg(windows)]
mod userreg;

#[cfg(feature = "remote")]
mod remote;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{ViewportCommand, WindowLevel};

use kvscf_core::{
    close_window, focus_with, launch_and_focus, scan_all, App, AppMatcher, EdgeWindow, Instance,
    LaunchSpec,
};

use apps::AppEntry;

/// Which source the app is showing (WI #474; Apps added sprint 007).
#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Code,
    Edge,
    Apps,
}

/// Update Assist flow state (WI #470).
#[derive(PartialEq)]
enum UaState {
    Idle,
    ConfirmClose,
    ReadyRelaunch,
}

/// A favorites mutation captured from a Code-tab click / context menu, applied *after* the row
/// loop so the immutable borrow of the window list is released before we mutate `favorites`
/// (sprint 008).
enum FavAction {
    Add(winset::SetEntry),
    Remove(winset::SetEntry),
    Close(i64),
}

/// Everything a frame's row clicks can ask for, applied after the tab render releases its
/// borrows of the lists.
#[derive(Default)]
struct Actions {
    /// Focus this HWND.
    focus: Option<i64>,
    /// Launch a configured app (not running), then foreground on appearance.
    launch: Option<(LaunchSpec, AppMatcher)>,
    /// Relaunch a clicked dimmed favorite.
    fav_launch: Option<winset::SetEntry>,
    /// A favorites mutation from a right-click.
    fav_action: Option<FavAction>,
}

const RAIL_WIDTH: f32 = 280.0;
const RAIL_HEIGHT: f32 = 1040.0;
const SCAN_INTERVAL: Duration = Duration::from_millis(1000);
const AUTO_HIDE_DELAY: Duration = Duration::from_secs(2);
const DOCK_REASSERT: Duration = Duration::from_secs(1);

/// Whether this build includes the remote (kdeskdash) channel.
#[cfg(feature = "remote")]
pub const REMOTE_BUILD: bool = true;
#[cfg(not(feature = "remote"))]
pub const REMOTE_BUILD: bool = false;

/// The window title / build identity: `kvscf` (remote) or `kvscf-local` (no comms).
pub(crate) const APP_TITLE: &str = if REMOTE_BUILD { "kvscf" } else { "kvscf-local" };

/// Run the app. Called by the thin `kvscf` / `kvscf-local` bin crates.
pub fn run() -> eframe::Result<()> {
    // Headless probes (--build-info, --dump-apps, …) print and exit without a window.
    if probes::try_run() {
        return Ok(());
    }

    // Single instance only — two docked bars would fight over the reserved edge.
    #[cfg(windows)]
    if !single_instance::acquire() {
        return Ok(());
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_TITLE)
        .with_inner_size([RAIL_WIDTH, RAIL_HEIGHT])
        .with_min_inner_size([160.0, 240.0])
        .with_position([0.0, 0.0]);
    // Runtime window/taskbar icon (the exe file icon is embedded separately via build.rs).
    if let Ok(icon) =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/kvscf-256.png"))
    {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let native_options = eframe::NativeOptions {
        persist_window: true, // remember size/position across runs
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        native_options,
        Box::new(|cc| Ok(Box::new(KvscfApp::new(cc)))),
    )
}

struct KvscfApp {
    items: Vec<Instance>,
    edge: Vec<EdgeWindow>,
    apps: Vec<AppEntry>,
    /// Persisted Code favorites (sprint 008) — folders that can be relaunched when closed.
    favorites: Vec<winset::SetEntry>,
    /// HWND → resolved folder entry, filled incrementally so we only read VS Code's
    /// workspaceStorage when a *new* window appears (not every 1s refresh).
    uri_cache: HashMap<i64, winset::SetEntry>,
    tab: Tab,
    last_scan: Instant,
    maximize_on_focus: bool,
    auto_hide: bool,
    docked: bool,
    has_bold: bool,
    hide_at: Option<Instant>,
    hwnd: Option<isize>,
    appbar_registered: bool,
    mode_applied: bool,
    last_dock_assert: Instant,
    /// Docked-only: are we currently yielding z-order to a fullscreen app? (WI #481)
    fullscreen_active: bool,
    ua_state: UaState,
    ua_relaunch: Vec<winset::SetEntry>,
    ua_closed: usize,
    ua_status: String,
    #[cfg(feature = "remote")]
    channel: Option<remote::Channel>,
}

impl KvscfApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let has_bold = fonts::install_bold_font(&cc.egui_ctx);
        let s = settings::load();
        let mut app = KvscfApp {
            items: Vec::new(),
            edge: Vec::new(),
            apps: Vec::new(),
            favorites: winset::load_favorites(),
            uri_cache: HashMap::new(),
            tab: Tab::Code,
            last_scan: Instant::now() - SCAN_INTERVAL, // force an immediate scan
            maximize_on_focus: s.maximize_on_focus,
            auto_hide: s.auto_hide,
            docked: s.docked,
            has_bold,
            hide_at: None,
            hwnd: None,
            appbar_registered: false,
            mode_applied: false,
            last_dock_assert: Instant::now(),
            fullscreen_active: false,
            ua_state: UaState::Idle,
            ua_relaunch: Vec::new(),
            ua_closed: 0,
            ua_status: String::new(),
            #[cfg(feature = "remote")]
            channel: remote::Channel::start(),
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        let (mut items, mut edge) = scan_all();
        // VS Code: fastest-to-scan ordering — lowercased workspace name (hosts interleaved).
        items.sort_by_key(|i| i.workspace.to_lowercase());
        // Edge: named windows first, then by label (both alphabetical).
        kvscf_core::sort_edge_windows(&mut edge);
        self.items = items;
        self.edge = edge;
        // Apps: configured in the registry, resolved to running/not each refresh.
        self.apps = apps::scan();
        // Favorites need each open window's folder URI; keep the HWND→URI cache in step.
        self.refresh_uri_cache();
        self.last_scan = Instant::now();

        // Publish the fresh lists to kdeskdash (no-op in the local build). Favorites ride along:
        // which open windows are starred, plus the not-open ones the dashboard greys out.
        #[cfg(feature = "remote")]
        if let Some(ch) = &self.channel {
            let favorited: HashSet<i64> = self
                .uri_cache
                .iter()
                .filter(|(_, e)| self.favorites.iter().any(|f| f.same_target(e)))
                .map(|(hwnd, _)| *hwnd)
                .collect();
            let dimmed = self.dimmed_favorites();
            ch.publish(&self.items, &self.edge, &self.apps, &favorited, &dimmed);
        }
    }

    /// Keep `uri_cache` = {open HWND → its folder entry}. Prunes closed windows, and only does the
    /// heavier workspaceStorage resolve when a window we haven't seen yet appears.
    fn refresh_uri_cache(&mut self) {
        let open: HashSet<i64> = self.items.iter().map(|i| i.hwnd).collect();
        self.uri_cache.retain(|hwnd, _| open.contains(hwnd));
        let has_new = self
            .items
            .iter()
            .any(|i| !self.uri_cache.contains_key(&i.hwnd));
        if has_new {
            let (resolved, _unresolved) = winset::resolve_open_set(&self.items);
            for (inst, entry) in resolved {
                self.uri_cache.insert(inst.hwnd, entry);
            }
        }
    }

    /// Favorites not currently open — the dimmed, relaunchable rows.
    fn dimmed_favorites(&self) -> Vec<winset::SetEntry> {
        self.favorites
            .iter()
            .filter(|f| !self.uri_cache.values().any(|e| e.same_target(f)))
            .cloned()
            .collect()
    }

    /// Add `entry` to favorites (if new) and persist.
    fn add_favorite(&mut self, entry: winset::SetEntry) {
        if !self.favorites.iter().any(|f| f.same_target(&entry)) {
            self.favorites.push(entry);
            let _ = winset::save_favorites(&self.favorites);
        }
    }

    /// Remove any favorite matching `entry`'s target and persist.
    fn remove_favorite(&mut self, entry: &winset::SetEntry) {
        let before = self.favorites.len();
        self.favorites.retain(|f| !f.same_target(entry));
        if self.favorites.len() != before {
            let _ = winset::save_favorites(&self.favorites);
        }
    }

    fn save_settings(&self) {
        settings::save(&settings::Settings {
            maximize_on_focus: self.maximize_on_focus,
            auto_hide: self.auto_hide,
            docked: self.docked,
        });
    }

    /// Apply the current `docked` state: register/remove the AppBar and flip
    /// decorations + always-on-top accordingly.
    fn apply_mode(&mut self, ctx: &egui::Context) {
        let Some(hwnd) = self.hwnd else { return };
        // Either branch sets the window level explicitly below, so any fullscreen yield in effect
        // is now moot — clear it so the state can't get stuck across a dock/undock.
        self.fullscreen_active = false;
        if self.docked {
            ctx.send_viewport_cmd(ViewportCommand::Decorations(false));
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
            if !self.appbar_registered {
                dock::register(hwnd);
                self.appbar_registered = true;
            }
            self.reassert_dock(ctx);
        } else {
            if self.appbar_registered {
                dock::remove(hwnd);
                self.appbar_registered = false;
            }
            ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::Normal));
        }
    }

    /// Docked-only: behave like the taskbar around fullscreen apps (WI #481). While one owns the
    /// foreground on our monitor, sink below it; come back afterwards. Only acts on a *state
    /// change*, so we aren't hammering `SetWindowPos` every tick.
    ///
    /// Z-order is driven straight through Win32 rather than `ViewportCommand::WindowLevel`, for
    /// two reasons: the yield needs `HWND_BOTTOM` (merely clearing topmost leaves us at the top of
    /// the non-topmost band, still above the fullscreen app — measured), and viewport commands are
    /// applied asynchronously on the next frame, which would race the ordering of the two
    /// `SetWindowPos` calls that the yield requires.
    ///
    /// The AppBar reservation deliberately stays registered throughout — fullscreen apps use the
    /// full monitor bounds and ignore the work area anyway (the taskbar keeps its band too).
    fn update_fullscreen_yield(&mut self) {
        let Some(hwnd) = self.hwnd else { return };
        let fullscreen = dock::fullscreen_app_present(hwnd);
        if fullscreen == self.fullscreen_active {
            return;
        }
        self.fullscreen_active = fullscreen;
        if fullscreen {
            dock::yield_z_order(hwnd);
        } else {
            dock::restore_z_order(hwnd);
        }
    }

    /// Re-assert the reserved left band and snap our window into it (physical pixels).
    fn reassert_dock(&mut self, ctx: &egui::Context) {
        let Some(hwnd) = self.hwnd else { return };
        let ppp = ctx.pixels_per_point();
        let width_px = (ctx.screen_rect().width() * ppp).round() as i32;
        dock::set_pos(hwnd, width_px);
        self.last_dock_assert = Instant::now();
    }

    /// Update Assist "Close Extras": keep one window per (remote host × build), close the rest,
    /// and remember the closed set to relaunch after the update. Locals are left alone.
    fn ua_close_extras(&mut self) {
        let (resolved, _unresolved) = winset::resolve_open_set_now();
        let mut groups: HashMap<(String, App), Vec<(Instance, winset::SetEntry)>> = HashMap::new();
        for (inst, entry) in resolved {
            // Only remote windows are grouped/closed; locals are left open.
            if let Some(host) = inst.remote.host() {
                groups
                    .entry((host.to_string(), inst.app))
                    .or_default()
                    .push((inst, entry));
            }
        }
        let mut relaunch = Vec::new();
        let mut closed = 0;
        for (_key, mut members) in groups {
            if members.len() <= 1 {
                continue; // nothing extra to close for this host×build
            }
            // Survivor = most-recently-active (lowest z_index); it carries the update.
            members.sort_by_key(|(inst, _)| inst.z_index);
            let mut iter = members.into_iter();
            let _survivor = iter.next();
            for (inst, entry) in iter {
                if close_window(inst.hwnd) {
                    closed += 1;
                }
                relaunch.push(entry);
            }
        }
        self.ua_relaunch = relaunch;
        self.ua_closed = closed;
        self.ua_state = UaState::ReadyRelaunch;
    }

    /// Relaunch the closed set (staggered) and return to idle.
    fn ua_relaunch_now(&mut self) {
        let set = std::mem::take(&mut self.ua_relaunch);
        winset::relaunch(set, Duration::from_millis(1500));
        self.ua_closed = 0;
        self.ua_state = UaState::Idle;
    }

    fn ua_cancel(&mut self) {
        self.ua_relaunch.clear();
        self.ua_closed = 0;
        self.ua_state = UaState::Idle;
    }

    // --- UI pieces (decomposed from update(), WI #496) ---

    /// The `[ Code | Edge | Apps ]` tab strip.
    fn ui_tabs(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.tab,
                    Tab::Code,
                    format!("Code ({})", self.items.len()),
                );
                ui.selectable_value(
                    &mut self.tab,
                    Tab::Edge,
                    format!("Edge ({})", self.edge.len()),
                );
                ui.selectable_value(
                    &mut self.tab,
                    Tab::Apps,
                    format!("Apps ({})", self.apps.len()),
                );
            });
            ui.add_space(2.0);
        });
    }

    /// Settings checkboxes + count + manual refresh.
    fn ui_header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(3.0);
            let mut changed = false;
            changed |= ui
                .checkbox(&mut self.maximize_on_focus, "Maximize on focus")
                .changed();
            ui.horizontal(|ui| {
                let dock_resp = ui.checkbox(&mut self.docked, "Dock (primary left)");
                if dock_resp.changed() {
                    changed = true;
                    self.apply_mode(ctx);
                }
                ui.add_enabled_ui(!self.docked, |ui| {
                    if ui
                        .checkbox(&mut self.auto_hide, "Auto-hide")
                        .on_hover_text("Self-minimize ~2s after focusing (floating mode only)")
                        .changed()
                    {
                        changed = true;
                    }
                });
            });
            ui.horizontal(|ui| {
                let n = match self.tab {
                    Tab::Code => self.items.len(),
                    Tab::Edge => self.edge.len(),
                    Tab::Apps => self.apps.len(),
                };
                let noun = if self.tab == Tab::Apps {
                    "app"
                } else {
                    "window"
                };
                ui.label(egui::RichText::new(format!("{n} {noun}(s)")).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⟳").on_hover_text("Refresh now").clicked() {
                        self.refresh();
                    }
                });
            });
            if changed {
                self.save_settings();
            }
            ui.add_space(3.0);
        });
    }

    /// Save/Restore + the Update Assist flow (bottom panel; VS-Code-specific, Code tab only).
    fn ui_update_assist(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("update_assist").show(ctx, |ui| {
            ui.add_space(4.0);
            match self.ua_state {
                UaState::Idle => {
                    ui.horizontal(|ui| {
                        if ui
                            .button("Save set")
                            .on_hover_text("Save the currently open windows as 'last'")
                            .clicked()
                        {
                            let (resolved, _) = winset::resolve_open_set_now();
                            let entries: Vec<_> = resolved.into_iter().map(|(_, e)| e).collect();
                            let n = entries.len();
                            self.ua_status = match winset::save_set("last", &entries) {
                                Ok(()) => format!("saved {n}"),
                                Err(_) => "save failed".into(),
                            };
                        }
                        if ui
                            .button("Restore")
                            .on_hover_text("Relaunch the saved 'last' set")
                            .clicked()
                        {
                            match winset::load_set("last") {
                                Some(set) => {
                                    let n = set.len();
                                    winset::relaunch(set, Duration::from_millis(1500));
                                    self.ua_status = format!("relaunching {n}…");
                                }
                                None => self.ua_status = "no saved set".into(),
                            }
                        }
                    });
                    if !self.ua_status.is_empty() {
                        ui.label(egui::RichText::new(&self.ua_status).small().weak());
                    }
                    if ui
                        .button("Update Assist")
                        .on_hover_text(
                            "Insiders update helper: close all but one window per host×build,\n\
                             you run the update(s), then relaunch the rest.",
                        )
                        .clicked()
                    {
                        self.ua_status.clear();
                        self.ua_state = UaState::ConfirmClose;
                    }
                }
                UaState::ConfirmClose => {
                    ui.label(
                        egui::RichText::new("Keep one window per host × build, close the rest?")
                            .small()
                            .weak(),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Close Extras").clicked() {
                            self.ua_close_extras();
                        }
                        if ui.button("Cancel").clicked() {
                            self.ua_cancel();
                        }
                    });
                }
                UaState::ReadyRelaunch => {
                    ui.label(
                        egui::RichText::new(format!(
                            "Closed {}. Your turn — start the update(s), then click Relaunch.",
                            self.ua_closed
                        ))
                        .small(),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Relaunch").clicked() {
                            self.ua_relaunch_now();
                        }
                        if ui.button("Cancel").clicked() {
                            self.ua_cancel();
                        }
                    });
                }
            }
            ui.add_space(4.0);
        });
    }

    /// Code tab: running windows (click to focus, right-click to (un)favorite), then a dimmed
    /// section of favorites that aren't open (click to relaunch).
    fn ui_code_tab(&self, ui: &mut egui::Ui, actions: &mut Actions) {
        let dimmed = self.dimmed_favorites();
        if self.items.is_empty() && dimmed.is_empty() {
            ui.add_space(12.0);
            ui.weak("No VS Code windows open.");
            return;
        }
        let name_font = fonts::name_font(self.has_bold);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                // Running windows — click to focus; right-click to (un)favorite.
                for item in &self.items {
                    let hwnd = item.hwnd;
                    let entry = self.uri_cache.get(&hwnd).cloned();
                    let favorited = entry
                        .as_ref()
                        .map(|e| self.favorites.iter().any(|f| f.same_target(e)))
                        .unwrap_or(false);
                    let resp = rows::code_row(ui, item, &name_font, favorited);
                    if resp.clicked() {
                        actions.focus = Some(hwnd);
                    }
                    resp.context_menu(|ui| match entry {
                        None => {
                            ui.add_enabled(false, egui::Button::new("★ Mark as favorite"))
                                .on_disabled_hover_text("Can't resolve this window's folder");
                        }
                        Some(e) if favorited => {
                            if ui.button("☆ Unfavorite").clicked() {
                                actions.fav_action = Some(FavAction::Remove(e));
                                ui.close_menu();
                            }
                            if ui.button("Close (keep favorite)").clicked() {
                                actions.fav_action = Some(FavAction::Close(hwnd));
                                ui.close_menu();
                            }
                        }
                        Some(e) => {
                            if ui.button("★ Mark as favorite").clicked() {
                                actions.fav_action = Some(FavAction::Add(e));
                                ui.close_menu();
                            }
                        }
                    });
                }
                // Favorites that aren't open — dimmed; click relaunches.
                if !dimmed.is_empty() {
                    if !self.items.is_empty() {
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(2.0);
                    }
                    for fav in &dimmed {
                        let resp = rows::fav_row(ui, fav, &name_font);
                        if resp.clicked() {
                            actions.fav_launch = Some(fav.clone());
                        }
                        resp.context_menu(|ui| {
                            if ui.button("☆ Unfavorite").clicked() {
                                actions.fav_action = Some(FavAction::Remove(fav.clone()));
                                ui.close_menu();
                            }
                        });
                    }
                }
            });
    }

    /// Edge tab: named windows first, separator, then unnamed (tab-title-derived).
    fn ui_edge_tab(&self, ui: &mut egui::Ui, actions: &mut Actions) {
        if self.edge.is_empty() {
            ui.add_space(12.0);
            ui.weak("No Edge windows open.");
            return;
        }
        let name_font = fonts::name_font(self.has_bold);
        let has_named = self.edge.iter().any(|w| w.named);
        let has_unnamed = self.edge.iter().any(|w| !w.named);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for w in self.edge.iter().filter(|w| w.named) {
                    if rows::edge_row(ui, w, &name_font).clicked() {
                        actions.focus = Some(w.hwnd);
                    }
                }
                if has_named && has_unnamed {
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(2.0);
                }
                for w in self.edge.iter().filter(|w| !w.named) {
                    if rows::edge_row(ui, w, &name_font).clicked() {
                        actions.focus = Some(w.hwnd);
                    }
                }
            });
    }

    /// Apps tab: configured apps — running → focus, not running → launch.
    fn ui_apps_tab(&self, ui: &mut egui::Ui, actions: &mut Actions) {
        if self.apps.is_empty() {
            ui.add_space(12.0);
            ui.weak("No apps configured.");
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Add one with the kvscf-add-app skill (writes\n\
                     HKCU\\Software\\kenhia\\kvscf\\apps).",
                )
                .small()
                .weak(),
            );
            return;
        }
        let name_font = fonts::name_font(self.has_bold);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for entry in &self.apps {
                    if rows::app_row(ui, entry, &name_font).clicked() {
                        match entry.hwnd {
                            // Running → focus it (like Code/Edge).
                            Some(hwnd) => actions.focus = Some(hwnd),
                            // Not running → launch, then foreground on appearance.
                            None => {
                                actions.launch = Some((entry.launch.clone(), entry.matcher.clone()))
                            }
                        }
                    }
                }
            });
    }

    /// Apply the frame's collected click actions, now that the list borrows are released.
    fn apply_actions(&mut self, actions: Actions) {
        if let Some(hwnd) = actions.focus {
            focus_with(hwnd, self.maximize_on_focus);
            // Auto-hide only makes sense as a floating window; a docked bar keeps its space.
            if self.auto_hide && !self.docked {
                self.hide_at = Some(Instant::now() + AUTO_HIDE_DELAY);
            }
        }
        if let Some((spec, matcher)) = actions.launch {
            // Launch on a background thread; it polls for the window and foregrounds it.
            // No auto-hide — a cold-launching app can take many seconds to appear.
            launch_and_focus(&spec, &matcher);
        }
        // Relaunch a clicked dimmed favorite (sprint 008).
        if let Some(entry) = actions.fav_launch {
            let _ = winset::launch(&entry);
        }
        // Apply a favorites mutation from a right-click.
        match actions.fav_action {
            Some(FavAction::Add(e)) => self.add_favorite(e),
            Some(FavAction::Remove(e)) => self.remove_favorite(&e),
            Some(FavAction::Close(hwnd)) => {
                close_window(hwnd);
            }
            None => {}
        }
    }
}

impl eframe::App for KvscfApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Capture our native handle once it exists, then apply the persisted mode.
        if self.hwnd.is_none() {
            self.hwnd = window_hwnd(frame);
        }
        if !self.mode_applied && self.hwnd.is_some() {
            self.apply_mode(ctx);
            self.mode_applied = true;
        }
        // While docked, keep re-asserting the reserved band (covers taskbar/res changes) and yield
        // z-order to any fullscreen app on our monitor, taskbar-style (WI #481).
        if self.docked && self.appbar_registered && self.last_dock_assert.elapsed() >= DOCK_REASSERT
        {
            self.update_fullscreen_yield();
            self.reassert_dock(ctx);
        }

        // Fire a pending self-minimize once its delay elapses.
        if let Some(when) = self.hide_at {
            if Instant::now() >= when {
                self.hide_at = None;
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            }
        }

        if self.last_scan.elapsed() >= SCAN_INTERVAL {
            self.refresh();
        }

        self.ui_tabs(ctx);
        self.ui_header(ctx);
        // Save/Restore + Update Assist are VS-Code-specific — Code tab only.
        if self.tab == Tab::Code {
            self.ui_update_assist(ctx);
        }

        let mut actions = Actions::default();
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Code => self.ui_code_tab(ui, &mut actions),
            Tab::Edge => self.ui_edge_tab(ui, &mut actions),
            Tab::Apps => self.ui_apps_tab(ui, &mut actions),
        });
        self.apply_actions(actions);

        // Keep polling / countdown ticking even when idle.
        ctx.request_repaint_after(Duration::from_millis(400));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Release the reserved edge so we don't leave a dead band behind.
        if self.appbar_registered {
            if let Some(hwnd) = self.hwnd {
                dock::remove(hwnd);
                self.appbar_registered = false;
            }
        }
    }
}

/// Our native window handle (Win32 HWND as isize), if available.
fn window_hwnd(frame: &eframe::Frame) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match frame.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}
