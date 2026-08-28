//! The favorite editor (sprint 020, korg kvscf #1683) — make a favorite's stored identity
//! visible, and fixable.
//!
//! A favorite is a *resolved* thing: starring a window stores the folder URI kvscf worked out
//! from VS Code's `workspaceStorage`, not anything Ken typed. When that resolution was wrong
//! (#1682: two folders sharing a leaf name), the entry was also unfixable from inside the app —
//! unfavoriting and re-favoriting just re-ran the same resolution and stored the same wrong URI.
//! The only recourse was hand-editing `%APPDATA%\kvscf\favorites.json`.
//!
//! So this window is a debugging surface first and an editor second. The **decoded** URI is shown
//! next to the stored one because that is the field you actually have to read to tell
//! `ssh-remote%2Bkubs0/ai/klams` from `ssh-remote%2Bkubs0/home/ken/src/ai/klams`, and the derived
//! workspace/host are shown live because those are what matching and relaunch really use.
//!
//! **Its own window, like the Launcher editor**, and for the same reason: a 280 px rail — often
//! docked, borderless and always-on-top — cannot hold a form with a full URI in it.
//!
//! It does not create favorites. Starring a window is how one comes into being; this fixes,
//! renames and removes what is already there.

use eframe::egui::{self, TextEdit, Ui};

use kvscf_core::App;

use crate::winset::{self, SetEntry};

/// The builds a favorite can name. `App::Unknown` is deliberately absent — it is what an
/// unreadable key decays to on load, never something to choose (it relaunches as plain `code`).
const BUILDS: [App; 3] = [App::Stable, App::Insiders, App::Exploration];

/// The editor's own state; lives in [`crate::KvscfApp`] so it survives across frames.
#[derive(Default)]
pub struct FavEditor {
    pub open: bool,
    /// The favorite being edited, as it was when loaded — the identity a Save replaces. Kept
    /// separate from the form so retargeting a favorite is expressible rather than becoming a
    /// second entry.
    editing: Option<SetEntry>,
    form: Form,
    status: Status,
}

/// One favorite's fields, as typed.
#[derive(Default)]
struct Form {
    app: Option<App>,
    uri: String,
    label: String,
}

impl Form {
    fn from_entry(e: &SetEntry) -> Self {
        Form {
            app: Some(e.app),
            uri: e.uri.clone(),
            label: e.label.clone(),
        }
    }

    /// The entry this form would save. `workspace`/`host` are re-derived from the URI exactly the
    /// way loading `favorites.json` derives them, so an edited entry and a reloaded one agree.
    fn to_entry(&self) -> SetEntry {
        let uri = self.uri.trim().to_string();
        let label = self.label.trim().to_string();
        let (workspace, host) = winset::parse_uri(&uri).unwrap_or((label.clone(), None));
        SetEntry {
            app: self.app.unwrap_or(App::Stable),
            uri,
            label,
            workspace,
            host,
        }
    }
}

/// A one-line result of the last Save / Delete, and whether it went well.
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

impl FavEditor {
    /// Open with nothing selected — the Controls drawer's entry point.
    pub fn open_list(&mut self) {
        self.open = true;
        self.clear();
    }

    /// Open on one favorite — the row context menu's entry point, which is the path that matters:
    /// it answers "what did starring *this* actually store?".
    pub fn open_on(&mut self, entry: &SetEntry) {
        self.open = true;
        self.load(entry);
    }

    fn clear(&mut self) {
        self.editing = None;
        self.form = Form::default();
        self.status = Status::default();
    }

    fn load(&mut self, entry: &SetEntry) {
        self.form = Form::from_entry(entry);
        self.editing = Some(entry.clone());
        self.status = Status::default();
    }

    /// Draw the editor window. Returns `true` when `favorites` changed, so the caller can persist
    /// and republish rather than leaving the file and the panel behind the list.
    pub fn show(&mut self, ctx: &egui::Context, favorites: &mut Vec<SetEntry>) -> bool {
        if !self.open {
            return false;
        }
        let viewport = egui::ViewportBuilder::default()
            .with_title("kvscf — Favorite editor")
            .with_inner_size([760.0, 460.0])
            .with_min_inner_size([560.0, 360.0]);

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("favorite_editor"),
            viewport,
            |ctx, _class| {
                let mut changed = false;
                egui::SidePanel::left("favorites")
                    .exact_width(220.0)
                    .show(ctx, |ui| self.ui_list(ui, favorites));
                egui::CentralPanel::default().show(ctx, |ui| {
                    changed = self.ui_form(ui, favorites);
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    self.open = false;
                }
                changed
            },
        )
    }

