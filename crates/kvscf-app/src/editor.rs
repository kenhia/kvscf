//! The Launcher editor (sprint 017, korg kvscf #1135) — define a button by *placing* it.
//!
//! Fields above a live grid that shows what is already occupied, writing to
//! `HKCU\Software\kenhia\kvscf\launcher\<key>`. Deliberately built **last** of the Launcher
//! slices: a grid picker cannot be designed until the real grid has been watched on the real
//! panel at ~21.6 mm per cell.
//!
//! **Its own window, not a panel in the rail.** The rail is 280 px wide and, docked, borderless
//! and always-on-top — a nine-column grid cannot live there. So this is a second eframe
//! viewport: a real, resizable OS window that opens over whatever Ken is doing and closes again.
//!
//! **The target dropdown is why this design has no regex.** Ken asked for a pattern field, and
//! the reason turned out to be that his Edge windows are named things like `🛠️ Pipes` and he
//! did not want to type that into a config box. A list of the windows kvscf can already see
//! solves exactly that, with no pattern language, no escaping, and no drift when a window is
//! renamed. What gets stored is the window's **name**, never its HWND — handles die with the
//! window.
//!
//! **Bad placements are impossible to express here, not reported downstream.** The picker will
//! not commit a rectangle that overlaps, overflows the grid, or exceeds a 3-cell span.
//! `validate_layout` and kdeskdash's own parser stay as backstops for hand-edited registry
//! entries; they should never have anything to say about a button this wrote.

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, TextEdit, Ui};

use kvscf_core::EdgeWindow;

use crate::launcher::{
    self, LauncherButton, LauncherSet, Target, COLOR_MAX_BYTES, LABEL_MAX_BYTES, MAX_SPAN,
};

/// What the panel paints a button's label in, always: kdeskdash's `MOON_INK`. Every swatch and
/// every cell here is drawn with it, so the editor previews legibility rather than describing it.
const PANEL_INK: Color32 = Color32::from_rgb(0xe9, 0xed, 0xf6);

/// What the panel fills a button with when its `color` is empty or unrecognized:
/// kdeskdash's `DEEP_SLATE`.
const PANEL_DEFAULT_FILL: Color32 = Color32::from_rgb(0x0a, 0x0f, 0x1a);

/// The panel background, so an empty cell here looks like an empty cell there: `VOID`.
const PANEL_VOID: Color32 = Color32::from_rgb(0x05, 0x07, 0x0d);

/// kdeskdash's named palette (its `src/palette.h`), filtered to what works as a *button
/// background*. Its five text-role colors — `MOON_INK`, `STEEL_MIST`, `FADED_DENIM`,
/// `UTC_FROST`, `HOST_GREY` — are deliberately absent: the panel always draws labels in
/// near-white, so choosing one of those gives an unreadable button.
///
/// Copied rather than shared, since the two repos are built by different toolchains. That is
/// safe by construction: an unrecognized name means "use the default" on the panel (§6 of
/// `docs/kdeskdash-vscode-mode.md`), so this table drifting stale can only ever cost a color,
/// never a button. Ordered darkest-first, chrome before accents.
const BUTTON_COLORS: &[(&str, u32, &str)] = &[
    ("DEEP_SLATE", 0x0a0f1a, "panel / card fill"),
    ("VOID", 0x05070d, "the panel background itself"),
    ("RAISED_SLATE", 0x101726, "pressed / hover lift"),
    ("QUIET_KEY", 0x141c2b, "quiet key"),
    ("SLATE_KEY", 0x1a2332, "digit island"),
    ("GUNMETAL_SEAM", 0x1b2334, "hairline borders"),
    ("STEEL_KEY", 0x24344d, "binary-op keys"),
    ("SCORCHED_WASH", 0x2a1109, "blocked-on-you wash"),
    ("SMOKED_MAROON", 0x3a1b22, "muted danger"),
    ("SELECT_BLUE", 0x3d6fb0, "selected row"),
    ("BURNT_CORAL", 0x99492e, "darker coral anchor"),
    ("PATIENT_AMBER", 0xb9832c, "awaiting input / warn"),
    ("ZOMBIE_RUST", 0xc0392b, "zombie red"),
    ("CLAUDE_CORAL", 0xcf6b4a, "claude accent"),
    ("ALARM_EMBER", 0xe0563f, "hard-blocked"),
    ("WORKING_JADE", 0x35a271, "working / OK"),
    ("INSIDER_MINT", 0x38be84, "VS Code Insiders"),
    ("EDGE_TEAL", 0x2ec4c4, "Edge windows"),
    ("CODE_BLUE", 0x60a5eb, "VS Code stable"),
    ("ROCKET_RED", 0xef5350, "Apps rail"),
    ("STAR_GOLD", 0xd9a441, "favorite star"),
    ("TOP_LILAC", 0xb197fc, "top process"),
];

