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

mod apps;
mod dock;
mod winset;

#[cfg(windows)]
mod userreg;

#[cfg(feature = "remote")]
mod remote;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::text::LayoutJob;
use egui::{Color32, FontFamily, FontId, Sense, TextFormat, ViewportCommand, WindowLevel};

use kvscf_core::{
    close_window, focus_with, launch_and_focus, scan_all, App, EdgeWindow, Instance, Remote,
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

const RAIL_WIDTH: f32 = 280.0;
const RAIL_HEIGHT: f32 = 1040.0;
const SCAN_INTERVAL: Duration = Duration::from_millis(1000);
const AUTO_HIDE_DELAY: Duration = Duration::from_secs(2);
const DOCK_REASSERT: Duration = Duration::from_secs(1);
const BOLD_FAMILY: &str = "kvscf-bold";
/// Left gutter reserved on every Code row for the favorite ★ / not-open ○ marker, so names stay
/// aligned whether or not a row is marked (sprint 008).
const FAV_GUTTER: f32 = 15.0;

/// Whether this build includes the remote (kdeskdash) channel.
#[cfg(feature = "remote")]
pub const REMOTE_BUILD: bool = true;
#[cfg(not(feature = "remote"))]
pub const REMOTE_BUILD: bool = false;

/// The window title / build identity: `kvscf` (remote) or `kvscf-local` (no comms).
const APP_TITLE: &str = if REMOTE_BUILD { "kvscf" } else { "kvscf-local" };

/// Run the app. Called by the thin `kvscf` / `kvscf-local` bin crates.
pub fn run() -> eframe::Result<()> {
    // Headless probe to confirm which build this is (guards the feature-unification trap).
    if std::env::args().any(|a| a == "--build-info") {
        println!("{APP_TITLE} (remote={REMOTE_BUILD})");
        return Ok(());
    }

    // Headless probe: load the Apps config and resolve running state (sprint 007 verification).
    if std::env::args().any(|a| a == "--dump-apps") {
        let entries = apps::scan();
        if entries.is_empty() {
            println!("(no apps configured under HKCU\\Software\\kenhia\\kvscf\\apps)");
        }
        for e in &entries {
            let state = match e.hwnd {
                Some(h) => format!("running  hwnd={h}"),
                None => "not running".to_string(),
            };
            println!(
                "{:<16} {:<12} launch={:?}:{}",
                e.label, state, e.launch.kind, e.launch.target
            );
        }
        return Ok(());
    }

    // Headless probe: resolve open windows -> folder URIs (WI #469 verification).
    if std::env::args().any(|a| a == "--dump-set") {
        let (resolved, unresolved) = winset::resolve_open_set();
        for (_, e) in &resolved {
            println!("{:<34} {:<10} {}", e.label, format!("{:?}", e.app), e.uri);
        }
        if !unresolved.is_empty() {
            println!("\nUNRESOLVED ({}): {:?}", unresolved.len(), unresolved);
        }
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
    ua_state: UaState,
    ua_relaunch: Vec<winset::SetEntry>,
    ua_closed: usize,
    ua_status: String,
    #[cfg(feature = "remote")]
    channel: Option<remote::Channel>,
}

impl KvscfApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let has_bold = install_bold_font(&cc.egui_ctx);
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
        edge.sort_by(|a, b| {
            b.named
                .cmp(&a.named)
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        });
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
            let (resolved, _unresolved) = winset::resolve_open_set();
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
        use std::collections::HashMap;
        let (resolved, _unresolved) = winset::resolve_open_set();
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

    fn name_font(&self) -> FontId {
        if self.has_bold {
            FontId::new(14.5, FontFamily::Name(Arc::from(BOLD_FAMILY)))
        } else {
            FontId::proportional(14.5)
        }
    }

    fn host_font(&self) -> FontId {
        FontId::proportional(13.0)
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
        // While docked, keep re-asserting the reserved band (covers taskbar/res changes).
        if self.docked && self.appbar_registered && self.last_dock_assert.elapsed() >= DOCK_REASSERT
        {
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

        // Save/Restore + Update Assist are VS-Code-specific — Code tab only.
        if self.tab == Tab::Code {
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
                                let (resolved, _) = winset::resolve_open_set();
                                let entries: Vec<_> =
                                    resolved.into_iter().map(|(_, e)| e).collect();
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
                            egui::RichText::new(
                                "Keep one window per host × build, close the rest?",
                            )
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

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut clicked: Option<i64> = None;
            let mut launch: Option<(kvscf_core::LaunchSpec, kvscf_core::AppMatcher)> = None;
            let mut fav_action: Option<FavAction> = None;
            let mut fav_launch: Option<winset::SetEntry> = None;
            match self.tab {
                Tab::Code => {
                    let dimmed = self.dimmed_favorites();
                    if self.items.is_empty() && dimmed.is_empty() {
                        ui.add_space(12.0);
                        ui.weak("No VS Code windows open.");
                    } else {
                        let name_font = self.name_font();
                        let host_font = self.host_font();
                        let dark = ui.visuals().dark_mode;
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
                                    let resp =
                                        draw_row(ui, item, &name_font, &host_font, favorited);
                                    if resp.clicked() {
                                        clicked = Some(item.hwnd);
                                    }
                                    resp.context_menu(|ui| match entry {
                                        None => {
                                            ui.add_enabled(
                                                false,
                                                egui::Button::new("★ Mark as favorite"),
                                            )
                                            .on_disabled_hover_text(
                                                "Can't resolve this window's folder",
                                            );
                                        }
                                        Some(e) if favorited => {
                                            if ui.button("☆ Unfavorite").clicked() {
                                                fav_action = Some(FavAction::Remove(e));
                                                ui.close_menu();
                                            }
                                            if ui.button("Close (keep favorite)").clicked() {
                                                fav_action = Some(FavAction::Close(hwnd));
                                                ui.close_menu();
                                            }
                                        }
                                        Some(e) => {
                                            if ui.button("★ Mark as favorite").clicked() {
                                                fav_action = Some(FavAction::Add(e));
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
                                        let resp = draw_fav_row(ui, fav, &name_font, dark);
                                        if resp.clicked() {
                                            fav_launch = Some(fav.clone());
                                        }
                                        resp.context_menu(|ui| {
                                            if ui.button("☆ Unfavorite").clicked() {
                                                fav_action = Some(FavAction::Remove(fav.clone()));
                                                ui.close_menu();
                                            }
                                        });
                                    }
                                }
                            });
                    }
                }
                Tab::Edge => {
                    if self.edge.is_empty() {
                        ui.add_space(12.0);
                        ui.weak("No Edge windows open.");
                    } else {
                        let name_font = self.name_font();
                        let dark = ui.visuals().dark_mode;
                        let has_named = self.edge.iter().any(|w| w.named);
                        let has_unnamed = self.edge.iter().any(|w| !w.named);
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                for w in self.edge.iter().filter(|w| w.named) {
                                    if draw_edge_row(ui, w, &name_font, dark).clicked() {
                                        clicked = Some(w.hwnd);
                                    }
                                }
                                if has_named && has_unnamed {
                                    ui.add_space(4.0);
                                    ui.separator();
                                    ui.add_space(2.0);
                                }
                                for w in self.edge.iter().filter(|w| !w.named) {
                                    if draw_edge_row(ui, w, &name_font, dark).clicked() {
                                        clicked = Some(w.hwnd);
                                    }
                                }
                            });
                    }
                }
                Tab::Apps => {
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
                    } else {
                        let name_font = self.name_font();
                        let dark = ui.visuals().dark_mode;
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                for entry in &self.apps {
                                    if draw_app_row(ui, entry, &name_font, dark).clicked() {
                                        match entry.hwnd {
                                            // Running → focus it (like Code/Edge).
                                            Some(hwnd) => clicked = Some(hwnd),
                                            // Not running → launch, then foreground on appearance.
                                            None => {
                                                launch = Some((
                                                    entry.launch.clone(),
                                                    entry.matcher.clone(),
                                                ))
                                            }
                                        }
                                    }
                                }
                            });
                    }
                }
            }
            if let Some(hwnd) = clicked {
                focus_with(hwnd, self.maximize_on_focus);
                // Auto-hide only makes sense as a floating window; a docked bar keeps its space.
                if self.auto_hide && !self.docked {
                    self.hide_at = Some(Instant::now() + AUTO_HIDE_DELAY);
                }
            }
            if let Some((spec, matcher)) = launch {
                // Launch on a background thread; it polls for the window and foregrounds it.
                // No auto-hide — a cold-launching app can take many seconds to appear.
                launch_and_focus(&spec, &matcher);
            }
            // Relaunch a clicked dimmed favorite (sprint 008).
            if let Some(entry) = fav_launch {
                let _ = winset::launch(&entry);
            }
            // Apply a favorites mutation from a right-click.
            match fav_action {
                Some(FavAction::Add(e)) => self.add_favorite(e),
                Some(FavAction::Remove(e)) => self.remove_favorite(&e),
                Some(FavAction::Close(hwnd)) => {
                    close_window(hwnd);
                }
                None => {}
            }
        });

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

