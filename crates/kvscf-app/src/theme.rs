//! Colors for the rail, per dark/light mode. Moved here from lib.rs (WI #496); sprint 014
//! restructures these into a palette of named tokens (WI #501).

use eframe::egui::Color32;

use kvscf_core::App;

/// Accent color per VS Code build — applied to the workspace name.
pub fn app_color(app: App, dark: bool) -> Color32 {
    match app {
        App::Insiders => Color32::from_rgb(56, 190, 132), // green
        App::Exploration => Color32::from_rgb(210, 130, 50),
        _ if dark => Color32::from_rgb(96, 165, 235), // Stable — blue
        _ => Color32::from_rgb(24, 108, 198),
    }
}

pub fn host_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(150)
    } else {
        Color32::from_gray(110)
    }
}

/// Gold accent for the favorite ★.
pub fn fav_star_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(230, 185, 70)
    } else {
        Color32::from_rgb(185, 140, 20)
    }
}

/// A muted, build-tinted color for a not-open favorite — the build accent blended halfway to gray,
/// so Insiders favorites still read greenish and Stable bluish while clearly dimmed.
pub fn fav_dim_color(app: App, dark: bool) -> Color32 {
    let base = app_color(app, dark);
    let g: u16 = if dark { 90 } else { 165 };
    Color32::from_rgb(
        ((base.r() as u16 + g) / 2) as u8,
        ((base.g() as u16 + g) / 2) as u8,
        ((base.b() as u16 + g) / 2) as u8,
    )
}

/// Full-strength color for a running app (blue, matching Code's stable accent).
pub fn app_running_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(96, 165, 235)
    } else {
        Color32::from_rgb(24, 108, 198)
    }
}

/// Muted color for a not-running (launchable) app — greyed out, per the dashboard convention.
pub fn app_dim_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(120)
    } else {
        Color32::from_gray(150)
    }
}

pub fn edge_named_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(72, 194, 205) // Edge teal
    } else {
        Color32::from_rgb(20, 120, 130)
    }
}

pub fn edge_unnamed_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(190)
    } else {
        Color32::from_gray(70)
    }
}