/// Resolve an authored color the way the panel's `kvscf_button_rgb` does — `#rrggbb` or `rrggbb`
/// hex first, then a case-insensitive palette name. `None` means the panel would fall back to
/// its default, which is exactly what the picker then draws.
fn resolve_color(color: &str) -> Option<Color32> {
    let hex = color.strip_prefix('#').unwrap_or(color);
    if hex.len() == 6 {
        if let Ok(v) = u32::from_str_radix(hex, 16) {
            return Some(from_rgb24(v));
        }
    }
    BUTTON_COLORS
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(color))
        .map(|(_, rgb, _)| from_rgb24(*rgb))
}

fn from_rgb24(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The fill the panel would actually use for an authored color.
fn fill_for(color: &str) -> Color32 {
    resolve_color(color).unwrap_or(PANEL_DEFAULT_FILL)
}

/// The form's fields — one button, as typed.
struct Form {
    key: String,
    label: String,
    url: String,
    target_named: bool,
    target_name: String,
    color: String,
    row: u32,
    col: u32,
    w: u32,
    h: u32,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            key: String::new(),
            label: String::new(),
            url: String::new(),
            target_named: false,
            target_name: String::new(),
            color: String::new(),
            // Placed by the picker; a fresh button starts unplaced-but-legal at the origin and
            // the first click moves it somewhere free.
            row: 0,
            col: 0,
            w: 1,
            h: 1,
        }
    }
}

impl Form {
    fn from_button(b: &LauncherButton) -> Self {
        let (target_named, target_name) = match &b.target {
            Target::Named(n) => (true, n.clone()),
            Target::Current => (false, String::new()),
        };
        Form {
            key: b.key.clone(),
            label: b.label.clone(),
            url: b.url.clone(),
            target_named,
            target_name,
            color: b.color.clone(),
            row: b.row,
            col: b.col,
            w: b.w,
            h: b.h,
        }
    }

    fn to_button(&self) -> LauncherButton {
        LauncherButton {
            key: self.key.trim().to_string(),
            label: self.label.clone(),
            url: self.url.trim().to_string(),
            target: if self.target_named && !self.target_name.is_empty() {
                Target::Named(self.target_name.clone())
            } else {
                Target::Current
            },
            color: self.color.clone(),
            row: self.row,
            col: self.col,
            w: self.w,
            h: self.h,
        }
    }
}

/// A one-line result of the last Save / Delete / Test, and whether it went well.
#[derive(Default)]
struct Status {
    text: String,
    ok: bool,
}

impl Status {
    fn say(&mut self, ok: bool, text: impl Into<String>) {
        self.text = text.into();
        self.ok = ok;
    }
}

/// The editor's own state. Lives in [`crate::KvscfApp`] so it survives across frames; the button
/// list and Edge windows are passed in each frame from the app's existing 1-second refresh
/// rather than re-scanned here.
#[derive(Default)]
pub struct Editor {
    pub open: bool,
    form: Form,
    /// The key of the button being edited, or `None` for a new one. Kept separate from
    /// `form.key` so renaming a button is expressible (it becomes delete-old + write-new).
    editing: Option<String>,
    /// Set the moment Ken types in the key field: after that, the key stops following the label.
    key_pinned: bool,
    /// The cell a picker drag started on.
    drag_anchor: Option<(u32, u32)>,
    status: Status,
}

impl Editor {
    /// Open the editor on a fresh button.
    pub fn open_new(&mut self) {
        self.open = true;
        self.reset();
    }

