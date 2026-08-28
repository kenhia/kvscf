//! Window sets (WI #469/#470). Resolve open VS Code windows to their full folder URIs via VS
//! Code's own `workspaceStorage`, so they can be saved/restored and relaunched, and drive the
//! Update Assist flow. Relaunch is a local `code`/`code-insiders --folder-uri` call — kvscf runs
//! on the same box, so no krcmd round-trip is needed.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use kvscf_core::{scan, App, Instance};

/// One entry in a window set: which build + the exact folder URI to relaunch.
#[derive(Debug, Clone)]
pub struct SetEntry {
    pub app: App,
    /// Verbatim as VS Code stored it (percent-encoded) — relaunched as-is.
    pub uri: String,
    /// Human label, e.g. `korg (kai)`.
    pub label: String,
    /// Workspace (folder) name on its own — kept structured so consumers (the wire payload)
    /// never have to reverse-parse it back out of `label` (WI #495). Only the remote build
    /// reads these two today, hence the cfg_attr.
    #[cfg_attr(not(feature = "remote"), allow(dead_code))]
    pub workspace: String,
    /// Remote host, `None` when local.
    #[cfg_attr(not(feature = "remote"), allow(dead_code))]
    pub host: Option<String>,
}

impl SetEntry {
    /// Identity for favorites (sprint 008): same build + same folder, ignoring the display label.
    /// A given folder open in Stable vs Insiders are distinct targets.
    pub fn same_target(&self, other: &SetEntry) -> bool {
        self.app == other.app && self.uri == other.uri
    }
}

/// A folder URI VS Code has recorded, decoded enough to match against an open window.
struct KnownUri {
    basename: String,
    host: Option<String>,
    uri: String,
    /// When this folder was last open — the tie-break between two folders sharing a leaf name.
    used_at: SystemTime,
}

fn appdata() -> Option<PathBuf> {
    // %APPDATA% can be absent for a process launched very early in boot (the same window as
    // userreg's .DEFAULT bug); fall back to the standard path under %USERPROFILE% so saves
    // land in the same place a normal launch reads from.
    std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| {
        std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Roaming"))
    })
}

/// `%APPDATA%` subdir name for a build.
fn storage_dir_name(app: App) -> &'static str {
    match app {
        App::Insiders => "Code - Insiders",
        App::Exploration => "Code - Exploration",
        _ => "Code",
    }
}

/// Launcher command for a build (assumed on PATH).
fn launcher(app: App) -> &'static str {
    match app {
        App::Insiders => "code-insiders",
        App::Exploration => "code-exploration",
        _ => "code",
    }
}

/// Every folder URI VS Code has recorded for this build (from `workspaceStorage/*/workspace.json`).
fn known_uris(app: App) -> Vec<KnownUri> {
    let mut out = Vec::new();
    let Some(base) = appdata() else {
        return out;
    };
    let dir = base
        .join(storage_dir_name(app))
        .join("User")
        .join("workspaceStorage");
    let Ok(read) = fs::read_dir(&dir) else {
        return out;
    };
    for entry in read.flatten() {
        let dir = entry.path();
        let wj = dir.join("workspace.json");
        let Ok(text) = fs::read_to_string(&wj) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let uri = json
            .get("folder")
            .and_then(|v| v.as_str())
            .or_else(|| json.get("workspace").and_then(|v| v.as_str()));
        let Some(uri) = uri else {
            continue;
        };
        if let Some((basename, host)) = parse_uri(uri) {
            out.push(KnownUri {
                basename,
                host,
                uri: uri.to_string(),
                used_at: last_used(&dir),
            });
        }
    }
    out
}

