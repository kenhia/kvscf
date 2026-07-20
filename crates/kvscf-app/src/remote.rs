//! Remote channel to kdeskdash over the shared "claude-feed" Redis (WI #471, sprint 003).
//!
//! Contract (see `docs/kdeskdash-vscode-mode.md`):
//! - **Publish** the instance list to `kvscf:instances:<host>` (JSON String, TTL 10s,
//!   republished ~every app refresh). kdeskdash SCANs `kvscf:instances:*` and renders rows.
//!   Each row carries `running` + `favorite`; **favorites with no open window are appended as
//!   `running:false` rows whose `id` is the folder URI** rather than an HWND (sprint 008).
//! - **Publish** the configured apps to `kvscf:apps:<host>` (JSON, same TTL); each is
//!   `{key,label,running,id?}` — `id` is the HWND when running (sprint 007 Apps tab).
//! - **Subscribe** to `kvscf:focus:<host>` (pub/sub). The dashboard just echoes back the tapped
//!   row's id, and we route it: `{token,id:<int>,maximize}` foregrounds that HWND;
//!   `{token,id:<uri>}` relaunches that closed favorite (`crate::winset::launch_favorite`);
//!   `{token,app:<key>}` does **focus-if-running-else-launch** for a configured app
//!   (`crate::apps::activate`). Token gates all three.
//!
//! Redis itself is unauthenticated (trusted LAN), so `KVSCF_TOKEN` is the app-level gate on the
//! focus command (the only action). Endpoint + token come from env / a `.env` file.
//!
//! This whole module is compiled out of the `kvscf-local` build (feature `remote` off).

#![allow(dead_code)]

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kvscf_core::{focus_with, App, EdgeWindow, Instance, Remote};

use std::collections::HashSet;

use crate::apps::{self, AppEntry};
use crate::winset::{self, SetEntry};

const DEFAULT_HOST: &str = "192.168.1.144"; // rpidash2 LAN IP (pinned, per handoff)
const DEFAULT_PORT: u16 = 6380;
const DEFAULT_HOST_NAME: &str = "cleo";
const INSTANCES_TTL_SECS: u64 = 10;
const RECONNECT_BACKOFF: Duration = Duration::from_secs(5);

/// Resolved connection + identity config. `None` from [`Config::load`] disables the channel.
#[derive(Clone)]
struct Config {
    redis_host: String,
    redis_port: u16,
    token: String,
    this_host: String,
}

impl Config {
    fn load() -> Option<Config> {
        // Best-effort: pull KEY=VALUE from a .env in cwd or next to the exe (for host/port
        // overrides and as the token fallback).
        dotenvy::dotenv().ok();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                dotenvy::from_path(dir.join(".env")).ok();
            }
        }

        // Token: registry (preferred — HKCU\Software\kenhia\kvscf) → env/.env fallback. It works
        // regardless of where the exe is launched from (a pinned launch from C:\tools\bin has no
        // cwd/exe-dir .env). Mandatory: without it the channel stays off rather than run open.
        let token = token_from_registry()
            .or_else(|| std::env::var("KVSCF_TOKEN").ok())
            .filter(|t| !t.is_empty())?;

        Some(Config {
            redis_host: env_or("KVSCF_REDIS_HOST", DEFAULT_HOST),
            redis_port: std::env::var("KVSCF_REDIS_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_PORT),
            token,
            this_host: env_or("KVSCF_HOST_NAME", &computer_name()),
        })
    }

    fn url(&self) -> String {
        format!("redis://{}:{}", self.redis_host, self.redis_port)
    }

    fn instances_key(&self) -> String {
        format!("kvscf:instances:{}", self.this_host)
    }

    fn edge_key(&self) -> String {
        format!("kvscf:edge:{}", self.this_host)
    }

    fn apps_key(&self) -> String {
        format!("kvscf:apps:{}", self.this_host)
    }

    fn focus_channel(&self) -> String {
        format!("kvscf:focus:{}", self.this_host)
    }
}