    fn reset(&mut self) {
        self.form = Form::default();
        self.editing = None;
        self.key_pinned = false;
        self.drag_anchor = None;
        self.status = Status::default();
    }

    fn load(&mut self, b: &LauncherButton) {
        self.form = Form::from_button(b);
        self.editing = Some(b.key.clone());
        // An existing button's key is already what it is; never re-derive it from the label and
        // silently rename a button the panel is already pressing.
        self.key_pinned = true;
        self.drag_anchor = None;
        self.status = Status::default();
    }

    /// Draw the editor window. Returns `true` when the registry changed, so the caller can
    /// re-scan immediately instead of waiting out the 1-second reload.
    pub fn show(&mut self, ctx: &egui::Context, set: &LauncherSet, edge: &[EdgeWindow]) -> bool {
        if !self.open {
            return false;
        }
        let viewport = egui::ViewportBuilder::default()
            .with_title("kvscf — Launcher editor")
            .with_inner_size([980.0, 700.0])
            .with_min_inner_size([620.0, 460.0]);

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("launcher_editor"),
            viewport,
            |ctx, _class| {
                let mut changed = false;
                egui::SidePanel::left("buttons")
                    .exact_width(200.0)
                    .show(ctx, |ui| self.ui_button_list(ui, set));
                egui::CentralPanel::default().show(ctx, |ui| {
                    changed = self.ui_form(ui, set, edge);
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    self.open = false;
                }
                changed
            },
        )
    }