/// When a `workspaceStorage` folder was last *used* — the recency signal that tells two folders
/// sharing a leaf name apart (kvscf #1682).
///
/// **Not `workspace.json`'s mtime.** VS Code writes that file once, when it creates the storage
/// folder, and never touches it again: its mtime means "first opened", so of two same-named
/// folders whichever was *created* later wins forever. That is what pinned the open
/// `klams (kubs0)` window to `/ai/klams` no matter how often it was re-favorited — its storage
/// folder was created eight days after the right one's, in May, and that never changes.
///
/// `state.vscdb` is rewritten as the window is *used*, which is the signal we actually want. It
/// is not a heartbeat — two idle windows can both sit unflushed for hours — so it orders windows
/// by recency of use, not of existence. That is enough to pick the right folder whenever only one
/// of a same-named pair is in play, which is the case this fixes. Fall back to `workspace.json`
/// only when it is absent (a folder VS Code recorded but never opened).
fn last_used(storage_dir: &Path) -> SystemTime {
    let mtime = |name: &str| {
        storage_dir
            .join(name)
            .metadata()
            .and_then(|m| m.modified())
            .ok()
    };
    mtime("state.vscdb")
        .or_else(|| mtime("workspace.json"))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// The stored folder an open window belongs to: same leaf name, same host, most recently used.
///
/// A window title carries only the folder's leaf name, so two folders that end in the same name
/// on the same host are indistinguishable from the title alone and recency is the only tie-break
/// available. That makes the *right* recency stamp load-bearing — see [`last_used`].
///
/// Known limit, deliberately not solved: two same-named folders open **at once** collapse to
/// whichever flushed last, so both rows resolve to one URI (live on cleo — `/ai/klams`, klams's
/// runtime data dir, and `~/src/ai/klams`, its repo). Nothing in the title separates them, so
/// fixing it needs an identity the window doesn't carry; a favorite made from the wrong one of
/// the pair is repairable by hand in the favorite editor.
fn best_match<'a>(
    uris: &'a [KnownUri],
    workspace: &str,
    host: Option<&str>,
) -> Option<&'a KnownUri> {
    uris.iter()
        .filter(|u| u.basename == workspace && u.host.as_deref() == host)
        .max_by_key(|u| u.used_at)
}

/// Extract `(basename, host)` from a stored folder URI. `host` is `None` for local folders.
pub fn parse_uri(uri: &str) -> Option<(String, Option<String>)> {
    let decoded = percent_decode(uri);
    if let Some(rest) = decoded.strip_prefix("vscode-remote://") {
        // rest = "ssh-remote+[user@]host/abs/path"
        let (authority, path) = rest.split_once('/')?;
        let host = authority
            .strip_prefix("ssh-remote+")
            .map(|h| h.rsplit('@').next().unwrap_or(h).to_string());
        let basename = path.trim_end_matches('/').rsplit('/').next()?.to_string();
        (!basename.is_empty()).then_some((basename, host))
    } else if let Some(rest) = decoded.strip_prefix("file://") {
        // rest = "/d:/ClaudeWorks/kvscf"
        let basename = rest
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()?
            .to_string();
        (!basename.is_empty()).then_some((basename, None))
    } else {
        None
    }
}

/// Decode a stored URI for display — `ssh-remote%2Bkubs0` is unreadable as an identity to check.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolve open VS Code windows (already scanned by the caller) to set entries. Returns
/// `(resolved, unresolved)` where `resolved` pairs each open window with the folder URI to
/// relaunch it, and `unresolved` lists labels whose URI couldn't be found (dropped).
///
/// Takes the caller's scan so the app's 1s refresh doesn't run a second `EnumWindows` +
/// per-window process-image pass on top of the one it just did (WI #499).
pub fn resolve_open_set(instances: &[Instance]) -> (Vec<(Instance, SetEntry)>, Vec<String>) {
    let mut cache: HashMap<&'static str, Vec<KnownUri>> = HashMap::new();
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for inst in instances {
        let uris = cache
            .entry(storage_dir_name(inst.app))
            .or_insert_with(|| known_uris(inst.app));
        let host = inst.remote.host();
        match best_match(uris, &inst.workspace, host) {
            Some(u) => {
                let entry = SetEntry {
                    app: inst.app,
                    uri: u.uri.clone(),
                    label: inst.label(),
                    workspace: inst.workspace.clone(),
                    host: host.map(str::to_string),
                };
                resolved.push((inst.clone(), entry));
            }
            None => unresolved.push(inst.label()),
        }
    }
    (resolved, unresolved)
}

/// Scan + resolve in one call — for one-shot callers (probes, button actions) that want a
/// fresh view rather than the app's last cached scan.
pub fn resolve_open_set_now() -> (Vec<(Instance, SetEntry)>, Vec<String>) {
    resolve_open_set(&scan())
}

/// Launch one folder URI in its build (local `code`/`code-insiders`, detached, no console).
pub fn launch(entry: &SetEntry) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/c", launcher(entry.app), "--folder-uri", &entry.uri])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        let _ = entry;
        Ok(())
    }
}

/// Relaunch a set on a background thread, staggered (so a burst of remote reconnects doesn't
/// stampede). Returns immediately.
pub fn relaunch(entries: Vec<SetEntry>, stagger: Duration) {
    std::thread::spawn(move || {
        for (i, e) in entries.iter().enumerate() {
            if i > 0 {
                std::thread::sleep(stagger);
            }
            let _ = launch(e);
        }
    });
}

