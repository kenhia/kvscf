//! Remote channel to kdeskdash over the shared "claude-feed" Redis (WI #471, sprint 003).
//!
//! Contract (see `docs/kdeskdash-vscode-mode.md`):
//! - **Publish** the instance list to `kvscf:instances:<host>` (JSON String, TTL 10s,
//!   republished ~every app refresh). kdeskdash SCANs `kvscf:instances:*` and renders rows.
//!   Each row carries `running` + `favorite`; **favorites with no open window are appended as
//!   `running:false` rows whose `id` is the folder URI** rather than an HWND (sprint 008).
//! - **Publish** the configured apps to `kvscf:apps:<host>` (JSON, same TTL); each is
//!   `{key,label,running,id?}` — `id` is the HWND when running (sprint 007 Apps tab).
//! - **Publish** the Launcher buttons to `kvscf:launcher:<host>` (JSON, same TTL): the grid plus
//!   `{key,label,color,row,col,w,h}` per button (sprint 016). **No `url` and no `target`** — the
//!   dashboard draws buttons, it never learns where they go.
//! - **Subscribe** to `kvscf:focus:<host>` (pub/sub). The dashboard just echoes back the tapped
//!   row's id, and we route it: `{token,id:<int>,maximize}` foregrounds that HWND;
//!   `{token,id:<uri>}` relaunches that closed favorite (`crate::winset::launch_favorite`);
//!   `{token,app:<key>}` does **focus-if-running-else-launch** for a configured app
//!   (`crate::apps::activate`); `{token,button:<key>}` opens a Launcher button's URL in its
//!   preferred Edge window (`crate::launcher::activate`). Token gates all four.
//!
//! Two independent gates, and they protect different things. `KVSCF_TOKEN` is the **app-level**
//! gate on the focus command (the only action) and is mandatory — without it the channel stays
//! off rather than run open. The Redis password is the **transport** gate, and is optional in the
//! code: an endpoint without `requirepass` is legitimate, and no password means no AUTH rather
//! than no channel. Both of today's endpoints do require one — rpidash2:6380 since korg:2231
//! (2026-09-10), rpidash3:6380 since the kwork pairing.
//!
//! The transport gate arrived in sprint 018 (WI #1147) for the Launcher's kwork half: kwork
//! publishes to rpidash3 over the LAN, where the tailnet ACLs that cover every other homelab path
//! do not reach.
//!
//! Where each setting comes from:
//! - the token: `HKCU\Software\kenhia\kvscf`, then env / a `.env` file (untouched until korg
//!   WI 2479 decides whose secret it is);
//! - the endpoint (`KVSCF_REDIS_HOST` / `_PORT`) and the password's key name
//!   (`KVSCF_REDIS_AUTH_KEY`): env / `.env` only, non-secret;
//! - the password: [`crate::redis_auth`] — the environment, then
//!   `%ProgramData%\khomelab\secrets.env`, then the deprecated registry value — **resolved on
//!   every connect**, so a rotation needs no relaunch (sprint 022).
//!
//! This whole module is compiled out of the `kvscf-local` build (feature `remote` off).

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kvscf_core::{focus_with, EdgeWindow, Instance};

use std::collections::HashSet;

use crate::apps::{self, AppEntry};
use crate::launcher::{self, LauncherSet};
use crate::redis_auth;
use crate::winset::{self, SetEntry};

const DEFAULT_HOST: &str = "192.168.1.144"; // rpidash2 LAN IP (pinned, per handoff)
const DEFAULT_PORT: u16 = 6380;
const DEFAULT_HOST_NAME: &str = "cleo";
const INSTANCES_TTL_SECS: u64 = 10;
const RECONNECT_BACKOFF: Duration = Duration::from_secs(5);