    /// The configured buttons, as a list you pick from to edit. Each row previews its real fill
    /// and label, so this doubles as "what is on the panel right now".
    fn ui_button_list(&mut self, ui: &mut Ui, set: &LauncherSet) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading("Buttons");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("+ New")
                    .on_hover_text("Start a new button")
                    .clicked()
                {
                    self.reset();
                }
            });
        });
        ui.add_space(4.0);
        if set.buttons.is_empty() {
            ui.weak("None configured yet.");
            return;
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for b in &set.buttons {
                    let selected = self.editing.as_deref() == Some(b.key.as_str());
                    if button_list_row(ui, b, selected).clicked() {
                        self.load(b);
                    }
                }
            });
    }

    /// Fields, then the picker, then the actions. Returns `true` if the registry changed.
    fn ui_form(&mut self, ui: &mut Ui, set: &LauncherSet, edge: &[EdgeWindow]) -> bool {
        let mut changed = false;

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading(match &self.editing {
                Some(k) => format!("Editing “{k}”"),
                None => "New button".to_string(),
            });
        });
        ui.add_space(6.0);

        self.ui_fields(ui, set, edge);
        ui.add_space(8.0);
        ui.separator();

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.strong("Placement");
            ui.weak(format!(
                "— click a free cell, drag to size (up to {MAX_SPAN}×{MAX_SPAN}). \
                 Grid is {} × {}, as published.",
                set.grid.rows, set.grid.cols
            ));
        });
        ui.add_space(4.0);
        self.ui_picker(ui, set);

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        changed |= self.ui_actions(ui, set);

        if !self.status.text.is_empty() {
            ui.add_space(4.0);
            let color = if self.status.ok {
                ui.visuals().weak_text_color()
            } else {
                ui.visuals().error_fg_color
            };
            ui.label(egui::RichText::new(&self.status.text).small().color(color));
        }
        changed
    }

    fn ui_fields(&mut self, ui: &mut Ui, set: &LauncherSet, edge: &[EdgeWindow]) {
        egui::Grid::new("fields")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label("Label");
                ui.vertical(|ui| {
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.form.label)
                            .desired_width(f32::INFINITY)
                            .hint_text("Pipelines"),
                    );
                    // The key follows the label until Ken takes it over.
                    if resp.changed() && !self.key_pinned {
                        self.form.key = launcher::slugify_key(&self.form.label);
                    }
                    // Over-long labels are truncated by the panel, not dropped — a warning, and
                    // deliberately measured in *bytes*, since an emoji costs four of them.
                    if self.form.label.len() > LABEL_MAX_BYTES {
                        warn(ui, "the panel will truncate this label");
                    }
                });
                ui.end_row();

                ui.label("Key");
                ui.vertical(|ui| {
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.form.key)
                            .desired_width(f32::INFINITY)
                            .hint_text("ado-pipelines"),
                    );
                    if resp.changed() {
                        self.key_pinned = true;
                    }
                    // Checked against the set the app re-scans every second, so a button added
                    // by hand or by the `kvscf-add-app`-style skill while this window is open
                    // still counts as a clash.
                    let key = self.form.key.trim();
                    if let Some(err) =
                        launcher::key_error(key, &set.buttons, self.editing.as_deref())
                    {
                        warn(ui, &err);
                    } else if self.editing.as_deref().is_some_and(|k| k != key) {
                        warn(ui, "saving under a new key removes the old one");
                    }
                });
                ui.end_row();

                ui.label("URL");
                ui.vertical(|ui| {
                    ui.add(
                        TextEdit::singleline(&mut self.form.url)
                            .desired_width(f32::INFINITY)
                            .hint_text("https://dev.azure.com/…"),
                    );
                    if self.form.url.trim().is_empty() {
                        warn(ui, "a button without a URL is skipped");
                    }
                });
                ui.end_row();

                ui.label("Opens in");
                ui.vertical(|ui| self.ui_target(ui, edge));
                ui.end_row();

                ui.label("Color");
                ui.vertical(|ui| self.ui_color(ui));
                ui.end_row();
            });
    }

    /// The window picker — the control that made a regex field unnecessary.
    fn ui_target(&mut self, ui: &mut Ui, edge: &[EdgeWindow]) {
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.form.target_named, false, "The current window")
                .on_hover_text("Whichever Edge window is in front — also the fallback when a named one is gone");
            ui.radio_value(&mut self.form.target_named, true, "A named window");
        });
        if !self.form.target_named {
            return;
        }

        // Live named windows, plus the stored one even when it is closed right now. Dropping a
        // configured name just because its window is shut would silently rewrite the button.
        let mut names: Vec<String> = edge
            .iter()
            .filter(|w| w.named)
            .map(|w| w.label.clone())
            .collect();
        let stored_is_open = names
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&self.form.target_name));
        if !self.form.target_name.is_empty() && !stored_is_open {
            names.push(self.form.target_name.clone());
        }

        ui.add_space(2.0);
        egui::ComboBox::from_id_salt("target_name")
            .width(320.0)
            .selected_text(if self.form.target_name.is_empty() {
                "— pick a window —".to_string()
            } else {
                self.form.target_name.clone()
            })
            .show_ui(ui, |ui| {
                for name in &names {
                    let selected = &self.form.target_name == name;
                    if ui.selectable_label(selected, name).clicked() {
                        self.form.target_name = name.clone();
                    }
                }
                if names.is_empty() {
                    ui.weak("No named Edge windows open.");
                }
            });

        if self.form.target_name.is_empty() {
            warn(
                ui,
                "no window picked — this button will use the current one",
            );
        } else if !stored_is_open {
            // Not an error: the fallback is the same code path "use current" takes, so this
            // button still works. Ken should just know the name is not live right now.
            ui.label(
                egui::RichText::new(
                    "not open right now — this button would fall back to the current window",
                )
                .small()
                .weak(),
            );
        }
    }

    /// Swatches drawn the way the panel draws a button: the fill, with `MOON_INK` text on it. The
    /// point is that legibility is *shown*, not asserted — several palette entries are unusable
    /// as a background and this makes that obvious at a glance.
    fn ui_color(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            // "" = whatever the panel defaults to.
            if swatch(ui, PANEL_DEFAULT_FILL, self.form.color.is_empty())
                .on_hover_text("Panel default (DEEP_SLATE)")
                .clicked()
            {
                self.form.color.clear();
            }
            for (name, rgb, usage) in BUTTON_COLORS {
                let selected = self.form.color.eq_ignore_ascii_case(name);
                if swatch(ui, from_rgb24(*rgb), selected)
                    .on_hover_text(format!("{name}\n{usage}"))
                    .clicked()
                {
                    self.form.color = (*name).to_string();
                }
            }
        });
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.form.color)
                    .desired_width(140.0)
                    .hint_text("#2ec4c4 or a name"),
            );
            if !self.form.color.is_empty() && resolve_color(&self.form.color).is_none() {
                warn(ui, "unrecognized — the panel will use its default");
            } else if self.form.color.len() > COLOR_MAX_BYTES {
                warn(ui, "too long for the panel's color field");
            }
        });
    }

    /// The grid, drawn as the panel would draw it, with occupancy attributed to its owner.
    ///
    /// One response for the whole grid rather than a widget per cell: a drag has to be read as a
    /// *rectangle* between two cells, which per-cell widgets make awkward and this makes trivial.
    fn ui_picker(&mut self, ui: &mut Ui, set: &LauncherSet) {
        let grid = set.grid;
        let (cols, rows) = (grid.cols.max(1) as f32, grid.rows.max(1) as f32);
        // Square-ish cells, like the panel's ~149x146, capped so a 3x9 grid does not sprawl.
        let cell = ((ui.available_width() - 4.0) / cols).clamp(28.0, 84.0);
        let size = vec2(cell * cols, cell * rows);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());

        let cell_at = |pos: egui::Pos2| -> Option<(u32, u32)> {
            if !rect.contains(pos) {
                return None;
            }
            let c = ((pos.x - rect.left()) / cell).floor() as u32;
            let r = ((pos.y - rect.top()) / cell).floor() as u32;
            (r < grid.rows && c < grid.cols).then_some((r, c))
        };
        let pointer_cell = resp
            .interact_pointer_pos()
            .or_else(|| ui.ctx().pointer_latest_pos())
            .and_then(cell_at);

        // Read the drag as the rectangle spanned between the anchor cell and the pointer,
        // clamped to a legal span. A click is just a 1x1 drag.
        if resp.drag_started() {
            self.drag_anchor = pointer_cell;
        }
        let mut proposal = None;
        if let (Some(anchor), Some(here)) = (self.drag_anchor, pointer_cell) {
            proposal = Some(span_between(anchor, here));
        } else if resp.clicked() {
            if let Some((r, c)) = pointer_cell {
                proposal = Some((r, c, 1, 1));
            }
        }

        let skip = self.editing.as_deref();
        let legal = proposal.is_some_and(|(r, c, w, h)| {
            launcher::rect_is_free(grid, &set.buttons, skip, r, c, w, h)
        });

        // Commit only what is legal — this is where an overlap becomes unexpressible rather than
        // something downstream has to drop.
        if let Some((r, c, w, h)) = proposal {
            let done = resp.clicked() || resp.drag_stopped();
            if legal && done {
                self.form.row = r;
                self.form.col = c;
                self.form.w = w;
                self.form.h = h;
                self.status = Status::default();
            } else if !legal && done {
                self.status
                    .say(false, "that rectangle is taken or off the grid");
            }
        }
        if resp.drag_stopped() {
            self.drag_anchor = None;
        }

        // --- paint ---
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, PANEL_VOID);
        let seam = Stroke::new(1.0_f32, Color32::from_rgb(0x1b, 0x23, 0x34));
        for r in 0..grid.rows {
            for c in 0..grid.cols {
                painter.rect_stroke(cell_rect(rect, cell, r, c, 1, 1).shrink(0.5), 2.0, seam);
            }
        }
        // Every button except the one being edited — it is drawn as the selection instead.
        for b in &set.buttons {
            if skip.is_some_and(|k| k == b.key) {
                continue;
            }
            paint_button(
                ui,
                &painter,
                cell_rect(rect, cell, b.row, b.col, b.w, b.h),
                fill_for(&b.color),
                &b.label,
                seam,
            );
        }
        // The button being placed, in its committed spot.
        let sel = cell_rect(
            rect,
            cell,
            self.form.row,
            self.form.col,
            self.form.w,
            self.form.h,
        );
        paint_button(
            ui,
            &painter,
            sel,
            fill_for(&self.form.color),
            &self.form.label,
            Stroke::new(2.0_f32, PANEL_INK),
        );
        // The live drag, in accent or refusal.
        if let Some((r, c, w, h)) = proposal {
            if !(resp.clicked() || resp.drag_stopped()) {
                let color = if legal {
                    Color32::from_rgb(0x38, 0xbe, 0x84)
                } else {
                    Color32::from_rgb(0xe0, 0x56, 0x3f)
                };
                painter.rect_stroke(
                    cell_rect(rect, cell, r, c, w, h).shrink(1.0),
                    3.0,
                    Stroke::new(2.0_f32, color),
                );
            }
        }

        ui.add_space(2.0);
        ui.weak(format!(
            "row {} col {} · {}×{}",
            self.form.row, self.form.col, self.form.w, self.form.h
        ));
    }

    /// Save / Test / Delete. Returns `true` if the registry changed.
    fn ui_actions(&mut self, ui: &mut Ui, set: &LauncherSet) -> bool {
        let mut changed = false;
        let blocker = self.save_blocker(set);
        ui.horizontal(|ui| {
            let save = ui.add_enabled(blocker.is_none(), egui::Button::new("Save"));
            let save = match &blocker {
                Some(why) => save.on_disabled_hover_text(why.clone()),
                None => save.on_hover_text(
                    "Write it to the registry — the panel picks it up in about two seconds",
                ),
            };
            if save.clicked() {
                changed |= self.do_save();
            }

            let can_fire = !self.form.url.trim().is_empty();
            if ui
                .add_enabled(can_fire, egui::Button::new("Test"))
                .on_hover_text(
                    "Run this button now, exactly as the panel would — no save, no dashboard",
                )
                .clicked()
            {
                let b = self.form.to_button();
                let ok = launcher::fire(&b);
                self.status.say(
                    ok,
                    if ok {
                        "fired — check the window it landed in".to_string()
                    } else {
                        "could not open the URL (see stderr)".to_string()
                    },
                );
            }

            ui.add_space(12.0);
            let editing = self.editing.clone();
            if ui
                .add_enabled(editing.is_some(), egui::Button::new("Delete"))
                .on_hover_text("Remove this button's registry key")
                .clicked()
            {
                if let Some(key) = editing {
                    match launcher::delete(&key) {
                        Ok(()) => {
                            self.reset();
                            self.status.say(true, format!("deleted “{key}”"));
                            changed = true;
                        }
                        Err(e) => self.status.say(false, e),
                    }
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.open = false;
                }
            });
        });
        changed
    }

    /// Why Save is disabled, if it is. One place, so the button and its tooltip cannot disagree.
    fn save_blocker(&self, set: &LauncherSet) -> Option<String> {
        let key = self.form.key.trim();
        if let Some(e) = launcher::key_error(key, &set.buttons, self.editing.as_deref()) {
            return Some(format!("Key: {e}"));
        }
        if self.form.url.trim().is_empty() {
            return Some("Needs a URL".into());
        }
        if !launcher::rect_is_free(
            set.grid,
            &set.buttons,
            self.editing.as_deref(),
            self.form.row,
            self.form.col,
            self.form.w,
            self.form.h,
        ) {
            return Some("Placement overlaps another button".into());
        }
        None
    }

    /// Write the form, removing the old key first when a button was renamed — otherwise a rename
    /// would leave the original behind and the panel would show both.
    fn do_save(&mut self) -> bool {
        let b = self.form.to_button();
        if let Err(e) = launcher::save(&b) {
            self.status.say(false, e);
            return false;
        }
        let renamed_from = self
            .editing
            .as_deref()
            .filter(|old| !old.eq_ignore_ascii_case(&b.key))
            .map(str::to_string);
        if let Some(old) = &renamed_from {
            if let Err(e) = launcher::delete(old) {
                // The new button is already written and working; say so plainly rather than
                // reporting a failed save.
                self.status.say(
                    false,
                    format!("saved, but the old key '{old}' remains: {e}"),
                );
                self.editing = Some(b.key.clone());
                return true;
            }
        }
        self.status.say(
            true,
            match renamed_from {
                Some(old) => format!("saved “{}” (was “{old}”)", b.key),
                None => format!("saved “{}”", b.key),
            },
        );
        self.editing = Some(b.key.clone());
        true
    }
}

