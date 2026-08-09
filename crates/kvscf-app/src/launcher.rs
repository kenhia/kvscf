//! Launcher buttons (sprint 016, korg kvscf #1133) — the Stream Deck replacement.
//!
//! A button is "open this URL, in *this* Edge window". Configured in the registry under
//! `HKCU\Software\kenhia\kvscf\launcher\<key>` (one subkey per button, same shape and
//! `UserRoot` handling as `apps\<key>`), published to kdeskdash as `kvscf:launcher:<host>`,
//! and fired by a `{token, button:<key>}` command on the existing focus channel.
//!
//! **The command carries the key, never the URL** — same as the Apps tab's `{app:<key>}`. The
//! dashboard renders labels and colors; it never learns where a button goes. That matters most
//! on kwork, whose URLs are an employer's business and stay on the employer's machine.
//!
//! Presentation (label, color, grid placement) lives here rather than on the dashboard side.
//! Slightly odd for a window-focuser to own layout, but splitting the action from its
//! appearance across two repos would be worse, and it makes the kdeskdash mode entirely
//! feed-driven — it renders whatever grid it is handed.

use kvscf_core::EdgeWindow;

/// Panel default: 3 rows x 9 columns = 27 cells on the 1920x440 panel, ~21.6mm each — larger
/// than a Stream Deck key. Overridable per host via the `rows`/`cols` values on the parent
/// `launcher` key, so a differently-shaped panel needs no code change.
pub const DEFAULT_ROWS: u32 = 3;
pub const DEFAULT_COLS: u32 = 9;

/// Largest span in either axis. Keeps a button from swallowing the grid, and matches what the
/// editor offers.
pub const MAX_SPAN: u32 = 3;

/// Field ceilings the **panel** enforces (`KV_BTNKEY_MAX` / `KV_BTNLABEL_MAX` / `KV_COLOR_MAX`
/// in kdeskdash's `src/kvscf_feed.h`), in bytes, less the NUL its C buffers need.
///
/// The asymmetry is deliberate on that side and matters here: an over-long **key is rejected**
/// by the parser — a clipped key would press the wrong button — so the editor must refuse to
/// write one, or the button silently never appears on the panel. An over-long **label** is
/// merely truncated on screen, so that one is a warning, not a block.
pub const KEY_MAX_BYTES: usize = 47;
pub const LABEL_MAX_BYTES: usize = 47;
pub const COLOR_MAX_BYTES: usize = 23;

/// Which Edge window a button prefers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// No preference — use the top-Z Edge window ("Use current window" in the editor).
    Current,
    /// A named Edge window, by its exact name.
    Named(String),
}

/// One configured button.
#[derive(Debug, Clone)]
pub struct LauncherButton {
    /// Registry subkey — the stable id the `{button:<key>}` command echoes back.
    pub key: String,
    /// Rendered by the dashboard, never here — but the editor round-trips it, so unlike in
    /// sprint 016 this is live in the `kvscf-local` build too.
    pub label: String,
    pub url: String,
    pub target: Target,
    /// Background color, as authored: `#rrggbb`, or a name from kdeskdash's palette that the
    /// panel resolves. Empty means "use the panel default".
    pub color: String,
    pub row: u32,
    pub col: u32,
    pub w: u32,
    pub h: u32,
}

impl LauncherButton {
    /// Does this button's rectangle overlap `other`'s?
    fn overlaps(&self, other: &LauncherButton) -> bool {
        self.col < other.col + other.w
            && other.col < self.col + self.w
            && self.row < other.row + other.h
            && other.row < self.row + self.h
    }
}

/// The grid the buttons are placed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub rows: u32,
    pub cols: u32,
}

impl Default for Grid {
    fn default() -> Self {
        Grid {
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
        }
    }
}

/// A validated set: the grid plus the buttons that actually fit on it.
#[derive(Debug, Clone, Default)]
pub struct LauncherSet {
    pub grid: Grid,
    pub buttons: Vec<LauncherButton>,
}