// --- persisted named sets (WI #469) ---

fn sets_dir() -> Option<PathBuf> {
    appdata().map(|p| p.join("kvscf").join("sets"))
}

/// Write a `{ "entries": [...] }` JSON file (creating parent dirs). Shared by named sets and the
/// favorites list.
fn write_entries(path: PathBuf, entries: &[SetEntry]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let arr: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| serde_json::json!({ "app": e.app.key(), "uri": e.uri, "label": e.label }))
        .collect();
    fs::write(path, serde_json::json!({ "entries": arr }).to_string())
}

/// Read a `{ "entries": [...] }` JSON file back into set entries. `None` if it's missing/unreadable.
/// The file stores `{app, uri, label}`; workspace/host are re-derived from the URI (falling back
/// to the label verbatim if the URI is in a shape we don't know).
fn read_entries(path: PathBuf) -> Option<Vec<SetEntry>> {
    let text = fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let arr = json.get("entries")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|e| {
                let uri = e.get("uri")?.as_str()?.to_string();
                let label = e
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let (workspace, host) = parse_uri(&uri).unwrap_or((label.clone(), None));
                Some(SetEntry {
                    app: App::from_key(e.get("app")?.as_str()?),
                    uri,
                    label,
                    workspace,
                    host,
                })
            })
            .collect(),
    )
}

pub fn save_set(name: &str, entries: &[SetEntry]) -> std::io::Result<()> {
    let dir = sets_dir().ok_or_else(|| std::io::Error::other("no APPDATA"))?;
    write_entries(dir.join(format!("{name}.json")), entries)
}

pub fn load_set(name: &str) -> Option<Vec<SetEntry>> {
    read_entries(sets_dir()?.join(format!("{name}.json")))
}

// --- favorites (sprint 008) ---

fn favorites_path() -> Option<PathBuf> {
    appdata().map(|p| p.join("kvscf").join("favorites.json"))
}

/// Load the persisted favorites (empty if none saved yet).
pub fn load_favorites() -> Vec<SetEntry> {
    favorites_path().and_then(read_entries).unwrap_or_default()
}

/// Persist the favorites list.
pub fn save_favorites(entries: &[SetEntry]) -> std::io::Result<()> {
    let path = favorites_path().ok_or_else(|| std::io::Error::other("no APPDATA"))?;
    write_entries(path, entries)
}