/// One published snapshot: the VS Code instances, the Edge windows, the configured apps, and the
/// favorites overlay (which open windows are starred + the favorites that aren't open).
struct Snapshot {
    instances: Vec<Instance>,
    edge: Vec<EdgeWindow>,
    apps: Vec<AppEntry>,
    favorited: HashSet<i64>,
    dimmed_favorites: Vec<SetEntry>,
}

/// The app-facing handle. Owns the sender that feeds the publisher thread.
pub struct Channel {
    tx: Sender<Snapshot>,
    host: String,
}

impl Channel {
    /// Start the publisher + subscriber threads. Returns `None` (channel disabled) if no
    /// `KVSCF_TOKEN` is configured.
    pub fn start() -> Option<Channel> {
        let cfg = Config::load()?;
        let host = cfg.this_host.clone();
        let (tx, rx) = mpsc::channel::<Snapshot>();

        {
            let cfg = cfg.clone();
            thread::Builder::new()
                .name("kvscf-redis-pub".into())
                .spawn(move || publisher_loop(cfg, rx))
                .ok()?;
        }
        {
            let cfg = cfg.clone();
            thread::Builder::new()
                .name("kvscf-redis-sub".into())
                .spawn(move || subscriber_loop(cfg))
                .ok()?;
        }
        eprintln!(
            "kvscf: remote channel up — {} (publish {}, focus {})",
            cfg.url(),
            cfg.instances_key(),
            cfg.focus_channel()
        );
        Some(Channel { tx, host })
    }

    /// Hand the latest window/app lists to the publisher thread (non-blocking). `favorited` is the
    /// set of open HWNDs that are starred; `dimmed_favorites` are favorites with no open window.
    pub fn publish(
        &self,
        items: &[Instance],
        edge: &[EdgeWindow],
        apps: &[AppEntry],
        favorited: &HashSet<i64>,
        dimmed_favorites: &[SetEntry],
    ) {
        let _ = self.tx.send(Snapshot {
            instances: items.to_vec(),
            edge: edge.to_vec(),
            apps: apps.to_vec(),
            favorited: favorited.clone(),
            dimmed_favorites: dimmed_favorites.to_vec(),
        });
    }

    pub fn host(&self) -> &str {
        &self.host
    }
}

/// Publisher: SET the instance + edge lists with a TTL on every snapshot the app sends.
fn publisher_loop(cfg: Config, rx: Receiver<Snapshot>) {
    let inst_key = cfg.instances_key();
    let edge_key = cfg.edge_key();
    let apps_key = cfg.apps_key();
    loop {
        let client = match redis::Client::open(cfg.url()) {
            Ok(c) => c,
            Err(_) => {
                thread::sleep(RECONNECT_BACKOFF);
                continue;
            }
        };
        let mut con = match client.get_connection() {
            Ok(c) => c,
            Err(_) => {
                thread::sleep(RECONNECT_BACKOFF);
                continue;
            }
        };

        // Publish snapshots until the app closes (sender dropped) or Redis errors.
        loop {
            // Block for the next snapshot, then collapse any backlog to the latest.
            let mut latest = match rx.recv() {
                Ok(v) => v,
                Err(_) => return, // app is shutting down
            };
            while let Ok(v) = rx.try_recv() {
                latest = v;
            }

            let set = |key: &str, payload: String, con: &mut redis::Connection| -> bool {
                redis::cmd("SET")
                    .arg(key)
                    .arg(payload)
                    .arg("EX")
                    .arg(INSTANCES_TTL_SECS)
                    .query::<()>(con)
                    .is_ok()
            };
            let ok = set(
                &inst_key,
                build_instances_json(
                    &cfg,
                    &latest.instances,
                    &latest.favorited,
                    &latest.dimmed_favorites,
                ),
                &mut con,
            ) && set(&edge_key, build_edge_json(&cfg, &latest.edge), &mut con)
                && set(&apps_key, build_apps_json(&cfg, &latest.apps), &mut con);
            if !ok {
                break; // drop out to reconnect
            }
        }
        thread::sleep(RECONNECT_BACKOFF);
    }
}