/// Resolved connection + identity config. `None` from [`Config::load`] disables the channel.
///
/// Deliberately holds **no password**: that is resolved on every connect
/// ([`Config::resolve_password`]), so a rotation is picked up without a relaunch.
#[derive(Clone)]
struct Config {
    redis_host: String,
    redis_port: u16,
    /// Which key in the environment / per-host secrets file holds this endpoint's password
    /// (`KVSCF_REDIS_AUTH_KEY`, default `CLAUDE_REDISCLI_AUTH` for the default endpoint).
    auth_key: String,
    token: String,
    this_host: String,
}

/// Best-effort: pull KEY=VALUE from a .env in cwd or next to the exe (endpoint and auth-key
/// overrides, and the token fallback). Never overrides a variable already set.
fn load_dotenv() {
    dotenvy::dotenv().ok();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dotenvy::from_path(dir.join(".env")).ok();
        }
    }
}

impl Config {
    fn load() -> Option<Config> {
        load_dotenv();

        // Token: registry (preferred — HKCU\Software\kenhia\kvscf) → env/.env fallback. It works
        // regardless of where the exe is launched from (a pinned launch from C:\tools\bin has no
        // cwd/exe-dir .env). Mandatory: without it the channel stays off rather than run open.
        let token = registry_value("KVSCF_TOKEN")
            .or_else(|| std::env::var("KVSCF_TOKEN").ok())
            .filter(|t| !t.is_empty())?;

        Some(Config::with_token(token))
    }

