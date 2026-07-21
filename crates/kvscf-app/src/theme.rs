//! Design tokens (WI #501): one named-color palette per dark/light mode, plus the shared
//! dimension constants. Everything the painters color or measure with lives here, so a design
//! change is a one-file edit.

use eframe::egui::Color32;

use kvscf_core::App;

/// Named colors for the rail, resolved for dark or light via [`palette`].
pub struct Palette {
    /// VS Code Insiders accent (green).
    pub insiders: Color32,
    /// VS Code Exploration accent (orange).
    pub exploration: Color32,
    /// VS Code Stable accent (blue) — also the running-app color on the Apps tab.
    pub stable: Color32,
    /// Edge named-window accent (teal).
    pub edge: Color32,
    /// Unnamed (tab-title-derived) Edge windows.
    pub edge_unnamed: Color32,
    /// Muted italic host suffix.
    pub host: Color32,
    /// Favorite ★ gold.
    pub fav_star: Color32,
    /// Not-running (launchable) app rows — plain gray, per the dashboard convention.
    pub dim: Color32,
    /// Gray level that [`Palette::dimmed`] blends accents toward.
    dim_blend: u16,
}

const DARK: Palette = Palette {
    insiders: Color32::from_rgb(56, 190, 132),
    exploration: Color32::from_rgb(210, 130, 50),
    stable: Color32::from_rgb(96, 165, 235),
    edge: Color32::from_rgb(72, 194, 205),
    edge_unnamed: Color32::from_gray(190),
    host: Color32::from_gray(150),
    fav_star: Color32::from_rgb(230, 185, 70),
    dim: Color32::from_gray(120),
    dim_blend: 90,
};

const LIGHT: Palette = Palette {
    insiders: Color32::from_rgb(56, 190, 132),
    exploration: Color32::from_rgb(210, 130, 50),
    stable: Color32::from_rgb(24, 108, 198),
    edge: Color32::from_rgb(20, 120, 130),
    edge_unnamed: Color32::from_gray(70),
    host: Color32::from_gray(110),
    fav_star: Color32::from_rgb(185, 140, 20),
    dim: Color32::from_gray(150),
    dim_blend: 165,
};

/// The palette for the current mode.
pub fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

impl Palette {
    /// Accent color per VS Code build — applied to the workspace name.
    pub fn app(&self, app: App) -> Color32 {
        match app {
            App::Insiders => self.insiders,
            App::Exploration => self.exploration,
            _ => self.stable,
        }
    }

    /// An accent blended halfway to gray — dimmed but still tinted, so a not-open Insiders
    /// favorite reads greenish and a Stable one bluish.
    pub fn dimmed(&self, base: Color32) -> Color32 {
        let g = self.dim_blend;
        Color32::from_rgb(
            ((base.r() as u16 + g) / 2) as u8,
            ((base.g() as u16 + g) / 2) as u8,
            ((base.b() as u16 + g) / 2) as u8,
        )
    }
}

/// Shared dimensions — the spacing scale the chrome and rows draw from.
pub mod dims {
    /// Horizontal padding inside a row.
    pub const ROW_PAD: f32 = 8.0;
    /// Minimum row height.
    pub const ROW_MIN_H: f32 = 24.0;
    /// Left gutter reserved for the marker (★ ● ○) so marked and unmarked siblings align.
    pub const GUTTER: f32 = 15.0;
    /// Marker glyph size.
    pub const MARKER_SIZE: f32 = 11.0;
    /// Vertical gap between rows.
    pub const ROW_GAP: f32 = 1.0;
    /// Breathing room at panel edges.
    pub const PANEL_PAD: f32 = 4.0;
    /// Gap around a section separator (running ↔ dimmed, named ↔ unnamed).
    pub const SECTION_GAP: f32 = 4.0;
    /// Width of the active-window accent bar at a row's left edge.
    pub const ACTIVE_BAR_W: f32 = 2.5;
}
