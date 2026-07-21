//! The one row painter behind every list row (WI #493). All four row kinds (Code window,
//! dimmed favorite, Edge window, configured app) are thin specs over [`draw`]: an optional
//! marker in a reserved left gutter, a truncating main text, and an optional never-truncated
//! trailing text (the remote host).

use eframe::egui::{self, Color32, FontId, Sense, TextFormat};
use egui::text::LayoutJob;

use kvscf_core::{EdgeWindow, Instance, Remote};

use crate::apps::AppEntry;
use crate::winset::SetEntry;
use crate::{fonts, theme};

/// Horizontal padding inside a row.
pub const PAD: f32 = 8.0;
/// Minimum row height.
pub const MIN_ROW_H: f32 = 24.0;
/// Left gutter reserved for the marker (★ ● ○) so marked and unmarked siblings stay aligned.
pub const GUTTER: f32 = 15.0;
/// Marker glyph size.
const MARKER_SIZE: f32 = 11.0;

/// One row's ingredients. `text` truncates with `…`; `trail` never truncates.
pub struct RowSpec<'a> {
    /// Reserve the marker gutter (even with no marker, so sibling rows align).
    pub gutter: bool,
    /// Marker glyph painted centered in the gutter.
    pub marker: Option<(&'a str, Color32)>,
    pub text: &'a str,
    pub font: FontId,
    pub color: Color32,
    /// Trailing italic text (the remote host), kept in full at the main text's expense.
    pub trail: Option<(String, FontId, Color32)>,
}

/// Draw one left-aligned, full-width clickable row from a [`RowSpec`].
pub fn draw(ui: &mut egui::Ui, spec: RowSpec<'_>) -> egui::Response {
    let width = ui.available_width();
    let gutter = if spec.gutter { GUTTER } else { 0.0 };

    // Trailing galley first (never truncated), so we know how much room the main text gets.
    let trail_galley = spec.trail.map(|(text, font_id, color)| {
        let mut job = LayoutJob::default();
        job.append(
            &text,
            0.0,
            TextFormat {
                color,
                font_id,
                italics: true,
                ..Default::default()
            },
        );
        ui.fonts(|f| f.layout_job(job))
    });
    let trail_w = trail_galley.as_ref().map(|g| g.size().x).unwrap_or(0.0);

    // Main galley, truncated to the remaining width.
    let main_galley = {
        let mut job = LayoutJob::default();
        job.append(
            spec.text,
            0.0,
            TextFormat {
                color: spec.color,
                font_id: spec.font,
                ..Default::default()
            },
        );
        job.wrap.max_width = (width - PAD * 2.0 - gutter - trail_w).max(24.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = false;
        job.wrap.overflow_character = Some('…');
        ui.fonts(|f| f.layout_job(job))
    };

    let main_w = main_galley.size().x;
    let main_h = main_galley.size().y;
    let trail_h = trail_galley.as_ref().map(|g| g.size().y).unwrap_or(0.0);
    let row_h = (main_h.max(trail_h) + 8.0).max(MIN_ROW_H);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::click());

    if ui.is_rect_visible(rect) {
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        if let Some((glyph, color)) = spec.marker {
            let g = ui.fonts(|f| {
                f.layout_no_wrap(glyph.to_string(), FontId::proportional(MARKER_SIZE), color)
            });
            ui.painter().galley(
                egui::pos2(rect.left() + PAD, rect.center().y - g.size().y / 2.0),
                g,
                Color32::PLACEHOLDER,
            );
        }
        let x = rect.left() + PAD + gutter;
        ui.painter().galley(
            egui::pos2(x, rect.center().y - main_h / 2.0),
            main_galley,
            Color32::PLACEHOLDER,
        );
        if let Some(g) = trail_galley {
            ui.painter().galley(
                egui::pos2(x + main_w, rect.center().y - trail_h / 2.0),
                g,
                Color32::PLACEHOLDER,
            );
        }
    }
    resp
}

/// A running VS Code window: build-colored bold name, host kept in full, ★ when favorited.
pub fn code_row(
    ui: &mut egui::Ui,
    item: &Instance,
    name_font: &FontId,
    favorited: bool,
) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let resp = draw(
        ui,
        RowSpec {
            gutter: true,
            marker: favorited.then(|| ("★", theme::fav_star_color(dark))),
            text: &item.workspace,
            font: name_font.clone(),
            color: theme::app_color(item.app, dark),
            trail: item.remote.host().map(|h| {
                (
                    format!("  {h}"),
                    fonts::host_font(),
                    theme::host_color(dark),
                )
            }),
        },
    );
    let mut tip = hover_text(item);
    if favorited {
        tip.push_str("\n★ favorite");
    }
    resp.on_hover_text(tip)
}

/// A favorite that isn't open: ○ marker + muted build-tinted label; click relaunches.
pub fn fav_row(ui: &mut egui::Ui, fav: &SetEntry, name_font: &FontId) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let color = theme::fav_dim_color(fav.app, dark);
    draw(
        ui,
        RowSpec {
            gutter: true,
            marker: Some(("○", color)),
            text: &fav.label,
            font: name_font.clone(),
            color,
            trail: None,
        },
    )
    .on_hover_text(format!("{}\nnot open — click to relaunch", fav.label))
}

/// An Edge window: named windows in the Edge-teal accent, unnamed (tab-derived) muted.
pub fn edge_row(ui: &mut egui::Ui, w: &EdgeWindow, name_font: &FontId) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let (color, font) = if w.named {
        (theme::edge_named_color(dark), name_font.clone())
    } else {
        (theme::edge_unnamed_color(dark), FontId::proportional(13.5))
    };
    let resp = draw(
        ui,
        RowSpec {
            gutter: false,
            marker: None,
            text: &w.label,
            font,
            color,
            trail: None,
        },
    );
    match w.tab_count.filter(|&n| n > 1) {
        Some(n) => resp.on_hover_text(format!("{} — {} tabs", w.label, n)),
        None => resp.on_hover_text(&w.label),
    }
}

/// A configured app: running → full color + ● (click focuses); not running → dimmed + ○
/// (click launches).
pub fn app_row(ui: &mut egui::Ui, entry: &AppEntry, name_font: &FontId) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let color = if entry.running {
        theme::app_running_color(dark)
    } else {
        theme::app_dim_color(dark)
    };
    let dot = if entry.running { "●" } else { "○" };
    let hint = if entry.running {
        "running — click to focus"
    } else {
        "not running — click to launch"
    };
    draw(
        ui,
        RowSpec {
            gutter: true,
            marker: Some((dot, color)),
            text: &entry.label,
            font: name_font.clone(),
            color,
            trail: None,
        },
    )
    .on_hover_text(format!("{} ({})\n{hint}", entry.label, entry.key))
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