/// Draw one left-aligned, full-width clickable row. The **name** (build-colored, bold/mono)
/// is truncated with `…` if needed, but the **host** (italic, muted) is always kept, e.g.
/// `generative_ai_w… kai`.
fn draw_row(
    ui: &mut egui::Ui,
    item: &Instance,
    name_font: &FontId,
    host_font: &FontId,
    favorited: bool,
) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let width = ui.available_width();
    let pad = 8.0;

    // Host galley first (full width, never truncated), so we know how much room the name gets.
    let host_galley = item.remote.host().map(|host| {
        let mut job = LayoutJob::default();
        job.append(
            &format!("  {host}"),
            0.0,
            TextFormat {
                color: host_color(dark),
                font_id: host_font.clone(),
                italics: true,
                ..Default::default()
            },
        );
        ui.fonts(|f| f.layout_job(job))
    });
    let host_w = host_galley.as_ref().map(|g| g.size().x).unwrap_or(0.0);

    // Name galley, truncated to the remaining width (less the ★ gutter).
    let avail_name = (width - pad * 2.0 - FAV_GUTTER - host_w).max(24.0);
    let name_galley = {
        let mut job = LayoutJob::default();
        job.append(
            &item.workspace,
            0.0,
            TextFormat {
                color: app_color(item.app, dark),
                font_id: name_font.clone(),
                ..Default::default()
            },
        );
        job.wrap.max_width = avail_name;
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = false;
        job.wrap.overflow_character = Some('…');
        ui.fonts(|f| f.layout_job(job))
    };

    let name_w = name_galley.size().x;
    let name_h = name_galley.size().y;
    let host_h = host_galley.as_ref().map(|g| g.size().y).unwrap_or(0.0);
    let row_h = (name_h.max(host_h) + 8.0).max(24.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::click());

    if ui.is_rect_visible(rect) {
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        // ★ in the reserved gutter for favorited windows; the gutter is always reserved so
        // marked and unmarked rows stay left-aligned with each other.
        if favorited {
            let star = ui.fonts(|f| {
                f.layout_no_wrap(
                    "★".to_string(),
                    FontId::proportional(11.0),
                    fav_star_color(dark),
                )
            });
            ui.painter().galley(
                egui::pos2(rect.left() + pad, rect.center().y - star.size().y / 2.0),
                star,
                Color32::PLACEHOLDER,
            );
        }
        let nx = rect.left() + pad + FAV_GUTTER;
        ui.painter().galley(
            egui::pos2(nx, rect.center().y - name_h / 2.0),
            name_galley,
            Color32::PLACEHOLDER,
        );
        if let Some(hg) = host_galley {
            ui.painter().galley(
                egui::pos2(nx + name_w, rect.center().y - host_h / 2.0),
                hg,
                Color32::PLACEHOLDER,
            );
        }
    }
    let mut tip = hover_text(item);
    if favorited {
        tip.push_str("\n★ favorite");
    }
    resp.on_hover_text(tip)
}