    /// Everything but the token, from the environment (after [`load_dotenv`]).
    fn with_token(token: String) -> Config {
        Config {
            redis_host: env_or("KVSCF_REDIS_HOST", DEFAULT_HOST),
            redis_port: std::env::var("KVSCF_REDIS_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_PORT),
            auth_key: redis_auth::auth_key_in(&|k| std::env::var_os(k)),
            token,
            this_host: env_or("KVSCF_HOST_NAME", &computer_name()),
        }
    }

    /// The password for this endpoint, and which rung supplied it. Called on every connect.
    fn resolve_password(&self) -> redis_auth::Resolution {
        redis_auth::resolve_in(
            &self.auth_key,
            |k| std::env::var_os(k),
            || registry_value(redis_auth::LEGACY_NAME),
        )
    }

    /// How to connect — built as a struct rather than a `redis://` URL, deliberately.
    ///
    /// A URL would have to carry the password inline, and that costs twice: [`endpoint`] is
    /// printed to stderr when the channel comes up, and a password containing `@`, `:`, `/`, `#`
    /// or `%` needs percent-encoding that a hand-rolled `format!` gets wrong for exactly the
    /// characters a generated password is likely to contain — silently presenting the wrong
    /// credentials, or failing to parse and surfacing as an ordinary reconnect loop.
    ///
    /// `Client::open` takes `impl IntoConnectionInfo`, so none of that is necessary: the password
    /// goes straight into the struct and never becomes part of a string anything prints.
    ///
    /// [`endpoint`]: Config::endpoint
    fn connection_info(&self, password: Option<String>) -> redis::ConnectionInfo {
        redis::ConnectionInfo {
            addr: redis::ConnectionAddr::Tcp(self.redis_host.clone(), self.redis_port),
            redis: redis::RedisConnectionInfo {
                password,
                ..Default::default()
            },
        }
    }

    /// Resolve, announce the source (never the value), and open a client for this endpoint.
    fn client(&self) -> redis::RedisResult<redis::Client> {
        let auth = self.resolve_password();
        redis_auth::announce(&self.auth_key, &auth);
        redis::Client::open(self.connection_info(auth.password))
    }

    /// The endpoint for humans. **Display only, and deliberately password-free** — this string is
    /// logged.
    fn endpoint(&self) -> String {
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

    fn launcher_key(&self) -> String {
        format!("kvscf:launcher:{}", self.this_host)
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
    launcher: LauncherSet,
    favorited: HashSet<i64>,
    dimmed_favorites: Vec<SetEntry>,
}

/// The app-facing handle. Owns the sender that feeds the publisher thread.
pub struct Channel {
    tx: Sender<Snapshot>,
}

impl Channel {
    /// Start the publisher + subscriber threads. Returns `None` (channel disabled) if no
    /// `KVSCF_TOKEN` is configured.
    pub fn start() -> Option<Channel> {
        let cfg = Config::load()?;
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
        // The key *name* only. Where the password actually came from is announced by the first
        // connect (`redis_auth::announce`) — an endpoint that grew a `requirepass` while kvscf
        // found none fails as a silent reconnect loop, and that line is what distinguishes it from
        // an unreachable host.
        eprintln!(
            "kvscf: remote channel up — {} auth key {} (publish {}, focus {})",
            cfg.endpoint(),
            cfg.auth_key,
            cfg.instances_key(),
            cfg.focus_channel()
        );
        Some(Channel { tx })
    }

    /// Hand the latest window/app lists to the publisher thread (non-blocking). `favorited` is the
    /// set of open HWNDs that are starred; `dimmed_favorites` are favorites with no open window.
    pub fn publish(
        &self,
        items: &[Instance],
        edge: &[EdgeWindow],
        apps: &[AppEntry],
        launcher: &LauncherSet,
        favorited: &HashSet<i64>,
        dimmed_favorites: &[SetEntry],
    ) {
        let _ = self.tx.send(Snapshot {
            instances: items.to_vec(),
            edge: edge.to_vec(),
            apps: apps.to_vec(),
            launcher: launcher.clone(),
            favorited: favorited.clone(),
            dimmed_favorites: dimmed_favorites.to_vec(),
        });
    }
}

/// Publisher: SET the instance + edge lists with a TTL on every snapshot the app sends.
fn publisher_loop(cfg: Config, rx: Receiver<Snapshot>) {
    let inst_key = cfg.instances_key();
    let edge_key = cfg.edge_key();
    let apps_key = cfg.apps_key();
    let launcher_key = cfg.launcher_key();
    loop {
        let client = match cfg.client() {
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
                && set(&apps_key, build_apps_json(&cfg, &latest.apps), &mut con)
                && set(
                    &launcher_key,
                    build_launcher_json(&cfg, &latest.launcher),
                    &mut con,
                );
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
        let client = match cfg.client() {
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
                // Foreground the button's preferred Edge window, then open its URL there.
                // Blocks this thread for the settle delay, which is what it is for.
                Some(Command::Button { key }) => {
                    launcher::activate(&key);
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
                "remote": i.remote.kind(),
                "remote_host": i.remote.host(),
                "app": i.app.key(),
                "active_file": i.active_file,
                "z_index": i.z_index,
                "running": true,
                "favorite": favorited.contains(&i.hwnd),
                // WI #627. Published on both row kinds so a consumer can read one field path
                // unconditionally; it is always false on a dimmed favorite, which cannot be a dev
                // host (there is no folder URI to favorite).
                "ext_dev_host": i.ext_dev_host,
            })
        })
        .collect();

    instances.extend(dimmed.iter().map(|f| {
        serde_json::json!({
            "id": f.uri,                       // folder URI, not an HWND — it has no window
            "label": f.label,
            "workspace": f.workspace,
            "remote": if f.host.is_some() { "ssh" } else { "local" }, // best-effort from the URI
            "remote_host": f.host,
            "app": f.app.key(),
            "active_file": serde_json::Value::Null,
            "z_index": serde_json::Value::Null,
            "running": false,
            "favorite": true,
            "ext_dev_host": false,
        })
    }));

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

/// Build the Launcher JSON payload (sprint 016). The grid is published rather than assumed on
/// both sides, so the editor's idea of the layout and the panel's can never drift apart.
///
/// **`url` and `target` are deliberately not here.** The dashboard needs a label, a color and a
/// rectangle; it does not need to know where a button goes, and on kwork those URLs are an
/// employer's business. The command echoes back `key` and this side resolves the rest.
fn build_launcher_json(cfg: &Config, set: &LauncherSet) -> String {
    let items: Vec<serde_json::Value> = set
        .buttons
        .iter()
        .map(|b| {
            serde_json::json!({
                "key": b.key,
                "label": b.label,
                "color": b.color,
                "row": b.row,
                "col": b.col,
                "w": b.w,
                "h": b.h,
            })
        })
        .collect();

    serde_json::json!({
        "host": cfg.this_host,
        "ts": now_secs(),
        "grid": { "rows": set.grid.rows, "cols": set.grid.cols },
        "buttons": items,
    })
    .to_string()
}

/// A parsed, authenticated command off the focus channel.
enum Command {
    /// Foreground an explicit HWND (VS Code / Edge rows).
    Focus { hwnd: i64, maximize: bool },
    /// Focus-if-running-else-launch a configured app by key (Apps tab).
    App { key: String },
    /// Open a Launcher button's URL in its preferred Edge window (sprint 016).
    Button { key: String },
    /// Relaunch a not-open Code favorite by folder URI (sprint 008).
    Favorite { uri: String },
}

/// Parse + authenticate a command. Returns `None` unless the token matches.
///
/// Routing, so kdeskdash can stay uniform (it just echoes back the tapped row's `id`):
/// - `button` present → [`Command::Button`] (checked first; a Launcher tap is unambiguous).
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
    if let Some(button) = v.get("button").and_then(|b| b.as_str()) {
        return Some(Command::Button {
            key: button.to_string(),
        });
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

/// A value under `HKCU\Software\kenhia\kvscf`. Robust to launch location (unlike a cwd/exe-dir
/// `.env`) and to the boot-time HKCU `.DEFAULT` binding (via `userreg` — otherwise an
/// early-launched kvscf would silently run with the channel off).
///
/// Used for `KVSCF_TOKEN` (preferred source) and `KVSCF_REDIS_PASSWORD` (deprecated since sprint
/// 022: the per-host secrets file comes first, see [`crate::redis_auth`]).
#[cfg(windows)]
fn registry_value(name: &str) -> Option<String> {
    crate::userreg::UserRoot::open()?
        .key()
        .open_subkey(r"Software\kenhia\kvscf")
        .ok()?
        .get_value::<String, _>(name)
        .ok()
        .filter(|v| !v.is_empty())
}

#[cfg(not(windows))]
fn registry_value(_name: &str) -> Option<String> {
    None
}

/// `--probe-redis-auth` (sprint 022): name the endpoint, the key and the rung that answers — never
/// the value — then AUTH, write a short-lived key and delete it. Returns `true` only if the write
/// landed.
///
/// A write, not `PING` or a bare connect: an endpoint that answers unauthenticated commands, or a
/// client that sends no AUTH at all, would pass either. Pair every pass with a wrong-password
/// control (point `ProgramData` at a directory whose `secrets.env` holds a wrong value); the
/// control must print `REFUSED`.
///
/// The probe key is `kvscf:probe:<host>`, outside every pattern kdeskdash scans, TTL 10s and
/// deleted immediately.
pub fn probe_redis_auth() -> bool {
    load_dotenv();
    let cfg = Config::with_token(String::new());
    let auth = cfg.resolve_password();
    println!("endpoint: {}", cfg.endpoint());
    println!("auth key: {}", cfg.auth_key);
    println!("source:   {}", auth.source.describe());

    let fail = |stage: &str, e: redis::RedisError| {
        // RedisError carries the server's reply (e.g. WRONGPASS), never the credential sent.
        println!("{stage}: REFUSED - {e}");
        false
    };
    let client = match redis::Client::open(cfg.connection_info(auth.password)) {
        Ok(c) => c,
        Err(e) => return fail("client", e),
    };
    let mut con = match client.get_connection_with_timeout(Duration::from_secs(5)) {
        Ok(c) => c,
        Err(e) => return fail("connect", e),
    };
    let key = format!("kvscf:probe:{}", cfg.this_host);
    if let Err(e) = redis::cmd("SET")
        .arg(&key)
        .arg(now_secs())
        .arg("EX")
        .arg(10)
        .query::<()>(&mut con)
    {
        return fail("write", e);
    }
    match redis::cmd("DEL").arg(&key).query::<i64>(&mut con) {
        Ok(1) => {
            println!("write:    ok ({key} set and deleted)");
            true
        }
        Ok(n) => {
            println!("write:    FAILED - DEL removed {n} keys, expected 1");
            false
        }
        Err(e) => fail("delete", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kvscf_core::{App, Remote};

    const TOK: &str = "s3cret";

    fn kwork_cfg() -> Config {
        Config {
            redis_host: "192.168.1.73".into(),
            redis_port: 6379,
            auth_key: "KVSCF_REDISCLI_AUTH".into(),
            token: TOK.into(),
            this_host: "kwork".into(),
        }
    }

    #[test]
    fn no_password_found_connects_unauthenticated() {
        // An endpoint without `requirepass` is legitimate. Absent password must mean "no AUTH",
        // never "no channel" — unlike the token.
        let info = kwork_cfg().connection_info(None);
        assert_eq!(info.redis.password, None);
        assert_eq!(
            info.addr,
            redis::ConnectionAddr::Tcp("192.168.1.73".into(), 6379)
        );
    }

    #[test]
    fn a_resolved_password_reaches_the_connection() {
        let info = kwork_cfg().connection_info(Some("hunter2".into()));
        assert_eq!(info.redis.password.as_deref(), Some("hunter2"));
    }

    #[test]
    fn the_logged_endpoint_never_carries_the_password() {
        // `endpoint()` is printed to stderr when the channel comes up, and the config no longer
        // even holds a password. This is the regression test for the reason a
        // `redis://:pw@host` URL was not used.
        let cfg = kwork_cfg();
        let _ = cfg.connection_info(Some("hunter2".into()));
        let shown = cfg.endpoint();
        assert!(!shown.contains("hunter2"), "password leaked into {shown:?}");
        assert_eq!(shown, "redis://192.168.1.73:6379");
    }

    #[test]
    fn a_url_hostile_password_survives_intact() {
        // Every character here is structural in a URL — `@` ends the userinfo, `:` splits it,
        // `/` ends the authority, `#` starts a fragment, `%` opens an escape. A hand-built
        // `redis://:{pw}@host` would present the wrong credentials or fail to parse; the struct
        // has no such failure mode, which is the whole argument for it.
        let pw = "p@ss:w/rd#%";
        let info = kwork_cfg().connection_info(Some(pw.into()));
        assert_eq!(info.redis.password.as_deref(), Some(pw));
    }

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
        let bad = r#"{"token":"nope","id":"12345"}"#;
        assert!(parse_command(bad, TOK).is_none());
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
    fn button_command_parses_key_and_outranks_the_others() {
        let msg = format!(r#"{{"token":"{TOK}","button":"ado-pipelines"}}"#);
        match parse_command(&msg, TOK) {
            Some(Command::Button { key }) => assert_eq!(key, "ado-pipelines"),
            _ => panic!("expected Button command"),
        }
        // Precedence is button > app > id: a Launcher tap is unambiguous, so it wins outright
        // rather than depending on which field the dashboard happened to fill.
        let all = format!(r#"{{"token":"{TOK}","button":"b","app":"claude","id":"999"}}"#);
        assert!(matches!(
            parse_command(&all, TOK),
            Some(Command::Button { .. })
        ));
        // Still token-gated, like every other verb.
        let bad = r#"{"token":"nope","button":"b"}"#;
        assert!(parse_command(bad, TOK).is_none());
    }

    #[test]
    fn launcher_json_publishes_the_grid_but_never_the_url() {
        let cfg = Config {
            redis_host: "h".into(),
            redis_port: 1,
            auth_key: redis_auth::DEFAULT_AUTH_KEY.into(),
            token: TOK.into(),
            this_host: "kwork".into(),
        };
        let set = LauncherSet {
            grid: crate::launcher::Grid { rows: 3, cols: 9 },
            buttons: vec![crate::launcher::LauncherButton {
                key: "ado-pipelines".into(),
                label: "Pipelines".into(),
                url: "https://dev.azure.com/secret/thing".into(),
                target: crate::launcher::Target::Named("GitHub".into()),
                color: "#2ec4c4".into(),
                row: 0,
                col: 0,
                w: 2,
                h: 1,
            }],
        };
        let raw = build_launcher_json(&cfg, &set);

        // The whole point of keying the command: work URLs never leave the work box.
        assert!(!raw.contains("dev.azure.com"));
        assert!(!raw.contains("GitHub"));

        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["host"], "kwork");
        assert_eq!(v["grid"]["rows"], 3);
        assert_eq!(v["grid"]["cols"], 9);
        let b = &v["buttons"].as_array().unwrap()[0];
        assert_eq!(b["key"], "ado-pipelines");
        assert_eq!(b["label"], "Pipelines");
        assert_eq!(b["color"], "#2ec4c4");
        assert_eq!(b["w"], 2);
        assert!(b.get("url").is_none());
        assert!(b.get("target").is_none());
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
            auth_key: redis_auth::DEFAULT_AUTH_KEY.into(),
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
            ext_dev_host: false,
        };
        let favorited: HashSet<i64> = [42].into_iter().collect();
        let dimmed = vec![SetEntry {
            app: App::Insiders,
            uri: "vscode-remote://ssh-remote+kai/home/ken/src/kyac".into(),
            label: "kyac (kai)".into(),
            workspace: "kyac".into(),
            host: Some("kai".into()),
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
        // Present on both row kinds so a consumer reads one field path (WI #627).
        assert_eq!(arr[0]["ext_dev_host"], false);
        assert_eq!(arr[1]["ext_dev_host"], false);
    }

    /// A dev host on the wire (WI #627), built from the real title rather than a hand-set flag, so
    /// this covers the parse and the payload together.
    ///
    /// It also pins the **bug** this fixed: the same window used to publish
    /// `active_file: "Insiders"` — VS Code's own edition name, split off the title at the ` - ` and
    /// handed to kdeskdash as the name of an open file. Nothing downstream could tell it was
    /// invented, so the regression has to be caught here.
    #[test]
    fn a_dev_host_is_flagged_on_the_wire_and_claims_no_open_file() {
        let cfg = Config {
            redis_host: "h".into(),
            redis_port: 1,
            auth_key: redis_auth::DEFAULT_AUTH_KEY.into(),
            token: TOK.into(),
            this_host: "cleo".into(),
        };
        let parsed =
            kvscf_core::parse_title("[Extension Development Host] Visual Studio Code - Insiders")
                .expect("the live dev-host title must parse");
        let inst = Instance {
            hwnd: 7,
            app: App::Insiders,
            workspace: parsed.workspace,
            remote: parsed.remote,
            active_file: parsed.active_file,
            z_index: 0,
            ext_dev_host: parsed.ext_dev_host,
        };
        let v: serde_json::Value =
            serde_json::from_str(&build_instances_json(&cfg, &[inst], &HashSet::new(), &[]))
                .unwrap();
        let row = &v["instances"][0];
        assert_eq!(row["ext_dev_host"], true);
        assert_eq!(row["workspace"], "Extension Development Host");
        assert_eq!(row["label"], "Extension Development Host");
        // The fabrication: this was "Insiders" before sprint 021.
        assert!(
            row["active_file"].is_null(),
            "a dev host with no folder open must not publish an active file, got {}",
            row["active_file"]
        );
    }

    #[test]
    fn apps_json_carries_running_state_and_id() {
        let cfg = Config {
            redis_host: "h".into(),
            redis_port: 1,
            auth_key: redis_auth::DEFAULT_AUTH_KEY.into(),
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