/// Subscriber: consume focus commands and foreground the requested window.
fn subscriber_loop(cfg: Config) {
    let channel = cfg.focus_channel();
    loop {
        let client = match redis::Client::open(cfg.url()) {
            Ok(c) => c,
            Err(_) => {
                thread::sleep(RECONNECT_BACKOFF);
                continue;
            }
        };
        let mut con = match client.get_connection() {
            Ok(c) => c,
            Err(_) => {
                thread::sleep(RECONNECT_BACKOFF);
                continue;
            }
        };
        let mut pubsub = con.as_pubsub();
        if pubsub.subscribe(&channel).is_err() {
            thread::sleep(RECONNECT_BACKOFF);
            continue;
        }

        // Loop ends (and we reconnect) when get_message() errors.
        while let Ok(msg) = pubsub.get_message() {
            let payload: String = match msg.get_payload() {
                Ok(p) => p,
                Err(_) => continue,
            };
            match parse_command(&payload, &cfg.token) {
                // Background-thread foreground — the hostile case the 001 recipe was built for.
                Some(Command::Focus { hwnd, maximize }) => {
                    focus_with(hwnd, maximize);
                }
                // Focus-if-running-else-launch the configured app (may spawn + poll).
                Some(Command::App { key }) => {
                    apps::activate(&key);
                }
                // Relaunch a favorite whose window is closed (reads the persisted list).
                Some(Command::Favorite { uri }) => {
                    winset::launch_favorite(&uri);
                }
                None => {}
            }
        }
        thread::sleep(RECONNECT_BACKOFF);
    }
}

/// Build the instance-list JSON payload. Open windows carry `running: true` plus a `favorite`
/// flag; favorites with no open window are appended as `running: false` rows whose **`id` is the
/// folder URI** rather than an HWND (sprint 008) — the dashboard greys those and echoes the id
/// back to relaunch them.
fn build_instances_json(
    cfg: &Config,
    items: &[Instance],
    favorited: &HashSet<i64>,
    dimmed: &[SetEntry],
) -> String {
    let mut instances: Vec<serde_json::Value> = items
        .iter()
        .map(|i| {
            serde_json::json!({
                "id": i.hwnd.to_string(),
                "label": i.label(),
                "workspace": i.workspace,
                "remote": remote_kind(&i.remote),
                "remote_host": i.remote.host(),
                "app": app_str(i.app),
                "active_file": i.active_file,
                "z_index": i.z_index,
                "running": true,
                "favorite": favorited.contains(&i.hwnd),
            })
        })
        .collect();

    instances.extend(dimmed.iter().map(|f| {
        let (workspace, host) = split_label(&f.label);
        serde_json::json!({
            "id": f.uri,                       // folder URI, not an HWND — it has no window
            "label": f.label,
            "workspace": workspace,
            "remote": if host.is_some() { "ssh" } else { "local" }, // best-effort from the URI
            "remote_host": host,
            "app": app_str(f.app),
            "active_file": serde_json::Value::Null,
            "z_index": serde_json::Value::Null,
            "running": false,
            "favorite": true,
        })
    }));

    serde_json::json!({
        "host": cfg.this_host,
        "ts": now_secs(),
        "instances": instances,
    })
    .to_string()
}

/// Split a `workspace (host)` label back into its parts (`host` is `None` when local).
fn split_label(label: &str) -> (String, Option<String>) {
    if let Some(idx) = label.rfind(" (") {
        if label.ends_with(')') {
            return (
                label[..idx].to_string(),
                Some(label[idx + 2..label.len() - 1].to_string()),
            );
        }
    }
    (label.to_string(), None)
}

/// Build the Edge-window JSON payload (WI #474). Same focus channel — `id` is the HWND.
fn build_edge_json(cfg: &Config, windows: &[EdgeWindow]) -> String {
    let items: Vec<serde_json::Value> = windows
        .iter()
        .map(|w| {
            serde_json::json!({
                "id": w.hwnd.to_string(),
                "label": w.label,
                "named": w.named,
                "tab_count": w.tab_count,
                "z_index": w.z_index,
            })
        })
        .collect();

    serde_json::json!({
        "host": cfg.this_host,
        "ts": now_secs(),
        "windows": items,
    })
    .to_string()
}