/// Gold accent for the favorite ★.
fn fav_star_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(230, 185, 70)
    } else {
        Color32::from_rgb(185, 140, 20)
    }
}

/// Draw one dimmed "favorite not currently open" row (sprint 008): a ○ dot + the label in a muted,
/// build-tinted color. Clicking relaunches it (`code --folder-uri`).
fn draw_fav_row(
    ui: &mut egui::Ui,
    fav: &winset::SetEntry,
    name_font: &FontId,
    dark: bool,
) -> egui::Response {
    let width = ui.available_width();
    let pad = 8.0;
    let color = fav_dim_color(fav.app, dark);

    // Label only — the ○ is painted into the same reserved gutter the ★ uses, so dimmed rows
    // line up with the running ones above them.
    let mut job = LayoutJob::default();
    job.append(
        &fav.label,
        0.0,
        TextFormat {
            color,
            font_id: name_font.clone(),
            ..Default::default()
        },
    );
    job.wrap.max_width = (width - pad * 2.0 - FAV_GUTTER).max(24.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');

    let galley = ui.fonts(|f| f.layout_job(job));
    let row_h = (galley.size().y + 8.0).max(24.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::click());
    if ui.is_rect_visible(rect) {
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        let dot =
            ui.fonts(|f| f.layout_no_wrap("○".to_string(), FontId::proportional(11.0), color));
        ui.painter().galley(
            egui::pos2(rect.left() + pad, rect.center().y - dot.size().y / 2.0),
            dot,
            Color32::PLACEHOLDER,
        );
        ui.painter().galley(
            egui::pos2(
                rect.left() + pad + FAV_GUTTER,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Color32::PLACEHOLDER,
        );
    }
    resp.on_hover_text(format!("{}\nnot open — click to relaunch", fav.label))
}

/// A muted, build-tinted color for a not-open favorite — the build accent blended halfway to gray,
/// so Insiders favorites still read greenish and Stable bluish while clearly dimmed.
fn fav_dim_color(app: App, dark: bool) -> Color32 {
    let base = app_color(app, dark);
    let g: u16 = if dark { 90 } else { 165 };
    Color32::from_rgb(
        ((base.r() as u16 + g) / 2) as u8,
        ((base.g() as u16 + g) / 2) as u8,
        ((base.b() as u16 + g) / 2) as u8,
    )
}

/// Accent color per VS Code build — applied to the workspace name.
fn app_color(app: App, dark: bool) -> Color32 {
    match app {
        App::Insiders => Color32::from_rgb(56, 190, 132), // green
        App::Exploration => Color32::from_rgb(210, 130, 50),
        _ if dark => Color32::from_rgb(96, 165, 235), // Stable — blue
        _ => Color32::from_rgb(24, 108, 198),
    }
}

fn host_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(150)
    } else {
        Color32::from_gray(110)
    }
}

