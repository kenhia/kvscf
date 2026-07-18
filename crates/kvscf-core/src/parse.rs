//! Pure, portable parsing of a VS Code window title into workspace / remote / active-file.
//!
//! Ground truth captured from live windows (see `sprints/001`): the title separator is
//! `" - "`, but the appName `"Visual Studio Code - Insiders"` itself contains `" - "`, so we
//! cut the appName suffix first (at the last `" - Visual Studio Code"`), then split the
//! remainder on the first `" - "` into `rootName` / `activeEditorShort`. The remote tag lives
//! inside `rootName` as a trailing `[SSH: host]` / `[WSL: distro]` / `[Dev Container: name]` /
//! `[Codespaces]` bracket. Dirty is intentionally not surfaced.

use crate::Remote;

/// The workspace/remote/active-file extracted from a window title. `app` is determined
/// separately from the process image, not the title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTitle {
    pub workspace: String,
    pub remote: Remote,
    pub active_file: Option<String>,
}

const APP_MARKER: &str = " - Visual Studio Code";

/// Parse a raw window title. Returns `None` for an empty/degenerate title.
pub fn parse_title(title: &str) -> Option<ParsedTitle> {
    // 1) drop stray ${...} tokens (config drift / the historical ${seperator} typo).
    let t = strip_var_tokens(title.trim());
    // 2) drop a leading dirty indicator (we do not expose dirty).
    let t = strip_leading_dirty(&t);
    // 3) cut the appName (and any trailing profile) at the last " - Visual Studio Code".
    let t = match t.rfind(APP_MARKER) {
        Some(idx) => &t[..idx],
        None => t,
    };
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    // 4) split rootName vs activeEditorShort on the FIRST " - ".
    let (root, active) = match t.find(" - ") {
        Some(i) => (t[..i].trim(), Some(t[i + 3..].trim())),
        None => (t, None),
    };
    let (workspace, remote) = parse_root(root);
    if workspace.is_empty() {
        return None;
    }
    let active_file = active.filter(|s| !s.is_empty()).map(|s| s.to_string());
    Some(ParsedTitle {
        workspace,
        remote,
        active_file,
    })
}

