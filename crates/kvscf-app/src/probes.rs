//! Headless verification probes, previously five separate arg scans in `run()` (WI #496).
//! Each prints to stdout and exits without opening a window.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, FontFamily, FontId};

use crate::{apps, dock, fonts, launcher, winset, APP_TITLE, REMOTE_BUILD};

/// If a probe flag (or `--help`) is present, run it and return `true` (the caller exits).
pub fn try_run() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let value_of = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    if has("--help") || has("-h") {
        print_help();
    } else if has("--build-info") {
        build_info();
    } else if has("--probe-glyphs") {
        probe_glyphs();
    } else if has("--dump-apps") {
        dump_apps();
    } else if has("--dump-launcher") {
        dump_launcher();
    } else if has("--fire-button") {
        fire_button(value_of("--fire-button"));
    } else if has("--probe-fullscreen") {
        probe_fullscreen();
    } else if has("--dump-set") {
        dump_set();
    } else {
        return false;
    }
    true
}

fn print_help() {
    println!(
        "{APP_TITLE} — VS Code window focuser\n\
         \n\
         Run with no arguments to open the rail. Headless probes:\n\
         \x20 --build-info        which build this is (remote={REMOTE_BUILD} here)\n\
         \x20 --probe-glyphs      does the bundled emoji font cover the tested glyphs?\n\
         \x20 --dump-apps         configured Apps entries + resolved running state\n\
         \x20 --dump-launcher     Launcher grid + buttons, and where each would open RIGHT NOW\n\
         \x20 --fire-button KEY   run one Launcher button now (no dashboard needed)\n\
         \x20 --probe-fullscreen  sample fullscreen detection for 20s (dock yield, WI #481)\n\
         \x20 --dump-set          open windows resolved to relaunchable folder URIs"
    );
}

/// Confirm which build this is (guards the feature-unification trap).
fn build_info() {
    println!("{APP_TITLE} (remote={REMOTE_BUILD})");
}

/// Confirm the bundled emoji font covers the glyphs Ken uses in window names (WI #489 /
/// sprint 010). `bold=true` for each means named rows render them.
fn probe_glyphs() {
    let ctx = egui::Context::default();
    let has_bold = fonts::install_bold_font(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |_| {}); // realize the font atlas
    let bold = FontId::new(14.5, FontFamily::Name(Arc::from(fonts::BOLD_FAMILY)));
    let prop = FontId::proportional(14.5);
    println!("bold family loaded: {has_bold}  (named rows use the bold family)\n");
    // crab (cleo), hammer-and-wrench + building (kwork), the FE0F selector, and two sanity
    // BMP symbols egui's subset is known to include.
    for c in ['🦀', '🛠', '🏗', '\u{FE0F}', '★', '✉'] {
        let b = ctx.fonts(|f| f.has_glyph(&bold, c));
        let p = ctx.fonts(|f| f.has_glyph(&prop, c));
        println!("U+{:05X} {:?}  bold={b:<5} proportional={p}", c as u32, c);
    }
}

/// Load the Apps config and resolve running state (sprint 007 verification).
fn dump_apps() {
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
}

/// Load the Launcher config and show, for each button, the Edge window it would open in **right
/// now** (sprint 016 verification).
///
/// Resolving live is the point: a button whose named window is closed silently degrades to the
/// top-Z fallback, and that is invisible in the config alone. This is how you tell "configured
/// correctly" from "will actually do what I meant".
fn dump_launcher() {
    let set = launcher::scan();
    println!("grid: {} rows x {} cols\n", set.grid.rows, set.grid.cols);
    if set.buttons.is_empty() {
        println!("(no buttons configured under HKCU\\Software\\kenhia\\kvscf\\launcher)");
        return;
    }

    let windows = kvscf_core::scan_edge();
    for b in &set.buttons {
        let wanted = launcher::preferred_name(&b.target);
        let hwnd = kvscf_core::pick_edge_target(&windows, wanted);
        let target = match (wanted, hwnd) {
            (_, None) => "no Edge window open — would cold-start Edge".to_string(),
            (None, Some(h)) => format!("top-Z window (hwnd={h})  [use current]"),
            (Some(name), Some(h)) => {
                let matched = windows
                    .iter()
                    .any(|w| w.named && w.label.eq_ignore_ascii_case(name) && w.hwnd == h);
                if matched {
                    format!("{name:?} (hwnd={h})")
                } else {
                    format!("{name:?} NOT OPEN -> falling back to top-Z (hwnd={h})")
                }
            }
        };
        println!(
            "{:<18} ({},{}) {}x{}  -> {}\n{:>21}{}",
            b.key, b.row, b.col, b.w, b.h, target, "", b.url
        );
    }
}

/// Fire one Launcher button immediately — the whole verb, without a dashboard or a Redis round
/// trip. The fastest way to check a newly-added button does what you meant.
fn fire_button(key: Option<String>) {
    let Some(key) = key else {
        println!("usage: --fire-button <key>   (see --dump-launcher for the configured keys)");
        return;
    };
    println!("firing launcher button {key:?} …");
    if launcher::activate(&key) {
        println!("ok");
    } else {
        println!("FAILED — see the message above (unknown key, or Edge could not be launched)");
    }
}

/// Sample fullscreen detection (WI #481 verification). Run it, then switch to the app under
/// test (WoW fullscreen / borderless, browser F11, or just a maximized window) and read the
/// samples back — `fullscreen=` is exactly what the docked rail acts on.
fn probe_fullscreen() {
    println!("Sampling the foreground window every 500ms for 20s.");
    println!("Switch to the app you want to test now; a maximized window should stay false.\n");
    for i in 0..40 {
        println!(
            "{i:>3}  fullscreen={:<5}  {}",
            dock::fullscreen_app_present(0),
            dock::describe_foreground()
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Resolve open windows -> folder URIs (WI #469 verification).
fn dump_set() {
    let (resolved, unresolved) = winset::resolve_open_set_now();
    for (_, e) in &resolved {
        println!("{:<34} {:<10} {}", e.label, format!("{:?}", e.app), e.uri);
    }
    if !unresolved.is_empty() {
        println!("\nUNRESOLVED ({}): {:?}", unresolved.len(), unresolved);
    }
}
