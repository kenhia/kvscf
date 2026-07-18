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

mod dock;

#[cfg(feature = "remote")]
mod remote;

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::text::LayoutJob;
use egui::{Color32, FontFamily, FontId, Sense, TextFormat, ViewportCommand, WindowLevel};

use kvscf_core::{focus_with, scan, App, Instance, Remote};

const RAIL_WIDTH: f32 = 280.0;
const RAIL_HEIGHT: f32 = 1040.0;
const SCAN_INTERVAL: Duration = Duration::from_millis(1000);
const AUTO_HIDE_DELAY: Duration = Duration::from_secs(2);
const DOCK_REASSERT: Duration = Duration::from_secs(1);
const BOLD_FAMILY: &str = "kvscf-bold";

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

    // Single instance only — two docked bars would fight over the reserved edge.
    #[cfg(windows)]
    if !single_instance::acquire() {
        return Ok(());
    }

    let native_options = eframe::NativeOptions {
        persist_window: true, // remember size/position across runs
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([RAIL_WIDTH, RAIL_HEIGHT])
            .with_min_inner_size([160.0, 240.0])
            .with_position([0.0, 0.0]),
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
}

impl KvscfApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let has_bold = install_bold_font(&cc.egui_ctx);
        let s = settings::load();
        let mut app = KvscfApp {
            items: Vec::new(),
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
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        let mut items = scan();
        // Single, fastest-to-scan ordering: lowercased workspace name (hosts interleaved).
        items.sort_by_key(|i| i.workspace.to_lowercase());
        self.items = items;
        self.last_scan = Instant::now();
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
                ui.label(egui::RichText::new(format!("{} window(s)", self.items.len())).weak());
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

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.items.is_empty() {
                ui.add_space(12.0);
                ui.weak("No VS Code windows open.");
                return;
            }
            let name_font = self.name_font();
            let host_font = self.host_font();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let mut clicked: Option<i64> = None;
                    for item in &self.items {
                        if draw_row(ui, item, &name_font, &host_font).clicked() {
                            clicked = Some(item.hwnd);
                        }
                    }
                    if let Some(hwnd) = clicked {
                        focus_with(hwnd, self.maximize_on_focus);
                        // Auto-hide only makes sense as a floating window; a docked bar keeps
                        // its reserved space.
                        if self.auto_hide && !self.docked {
                            self.hide_at = Some(Instant::now() + AUTO_HIDE_DELAY);
                        }
                    }
                });
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

    // Name galley, truncated to the remaining width.
    let avail_name = (width - pad * 2.0 - host_w).max(24.0);
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
        let nx = rect.left() + pad;
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
    resp.on_hover_text(hover_text(item))
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
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

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
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(PATH) {
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
        if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(PATH) {
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