/// Remove every `${...}` substring.
fn strip_var_tokens(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find('}') {
            Some(end) => rest = &rest[start + 2 + end + 1..],
            None => {
                // no closing brace — keep the remainder verbatim
                rest = &rest[start..];
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Strip a leading dirty indicator (`●`/`•`/`*`) plus surrounding whitespace and an
/// optional leading separator, so the workspace name isn't polluted.
fn strip_leading_dirty(s: &str) -> &str {
    let s = s.trim_start();
    let s = s.trim_start_matches(|c: char| {
        c == '\u{25CF}' /* ● */ || c == '\u{2022}' /* • */ || c == '*' || c.is_whitespace()
    });
    let s = s.strip_prefix("- ").unwrap_or(s);
    s.trim_start()
}

/// Parse a `rootName` into `(workspace, remote)`, extracting a trailing `[KIND: value]` tag.
fn parse_root(root: &str) -> (String, Remote) {
    let root = root.trim();
    if let Some(stripped) = root.strip_suffix(']') {
        if let Some(open) = stripped.rfind('[') {
            let inner = stripped[open + 1..].trim();
            let workspace = stripped[..open].trim().to_string();
            let remote = match inner.split_once(':') {
                Some((kind, val)) => {
                    let val = val.trim().to_string();
                    match kind.trim() {
                        "SSH" => Some(Remote::Ssh(val)),
                        "WSL" => Some(Remote::Wsl(val)),
                        "Dev Container" => Some(Remote::DevContainer(val)),
                        "Codespaces" => Some(Remote::Codespaces(val)),
                        _ => None,
                    }
                }
                // bracket with no colon, e.g. "[Codespaces]"
                None if inner == "Codespaces" => Some(Remote::Codespaces(String::new())),
                None => None,
            };
            if let Some(remote) = remote {
                return (workspace, remote);
            }
        }
    }
    (root.to_string(), Remote::Local)
}

/// Parsed Microsoft Edge window title (WI #474).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeTitle {
    /// Display label: the user-set window name, or (for unnamed) the active tab title.
    pub label: String,
    /// Whether this is a user-named window ("Name window…") vs. tab-title-derived.
    pub named: bool,
    /// Best-effort tab count (active + "and N more pages"), unnamed windows only.
    pub tab_count: Option<u32>,
}

/// Parse an Edge window title. Named windows carry just the name; unnamed windows read
/// `<active tab>[ and N more pages] - <Profile> - Microsoft Edge`. Edge's branding hides a
/// zero-width space (U+200B) between "Microsoft" and "Edge" — normalized out first.
pub fn parse_edge_title(title: &str) -> Option<EdgeTitle> {
    let norm: String = title.chars().filter(|&c| c != '\u{200B}').collect();
    let norm = norm.trim();
    if norm.is_empty() {
        return None;
    }
    const APP_SUFFIX: &str = " - Microsoft Edge";
    match norm.strip_suffix(APP_SUFFIX) {
        // Unnamed: strip the trailing " - <Profile>" (profile has no inner " - "), then the
        // " and N more pages" tab hint, leaving the active tab title.
        Some(rest) => {
            let no_profile = match rest.rfind(" - ") {
                Some(i) => rest[..i].trim(),
                None => rest.trim(),
            };
            let (base, more) = split_more_pages(no_profile);
            let label = base.trim().to_string();
            if label.is_empty() {
                return None;
            }
            Some(EdgeTitle {
                label,
                named: false,
                tab_count: Some(more.map_or(1, |n| n + 1)),
            })
        }
        // Named: the whole title is the user's window name.
        None => Some(EdgeTitle {
            label: norm.to_string(),
            named: true,
            tab_count: None,
        }),
    }
}

/// Split a trailing ` and N more page[s]` off a string, returning `(base, N)`.
fn split_more_pages(s: &str) -> (&str, Option<u32>) {
    if let Some(idx) = s.rfind(" and ") {
        let tail = &s[idx + 5..];
        if let Some(rest) = tail
            .strip_suffix(" more pages")
            .or_else(|| tail.strip_suffix(" more page"))
        {
            if let Ok(n) = rest.trim().parse::<u32>() {
                return (&s[..idx], Some(n));
            }
        }
    }
    (s, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(t: &str) -> ParsedTitle {
        parse_title(t).expect("should parse")
    }

    // --- real samples captured from live windows (sprints/001) ---

    #[test]
    fn local_insiders() {
        let r = p("kvscf - README.md - Visual Studio Code - Insiders");
        assert_eq!(r.workspace, "kvscf");
        assert_eq!(r.remote, Remote::Local);
        assert_eq!(r.active_file.as_deref(), Some("README.md"));
    }

    #[test]
    fn ssh_kai() {
        let r = p("kvllm [SSH: kai] - leaderboard.md - Visual Studio Code - Insiders");
        assert_eq!(r.workspace, "kvllm");
        assert_eq!(r.remote, Remote::Ssh("kai".into()));
        assert_eq!(r.active_file.as_deref(), Some("leaderboard.md"));
    }

    #[test]
    fn ssh_kubs0_with_parens_in_active() {
        let r = p("kpidash [SSH: kubs0] - kpidash-cards (7bb4b67) (kpidash-cards (7bb4b67)) - Visual Studio Code - Insiders");
        assert_eq!(r.workspace, "kpidash");
        assert_eq!(r.remote, Remote::Ssh("kubs0".into()));
        assert_eq!(
            r.active_file.as_deref(),
            Some("kpidash-cards (7bb4b67) (kpidash-cards (7bb4b67))")
        );
    }

    #[test]
    fn active_containing_colon() {
        let r = p("kyac [SSH: kai] - Start sprint korg:437 - Visual Studio Code - Insiders");
        assert_eq!(r.workspace, "kyac");
        assert_eq!(r.remote, Remote::Ssh("kai".into()));
        assert_eq!(r.active_file.as_deref(), Some("Start sprint korg:437"));
    }

    // --- variants not open during the live check, exercised here ---

    #[test]
    fn stable_local() {
        let r = p("myproj - main.rs - Visual Studio Code");
        assert_eq!(r.workspace, "myproj");
        assert_eq!(r.remote, Remote::Local);
        assert_eq!(r.active_file.as_deref(), Some("main.rs"));
    }

    #[test]
    fn wsl() {
        let r = p("api [WSL: Ubuntu] - server.ts - Visual Studio Code");
        assert_eq!(r.workspace, "api");
        assert_eq!(r.remote, Remote::Wsl("Ubuntu".into()));
        assert_eq!(r.active_file.as_deref(), Some("server.ts"));
    }

    #[test]
    fn dev_container() {
        let r = p("svc [Dev Container: node] - index.ts - Visual Studio Code");
        assert_eq!(r.workspace, "svc");
        assert_eq!(r.remote, Remote::DevContainer("node".into()));
    }

    #[test]
    fn no_active_file() {
        let r = p("justfolder - Visual Studio Code - Insiders");
        assert_eq!(r.workspace, "justfolder");
        assert_eq!(r.remote, Remote::Local);
        assert_eq!(r.active_file, None);
    }

    // --- robustness ---

    #[test]
    fn leading_dirty_stripped() {
        let r = p("\u{25CF} kvllm [SSH: kai] - leaderboard.md - Visual Studio Code - Insiders");
        assert_eq!(r.workspace, "kvllm");
        assert_eq!(r.remote, Remote::Ssh("kai".into()));
    }

    #[test]
    fn stray_var_token_stripped() {
        // simulate the historical ${seperator} typo leaking a literal token
        let r = p("proj - file.rs - Visual Studio Code${seperator}");
        assert_eq!(r.workspace, "proj");
        assert_eq!(r.active_file.as_deref(), Some("file.rs"));
    }

    #[test]
    fn empty_title_is_none() {
        assert!(parse_title("").is_none());
        assert!(parse_title("   ").is_none());
    }

    // --- Edge titles (WI #474), from real captured windows ---

    fn e(t: &str) -> EdgeTitle {
        parse_edge_title(t).expect("edge parse")
    }

    #[test]
    fn edge_named() {
        let r = e("korg");
        assert_eq!(r.label, "korg");
        assert!(r.named);
        assert_eq!(r.tab_count, None);
    }

    #[test]
    fn edge_named_with_spaces() {
        let r = e("AI-2 Computer Purchase");
        assert_eq!(r.label, "AI-2 Computer Purchase");
        assert!(r.named);
    }

    #[test]
    fn edge_unnamed_single_tab() {
        let r = e("hvsim — galaxy - Personal - Microsoft\u{200B} Edge");
        assert_eq!(r.label, "hvsim — galaxy");
        assert!(!r.named);
        assert_eq!(r.tab_count, Some(1));
    }

    #[test]
    fn edge_unnamed_inner_dashes_and_more_pages() {
        let r =
            e("Home - Dashboards - Grafana and 1 more page - Personal - Microsoft\u{200B} Edge");
        assert_eq!(r.label, "Home - Dashboards - Grafana");
        assert!(!r.named);
        assert_eq!(r.tab_count, Some(2));
    }

    #[test]
    fn edge_unnamed_many_tabs() {
        let r = e("scapula - Search and 6 more pages - Personal - Microsoft\u{200B} Edge");
        assert_eq!(r.label, "scapula - Search");
        assert_eq!(r.tab_count, Some(7));
    }
}
