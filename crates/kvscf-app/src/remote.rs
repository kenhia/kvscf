//! Remote channel to kdeskdash over the shared "claude-feed" Redis (WI #471, sprint 003).
//!
//! Contract (see `docs/kdeskdash-vscode-mode.md`):
//! - **Publish** the instance list to `kvscf:instances:<host>` (JSON String, TTL 10s,
//!   republished ~every app refresh). kdeskdash SCANs `kvscf:instances:*` and renders rows.
//! - **Subscribe** to `kvscf:focus:<host>` (pub/sub); on `{token,id,maximize}` with a valid
//!   token, foreground that HWND (`kvscf_core::focus_with`).
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

    fn focus_channel(&self) -> String {
        format!("kvscf:focus:{}", self.this_host)
    }
}

/// One published snapshot: the VS Code instances and the Edge windows.
struct Snapshot {
    instances: Vec<Instance>,
    edge: Vec<EdgeWindow>,
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

    /// Hand the latest window lists to the publisher thread (non-blocking).
    pub fn publish(&self, items: &[Instance], edge: &[EdgeWindow]) {
        let _ = self.tx.send(Snapshot {
            instances: items.to_vec(),
            edge: edge.to_vec(),
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
                build_instances_json(&cfg, &latest.instances),
                &mut con,
            ) && set(&edge_key, build_edge_json(&cfg, &latest.edge), &mut con);
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
            if let Some((hwnd, maximize)) = parse_focus(&payload, &cfg.token) {
                // Background-thread foreground — the hostile case the 001 recipe was built for.
                focus_with(hwnd, maximize);
            }
        }
        thread::sleep(RECONNECT_BACKOFF);
    }
}

/// Build the instance-list JSON payload.
fn build_instances_json(cfg: &Config, items: &[Instance]) -> String {
    let instances: Vec<serde_json::Value> = items
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
            })
        })
        .collect();

    serde_json::json!({
        "host": cfg.this_host,
        "ts": now_secs(),
        "instances": instances,
    })
    .to_string()
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

/// Parse + authenticate a focus command. Returns `(hwnd, maximize)` only if the token matches.
fn parse_focus(payload: &str, expected_token: &str) -> Option<(i64, bool)> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    let token = v.get("token")?.as_str()?;
    if token != expected_token {
        return None;
    }
    let hwnd = v.get("id")?.as_str()?.parse::<i64>().ok()?;
    let maximize = v.get("maximize").and_then(|m| m.as_bool()).unwrap_or(false);
    Some((hwnd, maximize))
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
/// location (unlike a cwd/exe-dir `.env`).
#[cfg(windows)]
fn token_from_registry() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
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
