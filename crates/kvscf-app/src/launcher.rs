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
/// slice-4 editor will offer.
pub const MAX_SPAN: u32 = 3;

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
    /// Only ever rendered by the dashboard, so nothing reads it in the `kvscf-local` build.
    #[cfg_attr(not(feature = "remote"), allow(dead_code))]
    pub label: String,
    pub url: String,
    pub target: Target,
    /// Background color, as authored (hex or a palette name the panel resolves). Published
    /// only — same as `label`.
    #[cfg_attr(not(feature = "remote"), allow(dead_code))]
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
#[allow(dead_code)] // only called from the `remote` build
pub fn activate(key: &str) -> bool {
    let set = scan();
    let Some(b) = set.buttons.into_iter().find(|b| b.key == key) else {
        eprintln!("kvscf: no launcher button '{key}' configured");
        return false;
    };
    let windows: Vec<EdgeWindow> = edge_windows();
    let hwnd = kvscf_core::pick_edge_target(&windows, preferred_name(&b.target));
    kvscf_core::open_url_in_window(hwnd, &b.url)
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
}

#[cfg(not(windows))]
mod config {
    use super::{Grid, LauncherButton};
    pub fn load() -> (Grid, Vec<LauncherButton>) {
        (Grid::default(), Vec::new())
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
}