/// Draw one Edge window row — named windows in an Edge-teal accent, unnamed (tab-derived) muted.
fn draw_edge_row(
    ui: &mut egui::Ui,
    w: &EdgeWindow,
    name_font: &FontId,
    dark: bool,
) -> egui::Response {
    let width = ui.available_width();
    let pad = 8.0;
    let (color, font) = if w.named {
        (edge_named_color(dark), name_font.clone())
    } else {
        (edge_unnamed_color(dark), FontId::proportional(13.5))
    };

    let mut job = LayoutJob::default();
    job.append(
        &w.label,
        0.0,
        TextFormat {
            color,
            font_id: font,
            ..Default::default()
        },
    );
    job.wrap.max_width = width - pad * 2.0;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');

    let galley = ui.fonts(|f| f.layout_job(job));
    let row_h = (galley.size().y + 8.0).max(24.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::click());
    if ui.is_rect_visible(rect) {
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        ui.painter().galley(
            egui::pos2(rect.left() + pad, rect.center().y - galley.size().y / 2.0),
            galley,
            Color32::PLACEHOLDER,
        );
    }
    let tabs = w.tab_count.filter(|&n| n > 1);
    match tabs {
        Some(n) => resp.on_hover_text(format!("{} — {} tabs", w.label, n)),
        None => resp.on_hover_text(&w.label),
    }
}

/// Draw one Apps row. A **running** app shows in full color with a small ● dot; a **not-running**
/// app is dimmed with a ○ dot — a click focuses the former, launches the latter.
fn draw_app_row(
    ui: &mut egui::Ui,
    entry: &AppEntry,
    name_font: &FontId,
    dark: bool,
) -> egui::Response {
    let width = ui.available_width();
    let pad = 8.0;
    let color = if entry.running {
        app_running_color(dark)
    } else {
        app_dim_color(dark)
    };
    let dot = if entry.running { "● " } else { "○ " };

    let mut job = LayoutJob::default();
    job.append(
        dot,
        0.0,
        TextFormat {
            color,
            font_id: FontId::proportional(11.0),
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job.append(
        &entry.label,
        0.0,
        TextFormat {
            color,
            font_id: name_font.clone(),
            ..Default::default()
        },
    );
    job.wrap.max_width = width - pad * 2.0;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');

    let galley = ui.fonts(|f| f.layout_job(job));
    let row_h = (galley.size().y + 8.0).max(24.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::click());
    if ui.is_rect_visible(rect) {
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        ui.painter().galley(
            egui::pos2(rect.left() + pad, rect.center().y - galley.size().y / 2.0),
            galley,
            Color32::PLACEHOLDER,
        );
    }
    let hint = if entry.running {
        "running — click to focus"
    } else {
        "not running — click to launch"
    };
    resp.on_hover_text(format!("{} ({})\n{hint}", entry.label, entry.key))
}

/// Full-strength color for a running app (blue, matching Code's stable accent).
fn app_running_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(96, 165, 235)
    } else {
        Color32::from_rgb(24, 108, 198)
    }
}