/// Build the configured-apps JSON payload (sprint 007). `id` (the HWND) is present only when the
/// app is running; the dashboard greys out non-running apps and sends `{app:<key>}` to launch them.
fn build_apps_json(cfg: &Config, apps: &[AppEntry]) -> String {
    let items: Vec<serde_json::Value> = apps
        .iter()
        .map(|a| {
            serde_json::json!({
                "key": a.key,
                "label": a.label,
                "running": a.running,
                "id": a.hwnd.map(|h| h.to_string()),
                "order": a.order,
            })
        })
        .collect();

    serde_json::json!({
        "host": cfg.this_host,
        "ts": now_secs(),
        "apps": items,
    })
    .to_string()
}

/// A parsed, authenticated command off the focus channel.
enum Command {
    /// Foreground an explicit HWND (VS Code / Edge rows).
    Focus { hwnd: i64, maximize: bool },
    /// Focus-if-running-else-launch a configured app by key (Apps tab).
    App { key: String },
    /// Relaunch a not-open Code favorite by folder URI (sprint 008).
    Favorite { uri: String },
}

/// Parse + authenticate a command. Returns `None` unless the token matches.
///
/// Routing, so kdeskdash can stay uniform (it just echoes back the tapped row's `id`):
/// - `app` present → [`Command::App`].
/// - `id` parses as an integer → an HWND → [`Command::Focus`].
/// - `id` is any other string → a favorite's folder URI → [`Command::Favorite`]. A not-open
///   favorite has no HWND, so its published `id` is the URI; URIs never parse as integers, which
///   makes the split unambiguous.
fn parse_command(payload: &str, expected_token: &str) -> Option<Command> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    if v.get("token")?.as_str()? != expected_token {
        return None;
    }
    if let Some(app) = v.get("app").and_then(|a| a.as_str()) {
        return Some(Command::App {
            key: app.to_string(),
        });
    }
    let id = v.get("id")?.as_str()?;
    match id.parse::<i64>() {
        Ok(hwnd) => {
            let maximize = v.get("maximize").and_then(|m| m.as_bool()).unwrap_or(false);
            Some(Command::Focus { hwnd, maximize })
        }
        Err(_) => Some(Command::Favorite {
            uri: id.to_string(),
        }),
    }
}

fn remote_kind(remote: &Remote) -> &'static str {
    match remote {
        Remote::Local => "local",
        Remote::Ssh(_) => "ssh",
        Remote::Wsl(_) => "wsl",
        Remote::DevContainer(_) => "devcontainer",
        Remote::Codespaces(_) => "codespaces",
    }
}

fn app_str(app: App) -> &'static str {
    match app {
        App::Stable => "stable",
        App::Insiders => "insiders",
        App::Exploration => "exploration",
        App::Unknown => "unknown",
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.to_lowercase())
        .unwrap_or_else(|| DEFAULT_HOST_NAME.to_string())
}