/// Relaunch a favorite by its folder URI — the action behind a dashboard tap on a not-open
/// favorite row (whose published `id` *is* the URI). Reads the persisted list, so it works from
/// the remote subscriber thread, which has no app state. `false` if no favorite matches.
#[allow(dead_code)] // only called from the `remote` build
pub fn launch_favorite(uri: &str) -> bool {
    let Some(entry) = load_favorites().into_iter().find(|f| f.uri == uri) else {
        return false;
    };
    launch(&entry).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_uri (WI #498): the favorites/relaunch identity path ---

    #[test]
    fn ssh_uri() {
        assert_eq!(
            parse_uri("vscode-remote://ssh-remote+kai/home/ken/src/kyac"),
            Some(("kyac".into(), Some("kai".into())))
        );
    }

    #[test]
    fn ssh_uri_with_user() {
        assert_eq!(
            parse_uri("vscode-remote://ssh-remote+ken@kai/home/ken/src/foo"),
            Some(("foo".into(), Some("kai".into())))
        );
    }

    #[test]
    fn local_file_uri_with_encoded_drive() {
        // VS Code stores Windows drives percent-encoded: file:///d%3A/ClaudeWorks/kvscf
        assert_eq!(
            parse_uri("file:///d%3A/ClaudeWorks/kvscf"),
            Some(("kvscf".into(), None))
        );
    }

    #[test]
    fn file_uri_trailing_slash_and_encoded_space() {
        assert_eq!(
            parse_uri("file:///c%3A/My%20Projects/app/"),
            Some(("app".into(), None))
        );
    }

    #[test]
    fn unknown_scheme_is_none() {
        assert_eq!(parse_uri("http://example.com/x"), None);
        // ssh authority with no path — degenerate, unmatchable.
        assert_eq!(parse_uri("vscode-remote://ssh-remote+kai"), None);
    }

    // --- percent_decode ---

    #[test]
    fn decodes_hex_escapes() {
        assert_eq!(percent_decode("%41%42c"), "ABc");
        assert_eq!(percent_decode("d%3A/x"), "d:/x");
    }

    #[test]
    fn malformed_escapes_pass_through() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%G1"), "%G1");
        assert_eq!(percent_decode("%2"), "%2");
    }

    // --- the same-leaf-name tie-break (#1682) ---

    /// A scratch dir of our own, so these can run alongside each other.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("kvscf-winset-test").join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Write `name` into `dir` with a fixed mtime, the way a storage folder carries one.
    fn stamped(dir: &Path, name: &str, at: SystemTime) {
        let path = dir.join(name);
        fs::write(&path, "{}").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    fn known(basename: &str, host: Option<&str>, uri: &str, used_at: SystemTime) -> KnownUri {
        KnownUri {
            basename: basename.into(),
            host: host.map(str::to_string),
            uri: uri.into(),
            used_at,
        }
    }

    #[test]
    fn last_used_reads_state_vscdb_not_workspace_json() {
        // The shape that caused #1682: workspace.json is the *newer* file, and following it is
        // exactly the mistake — state.vscdb is when the folder was actually last open.
        let dir = scratch("prefers-state");
        let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let new = SystemTime::UNIX_EPOCH + Duration::from_secs(9_000);
        stamped(&dir, "workspace.json", new);
        stamped(&dir, "state.vscdb", old);
        assert_eq!(last_used(&dir), old);
    }

    #[test]
    fn last_used_falls_back_to_workspace_json() {
        // A folder VS Code recorded but never opened has no state.vscdb at all.
        let dir = scratch("falls-back");
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(4_200);
        stamped(&dir, "workspace.json", at);
        assert_eq!(last_used(&dir), at);
    }

    #[test]
    fn last_used_of_an_empty_dir_loses_every_tie_break() {
        assert_eq!(last_used(&scratch("empty")), SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn duplicate_leaf_names_resolve_to_the_recently_used_one() {
        // The live case: two `klams` folders on kubs0. The one whose window is actually open —
        // ~/src/ai/klams — must win, regardless of which storage folder VS Code created first.
        let older = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let newer = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000);
        let uris = vec![
            known(
                "klams",
                Some("kubs0"),
                "vscode-remote://ssh-remote%2Bkubs0/ai/klams",
                older,
            ),
            known(
                "klams",
                Some("kubs0"),
                "vscode-remote://ssh-remote%2Bkubs0/home/ken/src/ai/klams",
                newer,
            ),
        ];
        let best = best_match(&uris, "klams", Some("kubs0")).expect("a match");
        assert_eq!(
            best.uri,
            "vscode-remote://ssh-remote%2Bkubs0/home/ken/src/ai/klams"
        );
    }

    #[test]
    fn a_same_named_folder_on_another_host_is_never_the_match() {
        // kai's `klams` is a different project entirely; recency must not reach across hosts.
        let older = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let newer = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000);
        let uris = vec![
            known(
                "klams",
                Some("kubs0"),
                "vscode-remote://ssh-remote%2Bkubs0/home/ken/src/ai/klams",
                older,
            ),
            known(
                "klams",
                Some("kai"),
                "vscode-remote://ssh-remote%2Bkai/home/ken/src/ai/klams",
                newer,
            ),
        ];
        let best = best_match(&uris, "klams", Some("kubs0")).expect("a match");
        assert_eq!(best.host.as_deref(), Some("kubs0"));
        // …and a local window never matches a remote folder of the same name.
        assert!(best_match(&uris, "klams", None).is_none());
    }

    #[test]
    fn an_unknown_workspace_has_no_match() {
        let uris = vec![known(
            "klams",
            Some("kubs0"),
            "vscode-remote://ssh-remote%2Bkubs0/ai/klams",
            SystemTime::UNIX_EPOCH,
        )];
        assert!(best_match(&uris, "korg", Some("kubs0")).is_none());
    }

    // --- persisted-entry roundtrip: workspace/host re-derived from the URI on load ---

    #[test]
    fn read_entries_derives_parts_from_uri() {
        let dir = std::env::temp_dir().join("kvscf-winset-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("set.json");
        let entries = vec![SetEntry {
            app: App::Insiders,
            uri: "vscode-remote://ssh-remote+kai/home/ken/src/kyac".into(),
            label: "kyac (kai)".into(),
            workspace: "kyac".into(),
            host: Some("kai".into()),
        }];
        write_entries(path.clone(), &entries).unwrap();
        let back = read_entries(path).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].app, App::Insiders);
        assert_eq!(back[0].workspace, "kyac");
        assert_eq!(back[0].host.as_deref(), Some("kai"));
        assert_eq!(back[0].label, "kyac (kai)");
    }
}
