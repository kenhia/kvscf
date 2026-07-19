//! Apps tab (sprint 007) — arbitrary apps Ken switches to a lot: **focus if running, launch if
//! not**. Unlike Code/Edge (auto-discovered), apps are **configured** in the registry under
//! `HKCU\Software\kenhia\kvscf\apps\<key>` (one subkey per app), populated by the
//! `kvscf-add-app` agent skill.
//!
//! Each configured app is matched against open windows (by process image and/or window class —
//! never title alone) to tell whether it's running; a non-running app has no HWND and is launched
//! by exe path (Win32) or `explorer shell:AppsFolder\<AUMID>` (Store apps).

use kvscf_core::{focus_with, launch_and_focus, resolve_apps, AppMatcher, LaunchSpec};

/// One configured app, plus its resolved running state for this refresh.
#[derive(Debug, Clone)]
pub struct AppEntry {
    /// Registry subkey — stable id used by the remote `{app:<key>}` command.
    pub key: String,
    pub label: String,
    pub matcher: AppMatcher,
    pub launch: LaunchSpec,
    /// Sort index (missing → sorts last). Applied at load time; also published so the dashboard
    /// can match our order (only read by the `remote` build).
    #[cfg_attr(not(feature = "remote"), allow(dead_code))]
    pub order: u32,
    /// Resolved this refresh: is a matching window open?
    pub running: bool,
    /// The topmost matching window, when running.
    pub hwnd: Option<i64>,
}

/// Load the configured apps and resolve their running state in one enumeration pass. Sorted by
/// `order` then label. Reloaded each refresh so apps added by the skill appear without a restart.
pub fn scan() -> Vec<AppEntry> {
    let mut cfgs = config::load();
    cfgs.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    let matchers: Vec<AppMatcher> = cfgs.iter().map(|c| c.matcher.clone()).collect();
    let hwnds = resolve_apps(&matchers);
    cfgs.into_iter()
        .zip(hwnds)
        .map(|(c, hwnd)| AppEntry {
            key: c.key,
            label: c.label,
            matcher: c.matcher,
            launch: c.launch,
            order: c.order,
            running: hwnd.is_some(),
            hwnd,
        })
        .collect()
}

/// Activate a configured app by key: **focus it if running, launch it if not** — the action
/// behind a dashboard tap over the remote channel (WI, sprint 007). Returns `false` if no app
/// with that key is configured. `focus_with(false)` — a remote tap doesn't maximize.
#[allow(dead_code)] // only called from the `remote` build
pub fn activate(key: &str) -> bool {
    let Some(entry) = scan().into_iter().find(|e| e.key == key) else {
        return false;
    };
    match entry.hwnd {
        Some(hwnd) => {
            focus_with(hwnd, false);
        }
        None => launch_and_focus(&entry.launch, &entry.matcher),
    }
    true
}

/// The static (pre-resolution) config for one app.
pub struct AppConfig {
    pub key: String,
    pub label: String,
    pub matcher: AppMatcher,
    pub launch: LaunchSpec,
    pub order: u32,
}

#[cfg(windows)]
mod config {
    use super::AppConfig;
    use kvscf_core::{AppMatcher, LaunchKind, LaunchSpec};
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const PATH: &str = r"Software\kenhia\kvscf\apps";

    /// Parse a `launch_kind` registry value into a [`LaunchKind`].
    fn parse_launch_kind(s: &str) -> Option<LaunchKind> {
        match s.to_ascii_lowercase().as_str() {
            "exe" => Some(LaunchKind::Exe),
            "aumid" => Some(LaunchKind::Aumid),
            _ => None,
        }
    }

    /// Read every `…\kvscf\apps\<key>` subkey into an [`AppConfig`]. Entries missing the fields a
    /// row can't work without (no matcher, or an unusable launch spec) are skipped with a warning.
    pub fn load() -> Vec<AppConfig> {
        let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(PATH) else {
            return Vec::new(); // no apps configured yet
        };
        let mut out = Vec::new();
        for key in root.enum_keys().flatten() {
            let Ok(sub) = root.open_subkey(&key) else {
                continue;
            };
            let get = |name: &str| sub.get_value::<String, _>(name).ok().filter(|v| !v.is_empty());

            let label = get("label").unwrap_or_else(|| key.clone());
            let matcher = AppMatcher {
                process: get("process"),
                class: get("class"),
                title_contains: get("match"),
            };
            if matcher.process.is_none() && matcher.class.is_none() {
                eprintln!("kvscf: app '{key}' has no process/class matcher — skipping");
                continue;
            }
            let (Some(kind), Some(target)) =
                (get("launch_kind").as_deref().and_then(parse_launch_kind), get("launch"))
            else {
                eprintln!("kvscf: app '{key}' has no valid launch_kind/launch — skipping");
                continue;
            };
            let order = sub.get_value::<u32, _>("order").unwrap_or(u32::MAX);

            out.push(AppConfig {
                key,
                label,
                matcher,
                launch: LaunchSpec { kind, target },
                order,
            });
        }
        out
    }
}

#[cfg(not(windows))]
mod config {
    use super::AppConfig;
    pub fn load() -> Vec<AppConfig> {
        Vec::new()
    }
}