/// Drop buttons that cannot be drawn, keeping the rest.
///
/// The slice-4 editor will make bad placements impossible to express, but until it exists Ken
/// edits the registry by hand — which is exactly when overlaps and typos happen. Rejected
/// buttons warn on stderr and are skipped; a bad button never takes the whole panel down, the
/// same discipline `apps` uses for an unusable launch spec.
///
/// Order matters: earlier buttons win a contested cell, so a newly-typed overlapping button
/// disappears rather than silently displacing one that was already working.
pub fn validate_layout(grid: Grid, buttons: Vec<LauncherButton>) -> Vec<LauncherButton> {
    let mut kept: Vec<LauncherButton> = Vec::new();
    for b in buttons {
        if b.w == 0 || b.h == 0 || b.w > MAX_SPAN || b.h > MAX_SPAN {
            eprintln!(
                "kvscf: launcher button '{}' has an unusable size {}x{} (max {MAX_SPAN}) — skipping",
                b.key, b.w, b.h
            );
            continue;
        }
        if b.col + b.w > grid.cols || b.row + b.h > grid.rows {
            eprintln!(
                "kvscf: launcher button '{}' at ({},{}) size {}x{} does not fit the {}x{} grid — skipping",
                b.key, b.row, b.col, b.w, b.h, grid.rows, grid.cols
            );
            continue;
        }
        if let Some(other) = kept.iter().find(|k| k.overlaps(&b)) {
            eprintln!(
                "kvscf: launcher button '{}' overlaps '{}' — skipping",
                b.key, other.key
            );
            continue;
        }
        kept.push(b);
    }
    kept
}

/// The button occupying a cell, if any. `skip` is the key currently being edited, whose own
/// cells read as free — you are moving it, not colliding with it.
pub fn occupant<'a>(
    buttons: &'a [LauncherButton],
    skip: Option<&str>,
    row: u32,
    col: u32,
) -> Option<&'a LauncherButton> {
    buttons.iter().find(|b| {
        !skip.is_some_and(|k| k.eq_ignore_ascii_case(&b.key))
            && row >= b.row
            && row < b.row + b.h
            && col >= b.col
            && col < b.col + b.w
    })
}

/// Can a `w`x`h` rectangle be placed at (`row`, `col`)?
///
/// This is what makes an overlap **impossible to express** rather than reported: the picker
/// refuses to commit a rectangle this rejects, so `validate_layout` never has anything to drop
/// from a button the editor wrote. It stays the backstop for hand-edited registry entries.
pub fn rect_is_free(
    grid: Grid,
    buttons: &[LauncherButton],
    skip: Option<&str>,
    row: u32,
    col: u32,
    w: u32,
    h: u32,
) -> bool {
    if w == 0 || h == 0 || w > MAX_SPAN || h > MAX_SPAN {
        return false;
    }
    if col + w > grid.cols || row + h > grid.rows {
        return false;
    }
    (row..row + h).all(|r| (col..col + w).all(|c| occupant(buttons, skip, r, c).is_none()))
}

/// Derive a registry-safe key from a label: `"🛠️ Pipes"` → `"pipes"`.
///
/// Emoji and wide Unicode are dropped rather than escaped, which is the whole reason the key is
/// separate from the label — Ken's labels carry both, and this string becomes a registry subkey
/// *and* travels through a C `char[48]` on the panel.
///
/// Returns `""` for a label with no ASCII alphanumerics at all (an emoji-only label is a real
/// possibility); the editor asks for a key by hand in that case rather than inventing one.
pub fn slugify_key(label: &str) -> String {
    let mut out = String::new();
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    // Only ASCII survives the filter, so bytes and chars agree and this cannot split a
    // codepoint.
    out.truncate(KEY_MAX_BYTES);
    out.trim_end_matches('-').to_string()
}