/// Preferred token source: `HKCU\Software\kenhia\kvscf` value `KVSCF_TOKEN`. Robust to launch
/// location (unlike a cwd/exe-dir `.env`) and to the boot-time HKCU `.DEFAULT` binding (via
/// `userreg` — otherwise an early-launched kvscf would silently run with the channel off).
#[cfg(windows)]
fn token_from_registry() -> Option<String> {
    crate::userreg::UserRoot::open()?
        .key()
        .open_subkey(r"Software\kenhia\kvscf")
        .ok()?
        .get_value::<String, _>("KVSCF_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
}

#[cfg(not(windows))]
fn token_from_registry() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOK: &str = "s3cret";

    #[test]
    fn focus_command_requires_matching_token() {
        let ok = format!(r#"{{"token":"{TOK}","id":"12345","maximize":true}}"#);
        match parse_command(&ok, TOK) {
            Some(Command::Focus { hwnd, maximize }) => {
                assert_eq!(hwnd, 12345);
                assert!(maximize);
            }
            other => panic!("expected Focus, got {:?}", other.is_none()),
        }
        // Wrong token → rejected.
        let bad = format!(r#"{{"token":"nope","id":"12345"}}"#);
        assert!(parse_command(&bad, TOK).is_none());
    }

    #[test]
    fn app_command_parses_key() {
        let msg = format!(r#"{{"token":"{TOK}","app":"everything"}}"#);
        match parse_command(&msg, TOK) {
            Some(Command::App { key }) => assert_eq!(key, "everything"),
            _ => panic!("expected App command"),
        }
        // `app` takes precedence over any `id` — an app tap is unambiguous.
        let both = format!(r#"{{"token":"{TOK}","app":"claude","id":"999"}}"#);
        assert!(matches!(
            parse_command(&both, TOK),
            Some(Command::App { .. })
        ));
    }

    #[test]
    fn non_numeric_id_routes_to_favorite_relaunch() {
        // A not-open favorite publishes its folder URI as `id`; it must not be read as an HWND.
        let uri = "vscode-remote://ssh-remote+kai/home/ken/src/kyac";
        let msg = format!(r#"{{"token":"{TOK}","id":"{uri}"}}"#);
        match parse_command(&msg, TOK) {
            Some(Command::Favorite { uri: got }) => assert_eq!(got, uri),
            _ => panic!("expected Favorite command"),
        }
        // A numeric id is still an HWND focus.
        let hwnd_msg = format!(r#"{{"token":"{TOK}","id":"98765"}}"#);
        assert!(matches!(
            parse_command(&hwnd_msg, TOK),
            Some(Command::Focus { hwnd: 98765, .. })
        ));
    }

    #[test]
    fn instances_json_flags_favorites_and_appends_not_open_ones() {
        let cfg = Config {
            redis_host: "h".into(),
            redis_port: 1,
            token: TOK.into(),
            this_host: "cleo".into(),
        };
        let inst = Instance {
            hwnd: 42,
            app: App::Insiders,
            workspace: "korg".into(),
            remote: Remote::Ssh("kai".into()),
            active_file: None,
            z_index: 0,
        };
        let favorited: HashSet<i64> = [42].into_iter().collect();
        let dimmed = vec![SetEntry {
            app: App::Insiders,
            uri: "vscode-remote://ssh-remote+kai/home/ken/src/kyac".into(),
            label: "kyac (kai)".into(),
        }];
        let v: serde_json::Value =
            serde_json::from_str(&build_instances_json(&cfg, &[inst], &favorited, &dimmed))
                .unwrap();
        let arr = v["instances"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // Open + starred.
        assert_eq!(arr[0]["id"], "42");
        assert_eq!(arr[0]["running"], true);
        assert_eq!(arr[0]["favorite"], true);
        // Not open: id is the URI, running false, label split back into workspace/host.
        assert_eq!(arr[1]["id"], dimmed[0].uri);
        assert_eq!(arr[1]["running"], false);
        assert_eq!(arr[1]["favorite"], true);
        assert_eq!(arr[1]["workspace"], "kyac");
        assert_eq!(arr[1]["remote_host"], "kai");
    }

    #[test]
    fn apps_json_carries_running_state_and_id() {
        let cfg = Config {
            redis_host: "h".into(),
            redis_port: 1,
            token: TOK.into(),
            this_host: "cleo".into(),
        };
        let apps = vec![
            AppEntry {
                key: "claude".into(),
                label: "Claude".into(),
                matcher: Default::default(),
                launch: kvscf_core::LaunchSpec {
                    kind: kvscf_core::LaunchKind::Aumid,
                    target: "X!App".into(),
                },
                order: 0,
                running: true,
                hwnd: Some(42),
            },
            AppEntry {
                key: "kindle".into(),
                label: "Kindle".into(),
                matcher: Default::default(),
                launch: kvscf_core::LaunchSpec {
                    kind: kvscf_core::LaunchKind::Exe,
                    target: "k.exe".into(),
                },
                order: 1,
                running: false,
                hwnd: None,
            },
        ];
        let v: serde_json::Value = serde_json::from_str(&build_apps_json(&cfg, &apps)).unwrap();
        assert_eq!(v["host"], "cleo");
        let arr = v["apps"].as_array().unwrap();
        assert_eq!(arr[0]["key"], "claude");
        assert_eq!(arr[0]["running"], true);
        assert_eq!(arr[0]["id"], "42"); // running → HWND as string
        assert_eq!(arr[1]["running"], false);
        assert!(arr[1]["id"].is_null()); // not running → no id
    }
}