    /// The favorites, as a list you pick from. Labels can collide — that is the whole bug — so
    /// each row's tooltip carries the decoded URI that tells two of them apart.
    fn ui_list(&mut self, ui: &mut Ui, favorites: &[SetEntry]) {
        ui.add_space(6.0);
        ui.heading("Favorites");
        ui.add_space(4.0);
        if favorites.is_empty() {
            ui.weak("None yet — right-click a window and “Mark as favorite”.");
            return;
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for fav in favorites {
                    let selected = self
                        .editing
                        .as_ref()
                        .is_some_and(|e| e.same_target(fav) && e.label == fav.label);
                    let text = format!("{}  ·  {}", fav.label, fav.app.label());
                    if ui
                        .selectable_label(selected, text)
                        .on_hover_text(winset::percent_decode(&fav.uri))
                        .clicked()
                    {
                        self.load(fav);
                    }
                }
            });
    }

    /// Fields, the derived readout, then the actions. Returns `true` if `favorites` changed.
    fn ui_form(&mut self, ui: &mut Ui, favorites: &mut Vec<SetEntry>) -> bool {
        ui.add_space(4.0);
        let Some(original) = self.editing.clone() else {
            ui.add_space(8.0);
            ui.weak("Pick a favorite on the left to see what it stores.");
            return false;
        };
        ui.heading(format!("Editing “{}”", original.label));
        ui.add_space(6.0);

        egui::Grid::new("fav_fields")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label("Label");
                ui.add(
                    TextEdit::singleline(&mut self.form.label)
                        .desired_width(f32::INFINITY)
                        .hint_text("klams (kubs0)"),
                );
                ui.end_row();

                ui.label("Build");
                ui.horizontal(|ui| {
                    for build in BUILDS {
                        let selected = self.form.app == Some(build);
                        if ui.selectable_label(selected, build.label()).clicked() {
                            self.form.app = Some(build);
                        }
                    }
                });
                ui.end_row();

                ui.label("Folder URI");
                ui.add(
                    TextEdit::singleline(&mut self.form.uri)
                        .desired_width(f32::INFINITY)
                        .hint_text("vscode-remote://ssh-remote%2Bkubs0/home/ken/src/ai/klams"),
                );
                ui.end_row();
            });

        ui.add_space(6.0);
        self.ui_derived(ui);

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        let changed = self.ui_actions(ui, favorites, &original);

        if !self.status.text.is_empty() {
            ui.add_space(6.0);
            let color = if self.status.ok {
                ui.visuals().weak_text_color()
            } else {
                ui.visuals().error_fg_color
            };
            ui.label(egui::RichText::new(&self.status.text).small().color(color));
        }
        changed
    }

    /// What the stored URI actually *means* — the readout this window exists for. The label is
    /// cosmetic; these three are what matching, the dimmed list and relaunch all run on.
    fn ui_derived(&self, ui: &mut Ui) {
        let uri = self.form.uri.trim();
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("fav_derived")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.weak("Decoded");
                    if uri.is_empty() {
                        ui.weak("—");
                    } else {
                        ui.label(
                            egui::RichText::new(winset::percent_decode(uri))
                                .small()
                                .monospace(),
                        );
                    }
                    ui.end_row();

                    match winset::parse_uri(uri) {
                        Some((workspace, host)) => {
                            ui.weak("Workspace");
                            ui.label(egui::RichText::new(workspace).small());
                            ui.end_row();
                            ui.weak("Host");
                            ui.label(
                                egui::RichText::new(host.unwrap_or_else(|| "local".into())).small(),
                            );
                            ui.end_row();
                        }
                        None => {
                            ui.weak("Workspace");
                            // Not fatal: relaunch passes the URI to `code --folder-uri` verbatim,
                            // so an unparsed one can still work. It just cannot be matched against
                            // an open window, so the favorite would stay dimmed while it is open.
                            ui.label(
                                egui::RichText::new(
                                    "can't be derived — this favorite won't match an open window",
                                )
                                .small()
                                .color(ui.visuals().warn_fg_color),
                            );
                            ui.end_row();
                        }
                    }
                });
        });
    }

    /// Save / Delete / Close. Returns `true` if `favorites` changed.
    fn ui_actions(
        &mut self,
        ui: &mut Ui,
        favorites: &mut Vec<SetEntry>,
        original: &SetEntry,
    ) -> bool {
        let mut changed = false;
        let candidate = self.form.to_entry();
        let blocker = save_blocker(&candidate, favorites, original);
        ui.horizontal(|ui| {
            let save = ui.add_enabled(blocker.is_none(), egui::Button::new("Save"));
            let save = match &blocker {
                Some(why) => save.on_disabled_hover_text(why.clone()),
                None => save.on_hover_text("Rewrite this favorite in favorites.json"),
            };
            if save.clicked() {
                apply_save(favorites, original, candidate.clone());
                self.load(&candidate);
                self.status
                    .say(true, format!("saved “{}”", candidate.label));
                changed = true;
            }

            if ui
                .button("Revert")
                .on_hover_text("Discard edits and reload the stored values")
                .clicked()
            {
                let stored = original.clone();
                self.load(&stored);
            }

            ui.add_space(12.0);
            if ui
                .button("Delete")
                .on_hover_text("Remove this favorite")
                .clicked()
            {
                favorites.retain(|f| !f.same_target(original));
                self.clear();
                self.status
                    .say(true, format!("deleted “{}”", original.label));
                changed = true;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.open = false;
                }
            });
        });
        changed
    }
}