/// Whether every character is legal in a button key. Deliberately narrow: this string is a
/// registry subkey name (a `\` would silently retarget a write or a delete), a JSON value, and a
/// C buffer on the panel.
fn key_chars_ok(key: &str) -> bool {
    key.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Why this key cannot be written, or `None` if it can. `editing` is the key being replaced, so
/// re-saving a button is not a clash with itself.
///
/// Clash detection is case-insensitive because registry subkey names are: `Pipes` and `pipes`
/// are one key, and letting the editor create the second would silently overwrite the first.
pub fn key_error(key: &str, buttons: &[LauncherButton], editing: Option<&str>) -> Option<String> {
    if key.is_empty() {
        return Some("needs a key".into());
    }
    if !key_chars_ok(key) {
        return Some("letters, digits, - and _ only".into());
    }
    if key.len() > KEY_MAX_BYTES {
        return Some(format!(
            "too long for the panel ({} of {KEY_MAX_BYTES} chars)",
            key.len()
        ));
    }
    let taken = buttons.iter().any(|b| {
        b.key.eq_ignore_ascii_case(key) && !editing.is_some_and(|e| e.eq_ignore_ascii_case(&b.key))
    });
    if taken {
        return Some("a button already uses that key".into());
    }
    None
}

/// The name a button prefers, for [`kvscf_core::pick_edge_target`].
pub fn preferred_name(target: &Target) -> Option<&str> {
    match target {
        Target::Named(n) => Some(n.as_str()),
        Target::Current => None,
    }
}

/// Load the configured buttons, validated against the configured grid. Reloaded each refresh
/// so an edit reaches the panel in about two seconds without restarting anything.
pub fn scan() -> LauncherSet {
    let (grid, buttons) = config::load();
    LauncherSet {
        grid,
        buttons: validate_layout(grid, buttons),
    }
}

/// Fire a button by key: pick its Edge window, foreground it, open the URL there.
///
/// Returns `false` if no button with that key is configured, or if Edge could not be spawned.
pub fn activate(key: &str) -> bool {
    let set = scan();
    let Some(b) = set.buttons.into_iter().find(|b| b.key == key) else {
        eprintln!("kvscf: no launcher button '{key}' configured");
        return false;
    };
    fire(&b)
}

/// Fire a button **as given**, without looking it up.
///
/// This is what the editor's Test button uses, so a placement or a target can be tried before it
/// is written to the registry — the whole verb, no save, no panel, no Redis round trip.
pub fn fire(b: &LauncherButton) -> bool {
    let windows: Vec<EdgeWindow> = edge_windows();
    let hwnd = kvscf_core::pick_edge_target(&windows, preferred_name(&b.target));
    kvscf_core::open_url_in_window(hwnd, &b.url)
}

/// Write one button to the registry, creating its subkey or overwriting it in place.
///
/// The 1s config reload in [`scan`] picks it up on the next pass, so an edit reaches the panel in
/// about two seconds with nothing restarted.
pub fn save(b: &LauncherButton) -> Result<(), String> {
    config::save(b)
}

/// Remove a button — **the whole subkey**, not its values.
///
/// Blanking the values would leave a key that `load` skips with a warning on every one-second
/// reload, and that a later button of the same name would inherit stale fields from.
pub fn delete(key: &str) -> Result<(), String> {
    if key.is_empty() || !key_chars_ok(key) {
        // `load` can hand back any subkey name the registry holds, including one written by
        // hand. Re-check here so a `\` can never turn a delete into a walk up the tree.
        return Err(format!("refusing to delete an unsafe key {key:?}"));
    }
    config::delete(key)
}

#[cfg(windows)]
fn edge_windows() -> Vec<EdgeWindow> {
    kvscf_core::scan_edge()
}

#[cfg(not(windows))]
fn edge_windows() -> Vec<EdgeWindow> {
    Vec::new()
}

#[cfg(windows)]
mod config {
    use super::{Grid, LauncherButton, Target, DEFAULT_COLS, DEFAULT_ROWS};
    use crate::userreg::UserRoot;

    const PATH: &str = r"Software\kenhia\kvscf\launcher";

    /// Read the grid (from the parent key's `rows`/`cols`) and every `…\launcher\<key>` subkey.
    /// Buttons missing the two fields a button cannot work without — a `url` and a placement —
    /// are skipped with a warning rather than published half-formed.
    pub fn load() -> (Grid, Vec<LauncherButton>) {
        // Resolve the real user hive fresh each call — see `userreg` for the .DEFAULT-binding bug.
        let Some(user) = UserRoot::open() else {
            return (Grid::default(), Vec::new()); // hive not resolvable yet — retry next reload
        };
        let Ok(root) = user.key().open_subkey(PATH) else {
            return (Grid::default(), Vec::new()); // no buttons configured yet
        };

        let grid = Grid {
            rows: root.get_value::<u32, _>("rows").unwrap_or(DEFAULT_ROWS),
            cols: root.get_value::<u32, _>("cols").unwrap_or(DEFAULT_COLS),
        };

        let mut out = Vec::new();
        for key in root.enum_keys().flatten() {
            let Ok(sub) = root.open_subkey(&key) else {
                continue;
            };
            let get = |name: &str| {
                sub.get_value::<String, _>(name)
                    .ok()
                    .filter(|v| !v.is_empty())
            };

            let Some(url) = get("url") else {
                eprintln!("kvscf: launcher button '{key}' has no url — skipping");
                continue;
            };
            // A named target is stored as the window's NAME, never its HWND — handles die with
            // the window, names survive it.
            let target = match get("target").as_deref() {
                Some("named") => match get("target_name") {
                    Some(n) => Target::Named(n),
                    None => {
                        eprintln!(
                            "kvscf: launcher button '{key}' is target=named with no target_name \
                             — treating as 'use current'"
                        );
                        Target::Current
                    }
                },
                _ => Target::Current,
            };

            out.push(LauncherButton {
                label: get("label").unwrap_or_else(|| key.clone()),
                url,
                target,
                color: get("color").unwrap_or_default(),
                row: sub.get_value::<u32, _>("row").unwrap_or(0),
                col: sub.get_value::<u32, _>("col").unwrap_or(0),
                w: sub.get_value::<u32, _>("w").unwrap_or(1),
                h: sub.get_value::<u32, _>("h").unwrap_or(1),
                key,
            });
        }
        // Stable, predictable order: top-left reading order. Also makes "earlier wins a
        // contested cell" mean something a human can predict.
        out.sort_by_key(|b| (b.row, b.col, b.key.clone()));
        (grid, out)
    }

    /// Create-or-overwrite `…\launcher\<key>`.
    ///
    /// Every field is written on every save, `target_name` included — a button switched from a
    /// named window back to "use current" must not keep a stale name in the registry, where the
    /// next reader would see `target=current` beside a name that means nothing.
    pub fn save(b: &LauncherButton) -> Result<(), String> {
        let user = UserRoot::open().ok_or("the current-user registry hive is not available")?;
        let (root, _) = user
            .key()
            .create_subkey(PATH)
            .map_err(|e| format!("cannot open {PATH}: {e}"))?;
        let (sub, _) = root
            .create_subkey(&b.key)
            .map_err(|e| format!("cannot create the '{}' key: {e}", b.key))?;

        let set_str = |name: &str, value: &str| {
            sub.set_value(name, &value.to_string())
                .map_err(|e| format!("cannot write {name}: {e}"))
        };
        let set_u32 = |name: &str, value: u32| {
            sub.set_value(name, &value)
                .map_err(|e| format!("cannot write {name}: {e}"))
        };

        set_str("label", &b.label)?;
        set_str("url", &b.url)?;
        set_str("color", &b.color)?;
        match &b.target {
            Target::Named(n) => {
                set_str("target", "named")?;
                set_str("target_name", n)?;
            }
            Target::Current => {
                set_str("target", "current")?;
                set_str("target_name", "")?;
            }
        }
        set_u32("row", b.row)?;
        set_u32("col", b.col)?;
        set_u32("w", b.w)?;
        set_u32("h", b.h)?;
        Ok(())
    }

    /// Delete `…\launcher\<key>`. A button subkey holds only values, so the non-recursive
    /// delete is the right one — it would fail rather than take a subtree with it.
    pub fn delete(key: &str) -> Result<(), String> {
        let user = UserRoot::open().ok_or("the current-user registry hive is not available")?;
        let root = user
            .key()
            .open_subkey_with_flags(PATH, winreg::enums::KEY_ALL_ACCESS)
            .map_err(|e| format!("cannot open {PATH}: {e}"))?;
        root.delete_subkey(key)
            .map_err(|e| format!("cannot delete the '{key}' key: {e}"))
    }
}

#[cfg(not(windows))]
mod config {
    use super::{Grid, LauncherButton};

    pub fn load() -> (Grid, Vec<LauncherButton>) {
        (Grid::default(), Vec::new())
    }

    pub fn save(_b: &LauncherButton) -> Result<(), String> {
        Err("the launcher registry is Windows-only".into())
    }

    pub fn delete(_key: &str) -> Result<(), String> {
        Err("the launcher registry is Windows-only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn btn(key: &str, row: u32, col: u32, w: u32, h: u32) -> LauncherButton {
        LauncherButton {
            key: key.into(),
            label: key.into(),
            url: "https://example.com".into(),
            target: Target::Current,
            color: String::new(),
            row,
            col,
            w,
            h,
        }
    }

    #[test]
    fn fitting_buttons_are_kept() {
        let g = Grid::default();
        let kept = validate_layout(g, vec![btn("a", 0, 0, 2, 1), btn("b", 0, 2, 1, 3)]);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn out_of_bounds_is_dropped() {
        let g = Grid::default(); // 3x9
        let kept = validate_layout(
            g,
            vec![
                btn("wide", 0, 8, 2, 1), // col 8 + w 2 = 10 > 9
                btn("tall", 1, 0, 1, 3), // row 1 + h 3 = 4 > 3
                btn("ok", 0, 0, 1, 1),
            ],
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].key, "ok");
    }

    #[test]
    fn oversized_and_zero_spans_are_dropped() {
        let g = Grid::default();
        let kept = validate_layout(g, vec![btn("zero", 0, 0, 0, 1), btn("huge", 0, 0, 4, 1)]);
        assert!(kept.is_empty());
    }

    #[test]
    fn overlap_drops_the_later_button_not_the_working_one() {
        let g = Grid::default();
        let kept = validate_layout(
            g,
            vec![
                btn("first", 0, 0, 2, 2),
                btn("clash", 1, 1, 1, 1), // inside first's rectangle
                btn("clear", 0, 2, 1, 1),
            ],
        );
        let keys: Vec<&str> = kept.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, vec!["first", "clear"]);
    }

    #[test]
    fn adjacent_buttons_do_not_count_as_overlapping() {
        let g = Grid::default();
        let kept = validate_layout(g, vec![btn("l", 0, 0, 1, 1), btn("r", 0, 1, 1, 1)]);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn preferred_name_maps_the_two_target_kinds() {
        assert_eq!(preferred_name(&Target::Current), None);
        assert_eq!(
            preferred_name(&Target::Named("GitHub".into())),
            Some("GitHub")
        );
    }

    // --- editor helpers (sprint 017) ---

    #[test]
    fn occupant_covers_a_buttons_whole_rectangle() {
        let bs = vec![btn("wide", 1, 2, 2, 2)];
        for (r, c) in [(1, 2), (1, 3), (2, 2), (2, 3)] {
            assert!(
                occupant(&bs, None, r, c).is_some(),
                "({r},{c}) should be taken"
            );
        }
        for (r, c) in [(0, 2), (1, 4), (3, 2), (1, 1)] {
            assert!(
                occupant(&bs, None, r, c).is_none(),
                "({r},{c}) should be free"
            );
        }
    }

    #[test]
    fn a_button_does_not_collide_with_itself_while_being_moved() {
        let bs = vec![btn("wide", 0, 0, 2, 1)];
        // Nudging it one column right overlaps its own old cells — which must not block it.
        assert!(rect_is_free(Grid::default(), &bs, Some("wide"), 0, 1, 2, 1));
        assert!(!rect_is_free(Grid::default(), &bs, None, 0, 1, 2, 1));
    }

    #[test]
    fn rect_is_free_rejects_what_validate_layout_would_have_dropped() {
        let g = Grid::default(); // 3x9
        let bs = vec![btn("a", 0, 0, 1, 1)];
        assert!(!rect_is_free(g, &bs, None, 0, 0, 1, 1), "occupied");
        assert!(
            !rect_is_free(g, &bs, None, 0, 8, 2, 1),
            "off the right edge"
        );
        assert!(!rect_is_free(g, &bs, None, 2, 0, 1, 2), "off the bottom");
        assert!(!rect_is_free(g, &bs, None, 0, 1, 4, 1), "over MAX_SPAN");
        assert!(!rect_is_free(g, &bs, None, 0, 1, 0, 1), "zero span");
        assert!(
            rect_is_free(g, &bs, None, 0, 1, 3, 3),
            "the biggest legal rect"
        );
    }

    #[test]
    fn slugify_drops_emoji_and_wide_unicode() {
        // The case the whole label/key split exists for.
        assert_eq!(slugify_key("🛠️ Pipes"), "pipes");
        assert_eq!(slugify_key("Work Items"), "work-items");
        assert_eq!(slugify_key("  ADO  //  Boards  "), "ado-boards");
        assert_eq!(slugify_key("kai — src"), "kai-src");
    }

    #[test]
    fn slugify_gives_up_rather_than_inventing_a_key() {
        assert_eq!(slugify_key("🦀"), "");
        assert_eq!(slugify_key(""), "");
    }

    #[test]
    fn slugify_never_exceeds_the_panels_key_buffer() {
        let key = slugify_key(&"long ".repeat(40));
        assert!(key.len() <= KEY_MAX_BYTES);
        assert!(
            !key.ends_with('-'),
            "a truncation must not leave a trailing dash"
        );
    }

    #[test]
    fn key_error_rejects_the_unwritable() {
        let bs = vec![btn("gh", 0, 0, 1, 1)];
        assert!(key_error("", &bs, None).is_some());
        assert!(
            key_error(r"sub\key", &bs, None).is_some(),
            "a path separator"
        );
        assert!(key_error("has space", &bs, None).is_some());
        assert!(key_error(&"x".repeat(KEY_MAX_BYTES + 1), &bs, None).is_some());
        assert!(key_error("ado-wits", &bs, None).is_none());
    }

    #[test]
    fn key_clashes_are_case_insensitive_like_the_registry_is() {
        let bs = vec![btn("gh", 0, 0, 1, 1)];
        // Writing "GH" would overwrite "gh" rather than adding a button.
        assert!(key_error("GH", &bs, None).is_some());
        // …but re-saving the button under its own key is not a clash.
        assert!(key_error("gh", &bs, Some("gh")).is_none());
        assert!(key_error("GH", &bs, Some("gh")).is_none());
    }

    #[test]
    fn delete_refuses_a_key_that_could_escape_its_own_subkey() {
        assert!(delete("").is_err());
        assert!(delete(r"..\..\apps").is_err());
    }

    /// The real thing: write a button to the real registry, read it back through `scan`, then
    /// remove it and confirm the subkey is gone.
    ///
    /// **Ignored by default** — it touches `HKCU\Software\kenhia\kvscf\launcher`, which the
    /// running app publishes to the panel, so it does not belong in `cargo test`. Run it during
    /// sprint verification with `cargo test -p kvscf-app -- --ignored --nocapture`. It places
    /// itself on a cell `rect_is_free` says is empty, so it can never displace a real button, and
    /// it removes itself even when an assertion fails.
    #[cfg(windows)]
    #[test]
    #[ignore = "writes to the real HKCU registry"]
    fn a_button_round_trips_through_the_registry() {
        const KEY: &str = "zz-selftest-017";
        let before = scan();
        let spot = (0..before.grid.rows)
            .flat_map(|r| (0..before.grid.cols).map(move |c| (r, c)))
            .find(|(r, c)| rect_is_free(before.grid, &before.buttons, None, *r, *c, 1, 1))
            .expect("the grid is full — free a cell and re-run");

        let want = LauncherButton {
            key: KEY.into(),
            label: "🛠️ Selftest".into(), // the emoji path, deliberately
            url: "https://example.com/selftest".into(),
            target: Target::Named("Pipes".into()),
            color: "EDGE_TEAL".into(),
            row: spot.0,
            col: spot.1,
            w: 1,
            h: 1,
        };
        save(&want).expect("save");

        let got = scan()
            .buttons
            .into_iter()
            .find(|b| b.key == KEY)
            .expect("the button did not come back from the registry");
        // Clean up before asserting, so a failure cannot leave a button on Ken's panel.
        let removed = delete(KEY);

        assert_eq!(got.label, want.label);
        assert_eq!(got.url, want.url);
        assert_eq!(got.target, want.target);
        assert_eq!(got.color, want.color);
        assert_eq!((got.row, got.col, got.w, got.h), (spot.0, spot.1, 1, 1));

        removed.expect("delete");
        assert!(
            !scan().buttons.iter().any(|b| b.key == KEY),
            "the subkey survived the delete"
        );
    }
}