/// Muted color for a not-running (launchable) app — greyed out, per the dashboard convention.
fn app_dim_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(120)
    } else {
        Color32::from_gray(150)
    }
}

fn edge_named_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(72, 194, 205) // Edge teal
    } else {
        Color32::from_rgb(20, 120, 130)
    }
}

fn edge_unnamed_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(190)
    } else {
        Color32::from_gray(70)
    }
}

fn hover_text(item: &Instance) -> String {
    let kind = match &item.remote {
        Remote::Local => "local".to_string(),
        Remote::Ssh(h) => format!("SSH: {h}"),
        Remote::Wsl(h) => format!("WSL: {h}"),
        Remote::DevContainer(h) => format!("Dev Container: {h}"),
        Remote::Codespaces(h) => format!("Codespaces: {h}"),
    };
    let active = item.active_file.as_deref().unwrap_or("—");
    format!(
        "{} [{}]\n{}\n{}",
        item.label(),
        item.app.label(),
        kind,
        active
    )
}

/// Load Segoe UI Bold from the system fonts dir and register it as [`BOLD_FAMILY`].
/// Returns whether it was available (fallback: regular proportional).
fn install_bold_font(ctx: &egui::Context) -> bool {
    // Candidate bold faces present on stock Windows, in preference order.
    let candidates = [
        r"C:\Windows\Fonts\segoeuib.ttf", // Segoe UI Bold
        r"C:\Windows\Fonts\seguisb.ttf",  // Segoe UI Semibold
        r"C:\Windows\Fonts\calibrib.ttf", // Calibri Bold
        r"C:\Windows\Fonts\arialbd.ttf",  // Arial Bold
    ];
    let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) else {
        return false;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert(BOLD_FAMILY.to_owned(), egui::FontData::from_owned(bytes));
    fonts
        .families
        .entry(FontFamily::Name(Arc::from(BOLD_FAMILY)))
        .or_default()
        .push(BOLD_FAMILY.to_owned());
    ctx.set_fonts(fonts);
    true
}

/// Named-mutex single-instance guard. Returns `true` if this is the first instance.
#[cfg(windows)]
mod single_instance {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    pub fn acquire() -> bool {
        unsafe {
            match CreateMutexW(None, true, w!("Local\\kvscf-single-instance")) {
                Ok(handle) => {
                    if GetLastError() == ERROR_ALREADY_EXISTS {
                        // Another instance owns the mutex.
                        return false;
                    }
                    // HANDLE is Copy with no Drop, so the OS mutex handle stays open for the
                    // whole process lifetime (we never CloseHandle it) — exactly what we want.
                    let _ = handle;
                    true
                }
                // If we can't create the mutex, fail open rather than block startup.
                Err(_) => true,
            }
        }
    }
}

#[cfg(windows)]
mod settings {
    use crate::userreg::UserRoot;

    const PATH: &str = r"Software\kenhia\kvscf";

    pub struct Settings {
        pub maximize_on_focus: bool,
        pub auto_hide: bool,
        pub docked: bool,
    }

    pub fn load() -> Settings {
        // Defaults: everything off (auto-hide default off, per request).
        let mut s = Settings {
            maximize_on_focus: false,
            auto_hide: false,
            docked: false,
        };
        // Real user hive (see `userreg`) — a boot-cached HKCU would silently read the wrong hive.
        if let Some(key) = UserRoot::open().and_then(|u| u.key().open_subkey(PATH).ok()) {
            let get = |name: &str| key.get_value::<u32, _>(name).ok().map(|v| v != 0);
            if let Some(v) = get("maximize_on_focus") {
                s.maximize_on_focus = v;
            }
            if let Some(v) = get("auto_hide") {
                s.auto_hide = v;
            }
            if let Some(v) = get("docked") {
                s.docked = v;
            }
        }
        s
    }

    pub fn save(s: &Settings) {
        if let Some((key, _)) = UserRoot::open().and_then(|u| u.key().create_subkey(PATH).ok()) {
            let _ = key.set_value("maximize_on_focus", &(s.maximize_on_focus as u32));
            let _ = key.set_value("auto_hide", &(s.auto_hide as u32));
            let _ = key.set_value("docked", &(s.docked as u32));
        }
    }
}

#[cfg(not(windows))]
mod settings {
    pub struct Settings {
        pub maximize_on_focus: bool,
        pub auto_hide: bool,
        pub docked: bool,
    }
    pub fn load() -> Settings {
        Settings {
            maximize_on_focus: false,
            auto_hide: false,
            docked: false,
        }
    }
    pub fn save(_s: &Settings) {}
}