/// Write `candidate` over the favorite `original` identifies.
///
/// Replaced **in place**, so a corrected favorite keeps its position in the rail instead of
/// jumping to the bottom — the repair should be invisible in the list, since to Ken it is the
/// same favorite as before, now pointing where he meant.
fn apply_save(favorites: &mut Vec<SetEntry>, original: &SetEntry, candidate: SetEntry) {
    match favorites.iter().position(|f| f.same_target(original)) {
        Some(i) => favorites[i] = candidate,
        // The list moved under us — a window starred from the rail in the same frame, or the
        // entry deleted from this very window. Keep the edit rather than dropping it silently.
        None => favorites.push(candidate),
    }
}

/// Why Save is disabled, if it is. One place, so the button and its tooltip cannot disagree.
fn save_blocker(
    candidate: &SetEntry,
    favorites: &[SetEntry],
    original: &SetEntry,
) -> Option<String> {
    if candidate.uri.is_empty() {
        return Some("Needs a folder URI".into());
    }
    if candidate.label.is_empty() {
        return Some("Needs a label".into());
    }
    // Retargeting onto a folder another favorite already owns would make two entries that match
    // the same window — one of them permanently dimmed, and neither obviously wrong.
    let collides = favorites
        .iter()
        .any(|f| f.same_target(candidate) && !f.same_target(original));
    collides.then(|| "Another favorite already points at that folder".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(uri: &str, label: &str) -> SetEntry {
        let (workspace, host) = winset::parse_uri(uri).unwrap_or((label.to_string(), None));
        SetEntry {
            app: App::Insiders,
            uri: uri.into(),
            label: label.into(),
            workspace,
            host,
        }
    }

    fn poisoned() -> SetEntry {
        entry(
            "vscode-remote://ssh-remote%2Bkubs0/ai/klams",
            "klams (kubs0)",
        )
    }

    #[test]
    fn the_form_round_trips_an_entry() {
        let e = poisoned();
        let back = Form::from_entry(&e).to_entry();
        assert_eq!(back.uri, e.uri);
        assert_eq!(back.label, e.label);
        assert_eq!(back.app, e.app);
        assert_eq!(back.workspace, "klams");
        assert_eq!(back.host.as_deref(), Some("kubs0"));
    }

    #[test]
    fn retargeting_re_derives_workspace_and_host() {
        // The repair this window exists for: correct the URI, and the parts kvscf matches on
        // must follow it rather than keeping the old folder's.
        let mut form = Form::from_entry(&poisoned());
        form.uri = "vscode-remote://ssh-remote%2Bkai/home/ken/src/tools/korg".into();
        let fixed = form.to_entry();
        assert_eq!(fixed.workspace, "korg");
        assert_eq!(fixed.host.as_deref(), Some("kai"));
    }

    #[test]
    fn an_unparseable_uri_still_saves_falling_back_to_the_label() {
        // read_entries does the same on load; an escape hatch that refuses odd input is not one.
        let form = Form {
            app: Some(App::Stable),
            uri: "some-scheme://whatever".into(),
            label: "hand-rolled".into(),
        };
        let e = form.to_entry();
        assert_eq!(e.workspace, "hand-rolled");
        assert_eq!(e.host, None);
    }

    #[test]
    fn save_is_blocked_on_empty_fields() {
        let orig = poisoned();
        let stored = std::slice::from_ref(&orig);

        let mut form = Form::from_entry(&orig);
        form.uri = "   ".into();
        assert!(save_blocker(&form.to_entry(), stored, &orig).is_some());

        let mut form = Form::from_entry(&orig);
        form.label = String::new();
        assert!(save_blocker(&form.to_entry(), stored, &orig).is_some());
    }

    #[test]
    fn save_is_blocked_when_retargeting_onto_another_favorite() {
        let orig = poisoned();
        let other = entry(
            "vscode-remote://ssh-remote%2Bkai/home/ken/src/tools/korg",
            "korg (kai)",
        );
        let favorites = vec![orig.clone(), other.clone()];

        // Onto korg's folder — taken.
        let mut form = Form::from_entry(&orig);
        form.uri = other.uri.clone();
        assert!(save_blocker(&form.to_entry(), &favorites, &orig).is_some());

        // Onto the folder it *should* have had all along — free.
        let mut form = Form::from_entry(&orig);
        form.uri = "vscode-remote://ssh-remote%2Bkubs0/home/ken/src/ai/klams".into();
        assert!(save_blocker(&form.to_entry(), &favorites, &orig).is_none());

        // Editing only the label of an entry that is already in the list is not a collision
        // with itself.
        let mut form = Form::from_entry(&orig);
        form.label = "klams (kubs0) — old".into();
        assert!(save_blocker(&form.to_entry(), &favorites, &orig).is_none());
    }

    #[test]
    fn saving_a_repair_replaces_the_entry_in_place() {
        // The end of the #1682 story: the poisoned klams favorite, retargeted at the repo it
        // should always have pointed at. It must overwrite that favorite — not become a second
        // one — and must not move in the list.
        let orig = poisoned();
        let mut favorites = vec![
            entry("file:///d%3A/ClaudeWorks/kvscf", "kvscf"),
            orig.clone(),
            entry(
                "vscode-remote://ssh-remote%2Bkai/home/ken/src/tools/korg",
                "korg (kai)",
            ),
        ];

        let mut form = Form::from_entry(&orig);
        form.uri = "vscode-remote://ssh-remote%2Bkubs0/home/ken/src/ai/klams".into();
        apply_save(&mut favorites, &orig, form.to_entry());

        assert_eq!(favorites.len(), 3, "a repair must not add an entry");
        assert_eq!(
            favorites[1].uri, "vscode-remote://ssh-remote%2Bkubs0/home/ken/src/ai/klams",
            "the repair belongs where the old entry was"
        );
        assert_eq!(favorites[1].workspace, "klams");
        // Exact-match, not a suffix: the *correct* URI also ends `/ai/klams`, which is precisely
        // how confusable these two are.
        assert!(
            !favorites.iter().any(|f| f.same_target(&orig)),
            "the poisoned URI must be gone, not merely shadowed"
        );
    }

    #[test]
    fn saving_keeps_the_edit_when_the_entry_vanished_underneath() {
        // Starred from the rail, or deleted here, between load and Save. Dropping the edit
        // silently would be the worse failure — this is an escape hatch.
        let orig = poisoned();
        let mut favorites = vec![entry("file:///d%3A/ClaudeWorks/kvscf", "kvscf")];
        apply_save(&mut favorites, &orig, Form::from_entry(&orig).to_entry());
        assert_eq!(favorites.len(), 2);
        assert!(favorites.iter().any(|f| f.same_target(&orig)));
    }

    /// Draw the whole editor headlessly and fail on a panic — this window opens over whatever Ken
    /// is doing, and a panic in it would take the rail down with it. Same shape as the Launcher
    /// editor's guard: a bare `Context` has no renderer, so the viewport closure runs inline and
    /// this exercises the real form, list and derived readout.
    #[test]
    fn the_editor_draws_without_panicking() {
        let ctx = egui::Context::default();
        let mut favorites = vec![poisoned(), entry("file:///d%3A/ClaudeWorks/kvscf", "kvscf")];

        let mut ed = FavEditor::default();
        ed.open_list();
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                ed.show(ctx, &mut favorites);
            });
        }
        // …and again with a favorite loaded, which takes the other branch through the heading,
        // the derived readout and an enabled Delete.
        ed.open_on(&poisoned());
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            ed.show(ctx, &mut favorites);
        });
        assert_eq!(
            ed.editing.as_ref().map(|e| e.label.clone()).as_deref(),
            Some("klams (kubs0)")
        );

        // An unparseable URI draws the warning branch rather than panicking on the missing parts.
        ed.form.uri = "nonsense".into();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            ed.show(ctx, &mut favorites);
        });
    }

    #[test]
    fn a_closed_editor_draws_nothing_and_reports_no_change() {
        let ctx = egui::Context::default();
        let mut ed = FavEditor::default();
        let mut favorites = vec![poisoned()];
        let mut changed = true;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            changed = ed.show(ctx, &mut favorites);
        });
        assert!(!ed.open);
        assert!(!changed, "a closed editor cannot have changed anything");
    }
}