/// The rectangle two cells span, clamped to a legal size. Dragging past three cells stops at
/// three rather than refusing, so an over-long drag still lands somewhere sensible.
fn span_between(a: (u32, u32), b: (u32, u32)) -> (u32, u32, u32, u32) {
    let (r0, c0) = (a.0.min(b.0), a.1.min(b.1));
    let (r1, c1) = (a.0.max(b.0), a.1.max(b.1));
    let h = (r1 - r0 + 1).min(MAX_SPAN);
    let w = (c1 - c0 + 1).min(MAX_SPAN);
    (r0, c0, w, h)
}

/// The screen rectangle of a `w`x`h` button at (`row`, `col`).
fn cell_rect(grid: Rect, cell: f32, row: u32, col: u32, w: u32, h: u32) -> Rect {
    let min = pos2(
        grid.left() + col as f32 * cell,
        grid.top() + row as f32 * cell,
    );
    Rect::from_min_size(min, vec2(w as f32 * cell, h as f32 * cell))
}

/// One button on the picker, painted as the panel paints it: the fill, a wrapped centered label
/// in `MOON_INK`, rounded corners.
fn paint_button(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    fill: Color32,
    label: &str,
    stroke: Stroke,
) {
    let r = rect.shrink(2.0);
    painter.rect_filled(r, 5.0, fill);
    painter.rect_stroke(r, 5.0, stroke);
    if label.is_empty() {
        return;
    }
    let galley = ui.fonts(|f| {
        f.layout(
            label.to_string(),
            FontId::proportional(12.0),
            PANEL_INK,
            r.width() - 6.0,
        )
    });
    let pos = r.center() - galley.size() / 2.0;
    painter.galley(pos, galley, PANEL_INK);
}

