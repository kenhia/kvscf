//! The one row painter behind every list row (WI #493). All four row kinds (Code window,
//! dimmed favorite, Edge window, configured app) are thin specs over [`draw`]: an optional
//! marker in a reserved left gutter, a truncating main text, and an optional never-truncated
//! trailing text (the remote host). Colors and dimensions come from [`crate::theme`].

use eframe::egui::{self, Color32, FontId, Sense, TextFormat};
use egui::text::LayoutJob;

use kvscf_core::{EdgeWindow, Instance, Remote};

use crate::apps::AppEntry;
use crate::fonts;
use crate::theme::{self, dims};
use crate::winset::SetEntry;

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
    /// This row's window is the current foreground window — paint a thin accent bar (WI #502).
    pub active: bool,
}

/// Draw one left-aligned, full-width clickable row from a [`RowSpec`].
pub fn draw(ui: &mut egui::Ui, spec: RowSpec<'_>) -> egui::Response {
    let width = ui.available_width();
    let gutter = if spec.gutter { dims::GUTTER } else { 0.0 };

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
        job.wrap.max_width = (width - dims::ROW_PAD * 2.0 - gutter - trail_w).max(24.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = false;
        job.wrap.overflow_character = Some('…');
        ui.fonts(|f| f.layout_job(job))
    };

    let main_w = main_galley.size().x;
    let main_h = main_galley.size().y;
    let trail_h = trail_galley.as_ref().map(|g| g.size().y).unwrap_or(0.0);
    let row_h = (main_h.max(trail_h) + 8.0).max(dims::ROW_MIN_H);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::click());

    if ui.is_rect_visible(rect) {
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        // Active-window indicator: a thin accent bar hugging the row's left edge.
        if spec.active {
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top() + 3.0),
                egui::pos2(rect.left() + dims::ACTIVE_BAR_W, rect.bottom() - 3.0),
            );
            ui.painter()
                .rect_filled(bar, dims::ACTIVE_BAR_W / 2.0, spec.color);
        }
        if let Some((glyph, color)) = spec.marker {
            let g = ui.fonts(|f| {
                f.layout_no_wrap(
                    glyph.to_string(),
                    FontId::proportional(dims::MARKER_SIZE),
                    color,
                )
            });
            ui.painter().galley(
                egui::pos2(
                    rect.left() + dims::ROW_PAD,
                    rect.center().y - g.size().y / 2.0,
                ),
                g,
                Color32::PLACEHOLDER,
            );
        }
        let x = rect.left() + dims::ROW_PAD + gutter;
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
    active: bool,
) -> egui::Response {
    let p = theme::palette(ui.visuals().dark_mode);
    let resp = draw(
        ui,
        RowSpec {
            gutter: true,
            marker: favorited.then_some(("★", p.fav_star)),
            text: &item.workspace,
            font: name_font.clone(),
            color: p.app(item.app),
            trail: item
                .remote
                .host()
                .map(|h| (format!("  {h}"), fonts::host_font(), p.host)),
            active,
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
    let p = theme::palette(ui.visuals().dark_mode);
    let color = p.dimmed(p.app(fav.app));
    draw(
        ui,
        RowSpec {
            gutter: true,
            marker: Some(("○", color)),
            text: &fav.label,
            font: name_font.clone(),
            color,
            trail: None,
            active: false,
        },
    )
    .on_hover_text(format!("{}\nnot open — click to relaunch", fav.label))
}

/// An Edge window: named windows in the Edge-teal accent, unnamed (tab-derived) muted.
pub fn edge_row(
    ui: &mut egui::Ui,
    w: &EdgeWindow,
    name_font: &FontId,
    active: bool,
) -> egui::Response {
    let p = theme::palette(ui.visuals().dark_mode);
    let (color, font) = if w.named {
        (p.edge, name_font.clone())
    } else {
        (p.edge_unnamed, FontId::proportional(13.5))
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
            active,
        },
    );
    match w.tab_count.filter(|&n| n > 1) {
        Some(n) => resp.on_hover_text(format!("{} — {} tabs", w.label, n)),
        None => resp.on_hover_text(&w.label),
    }
}

/// A configured app: running → full color + ● (click focuses); not running → dimmed + ○
/// (click launches).
pub fn app_row(
    ui: &mut egui::Ui,
    entry: &AppEntry,
    name_font: &FontId,
    active: bool,
) -> egui::Response {
    let p = theme::palette(ui.visuals().dark_mode);
    let color = if entry.running { p.stable } else { p.dim };
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
            active,
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
