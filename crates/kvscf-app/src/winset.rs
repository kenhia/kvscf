//! Window sets (WI #469/#470). Resolve open VS Code windows to their full folder URIs via VS
//! Code's own `workspaceStorage`, so they can be saved/restored and relaunched, and drive the
//! Update Assist flow. Relaunch is a local `code`/`code-insiders --folder-uri` call — kvscf runs
//! on the same box, so no krcmd round-trip is needed.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
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
}

/// A folder URI VS Code has recorded, decoded enough to match against an open window.
struct KnownUri {
    basename: String,
    host: Option<String>,
    uri: String,
    mtime: SystemTime,
}

fn appdata() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
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

fn app_key(app: App) -> &'static str {
    match app {
        App::Stable => "stable",
        App::Insiders => "insiders",
        App::Exploration => "exploration",
        App::Unknown => "unknown",
    }
}

fn app_from_key(k: &str) -> App {
    match k {
        "insiders" => App::Insiders,
        "exploration" => App::Exploration,
        _ => App::Stable,
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
        let wj = entry.path().join("workspace.json");
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
        let mtime = wj
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if let Some((basename, host)) = parse_uri(uri) {
            out.push(KnownUri {
                basename,
                host,
                uri: uri.to_string(),
                mtime,
            });
        }
    }
    out
}

/// Extract `(basename, host)` from a stored folder URI. `host` is `None` for local folders.
fn parse_uri(uri: &str) -> Option<(String, Option<String>)> {
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

fn percent_decode(s: &str) -> String {
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

/// Resolve the currently open VS Code windows to set entries. Returns `(resolved, unresolved)`
/// where `resolved` pairs each open window with the folder URI to relaunch it, and `unresolved`
/// lists labels whose URI couldn't be found (dropped).
pub fn resolve_open_set() -> (Vec<(Instance, SetEntry)>, Vec<String>) {
    let mut cache: HashMap<&'static str, Vec<KnownUri>> = HashMap::new();
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for inst in scan() {
        let uris = cache
            .entry(storage_dir_name(inst.app))
            .or_insert_with(|| known_uris(inst.app));
        let host = inst.remote.host();
        let best = uris
            .iter()
            .filter(|u| u.basename == inst.workspace && u.host.as_deref() == host)
            .max_by_key(|u| u.mtime);
        match best {
            Some(u) => {
                let entry = SetEntry {
                    app: inst.app,
                    uri: u.uri.clone(),
                    label: inst.label(),
                };
                resolved.push((inst, entry));
            }
            None => unresolved.push(inst.label()),
        }
    }
    (resolved, unresolved)
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

pub fn save_set(name: &str, entries: &[SetEntry]) -> std::io::Result<()> {
    let dir = sets_dir().ok_or_else(|| std::io::Error::other("no APPDATA"))?;
    fs::create_dir_all(&dir)?;
    let arr: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| serde_json::json!({ "app": app_key(e.app), "uri": e.uri, "label": e.label }))
        .collect();
    let json = serde_json::json!({ "entries": arr }).to_string();
    fs::write(dir.join(format!("{name}.json")), json)
}

pub fn load_set(name: &str) -> Option<Vec<SetEntry>> {
    let dir = sets_dir()?;
    let text = fs::read_to_string(dir.join(format!("{name}.json"))).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let arr = json.get("entries")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|e| {
                Some(SetEntry {
                    app: app_from_key(e.get("app")?.as_str()?),
                    uri: e.get("uri")?.as_str()?.to_string(),
                    label: e
                        .get("label")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
            })
            .collect(),
    )
}