/// A color swatch with the panel's own label color on it, so the choice previews its legibility.
fn swatch(ui: &mut Ui, fill: Color32, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 22.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, fill);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Ab",
            FontId::proportional(11.0),
            PANEL_INK,
        );
        let stroke = if selected {
            Stroke::new(2.0_f32, ui.visuals().strong_text_color())
        } else if resp.hovered() {
            Stroke::new(1.0_f32, ui.visuals().weak_text_color())
        } else {
            Stroke::new(1.0_f32, Color32::from_rgb(0x1b, 0x23, 0x34))
        };
        painter.rect_stroke(rect, 3.0, stroke);
    }
    resp
}

/// One row of the button list: a small live preview plus its key and placement.
fn button_list_row(ui: &mut Ui, b: &LauncherButton, selected: bool) -> egui::Response {
    let height = 26.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if selected || resp.hovered() {
            let fill = if selected {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            };
            painter.rect_filled(rect, 3.0, fill);
        }
        let chip = Rect::from_min_size(
            pos2(rect.left() + 3.0, rect.center().y - 8.0),
            vec2(22.0, 16.0),
        );
        painter.rect_filled(chip, 3.0, fill_for(&b.color));
        let text_color = ui.visuals().text_color();
        painter.text(
            pos2(chip.right() + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            &b.label,
            FontId::proportional(12.5),
            text_color,
        );
        painter.text(
            pos2(rect.right() - 4.0, rect.center().y),
            Align2::RIGHT_CENTER,
            format!("{},{}", b.row, b.col),
            FontId::proportional(10.0),
            ui.visuals().weak_text_color(),
        );
    }
    resp.on_hover_text(format!("{}\n{}", b.key, b.url))
}

/// A small inline caution — used for everything the panel would quietly do to a value.
fn warn(ui: &mut Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(ui.visuals().warn_fg_color),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::Grid;

    fn sample() -> LauncherButton {
        LauncherButton {
            key: "ado-pipelines".into(),
            label: "🛠️ Pipelines".into(),
            url: "https://example.com/pipes".into(),
            target: Target::Named("Pipes".into()),
            color: "EDGE_TEAL".into(),
            row: 0,
            col: 0,
            w: 2,
            h: 1,
        }
    }

    fn sample_window(label: &str) -> EdgeWindow {
        EdgeWindow {
            hwnd: 1,
            label: label.into(),
            named: true,
            tab_count: None,
            z_index: 0,
        }
    }

    #[test]
    fn hex_colors_parse_with_or_without_a_hash() {
        let teal = Color32::from_rgb(0x2e, 0xc4, 0xc4);
        assert_eq!(resolve_color("#2ec4c4"), Some(teal));
        assert_eq!(resolve_color("2EC4C4"), Some(teal));
    }

    #[test]
    fn palette_names_resolve_case_insensitively_like_the_panel() {
        let mint = Color32::from_rgb(0x38, 0xbe, 0x84);
        assert_eq!(resolve_color("INSIDER_MINT"), Some(mint));
        assert_eq!(resolve_color("insider_mint"), Some(mint));
    }

    #[test]
    fn an_unknown_color_falls_back_the_way_the_panel_does() {
        assert_eq!(resolve_color("chartreuse"), None);
        assert_eq!(resolve_color(""), None);
        assert_eq!(fill_for("chartreuse"), PANEL_DEFAULT_FILL);
    }

    #[test]
    fn no_offered_color_is_a_text_color() {
        // The panel always draws labels in MOON_INK; offering a near-white fill would hand Ken
        // an unreadable button. Guards the copied table against a careless paste.
        for (name, rgb, _) in BUTTON_COLORS {
            let c = from_rgb24(*rgb);
            let lum = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
            assert!(lum < 200.0, "{name} is too light to put MOON_INK on");
        }
    }

    #[test]
    fn every_offered_color_resolves() {
        for (name, _, _) in BUTTON_COLORS {
            assert!(resolve_color(name).is_some(), "{name} does not resolve");
            assert!(
                name.len() <= COLOR_MAX_BYTES,
                "{name} does not fit the panel's color field"
            );
        }
    }

    #[test]
    fn a_drag_spans_the_rectangle_between_two_cells() {
        assert_eq!(span_between((0, 0), (0, 0)), (0, 0, 1, 1));
        assert_eq!(span_between((0, 0), (1, 2)), (0, 0, 3, 2));
        // Dragged backwards — the anchor is a corner, not the origin.
        assert_eq!(span_between((2, 5), (1, 3)), (1, 3, 3, 2));
    }

    #[test]
    fn a_drag_past_the_span_limit_stops_at_it() {
        let (_, _, w, h) = span_between((0, 0), (5, 8));
        assert_eq!((w, h), (MAX_SPAN, MAX_SPAN));
    }

    #[test]
    fn the_form_round_trips_a_button() {
        let b = sample();
        let back = Form::from_button(&b).to_button();
        assert_eq!(back.key, b.key);
        assert_eq!(back.label, b.label);
        assert_eq!(back.target, b.target);
        assert_eq!((back.row, back.col, back.w, back.h), (0, 0, 2, 1));
    }

    /// Draw the whole editor headlessly, twice, and fail on a panic.
    ///
    /// A bare `egui::Context` has no renderer registered, so `show_viewport_immediate` embeds the
    /// viewport and runs the closure inline — meaning this exercises the *real* path: the form,
    /// the swatch row, the button list, and every painter call in the picker. Worth having,
    /// because this is a window Ken opens occasionally and a panic in it would take the rail down
    /// with it. Two frames rather than one so the second sees a populated font atlas.
    #[test]
    fn the_editor_draws_without_panicking() {
        let ctx = egui::Context::default();
        let set = LauncherSet {
            grid: Grid::default(),
            buttons: vec![sample()],
        };
        let edge = vec![sample_window("Pipes"), sample_window("GitHub")];

        let mut ed = Editor::default();
        ed.open_new();
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                ed.show(ctx, &set, &edge);
            });
        }
        // …and again with an existing button loaded, which takes the other branch through the
        // heading, the picker's skip-self path, and an enabled Delete.
        ed.load(&sample());
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            ed.show(ctx, &set, &edge);
        });
        assert_eq!(ed.editing.as_deref(), Some("ado-pipelines"));
    }

    /// A closed editor must not draw at all — `show` is called every frame from `update`.
    #[test]
    fn a_closed_editor_draws_nothing_and_reports_no_change() {
        let ctx = egui::Context::default();
        let mut ed = Editor::default();
        let set = LauncherSet::default();
        let mut changed = true;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            changed = ed.show(ctx, &set, &[]);
        });
        assert!(!ed.open);
        assert!(!changed, "a closed editor cannot have written anything");
    }

    #[test]
    fn an_empty_named_target_falls_back_to_current() {
        // "A named window" selected but none picked yet — must not write `target=named` with no
        // name, which `load` would then have to warn about on every one-second reload.
        let f = Form {
            target_named: true,
            ..Form::default()
        };
        assert_eq!(f.to_button().target, Target::Current);
    }
}
