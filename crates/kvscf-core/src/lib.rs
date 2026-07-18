//! kvscf-core — enumerate open VS Code / Insiders windows, parse their titles into
//! structured [`Instance`]s, and foreground+focus a chosen window.
//!
//! The Win32 pieces ([`scan`], [`focus`]) are Windows-only; [`parse`] is pure and
//! portable so its unit tests run on any dev box (e.g. Linux).

pub mod parse;

pub use parse::{parse_edge_title, parse_title, EdgeTitle, ParsedTitle};

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod enumerate;
#[cfg(windows)]
mod focus;

#[cfg(windows)]
pub use app::{launch_and_focus, launch_app};
#[cfg(windows)]
pub use enumerate::{find_app_window, scan, scan_all, scan_edge};
#[cfg(windows)]
pub use focus::{close_window, focus, focus_unmitigated, focus_with};

// Portable stubs so the crate (and the parse tests) build on non-Windows hosts.
#[cfg(not(windows))]
pub fn scan() -> Vec<Instance> {
    Vec::new()
}
#[cfg(not(windows))]
pub fn scan_edge() -> Vec<EdgeWindow> {
    Vec::new()
}
#[cfg(not(windows))]
pub fn scan_all() -> (Vec<Instance>, Vec<EdgeWindow>) {
    (Vec::new(), Vec::new())
}
#[cfg(not(windows))]
pub fn find_app_window(_m: &AppMatcher) -> Option<i64> {
    None
}
#[cfg(not(windows))]
pub fn launch_app(_s: &LaunchSpec) -> std::io::Result<()> {
    Ok(())
}
#[cfg(not(windows))]
pub fn launch_and_focus(_s: &LaunchSpec, _m: &AppMatcher) {}
#[cfg(not(windows))]
pub fn focus(_hwnd: i64) -> bool {
    false
}
#[cfg(not(windows))]
pub fn focus_with(_hwnd: i64, _maximize: bool) -> bool {
    false
}
#[cfg(not(windows))]
pub fn close_window(_hwnd: i64) -> bool {
    false
}
#[cfg(not(windows))]
pub fn focus_unmitigated(_hwnd: i64) -> bool {
    false
}

/// Which VS Code build a window belongs to. Determined from the process image name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum App {
    Stable,
    Insiders,
    Exploration,
    Unknown,
}

impl App {
    /// Classify from a process image basename, e.g. `"Code - Insiders.exe"`.
    pub fn from_image(image: &str) -> App {
        let i = image.to_ascii_lowercase();
        if i.contains("insiders") {
            App::Insiders
        } else if i.contains("exploration") {
            App::Exploration
        } else if i.contains("code") {
            App::Stable
        } else {
            App::Unknown
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            App::Stable => "Code",
            App::Insiders => "Insiders",
            App::Exploration => "Exploration",
            App::Unknown => "?",
        }
    }
}

/// Where a window's workspace lives — local, or a remote of some kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remote {
    Local,
    Ssh(String),
    Wsl(String),
    DevContainer(String),
    Codespaces(String),
}

impl Remote {
    /// The host/distro/container name, if remote.
    pub fn host(&self) -> Option<&str> {
        match self {
            Remote::Local => None,
            Remote::Ssh(h) | Remote::Wsl(h) | Remote::DevContainer(h) | Remote::Codespaces(h) => {
                if h.is_empty() {
                    None
                } else {
                    Some(h)
                }
            }
        }
    }
}

/// One open VS Code window.
#[derive(Debug, Clone)]
pub struct Instance {
    /// Native window handle, widened to i64 for a stable, printable id.
    pub hwnd: i64,
    pub app: App,
    pub workspace: String,
    pub remote: Remote,
    pub active_file: Option<String>,
    /// Enumeration order = top-to-bottom Z-order; a cheap recency proxy.
    pub z_index: usize,
}

impl Instance {
    /// Display label per the target look: `workspace (host)`, or just `workspace` when local.
    pub fn label(&self) -> String {
        match self.remote.host() {
            Some(h) => format!("{} ({})", self.workspace, h),
            None => self.workspace.clone(),
        }
    }
}

/// One open Microsoft Edge window (WI #474). `named` distinguishes a user-set window name from a
/// tab-title-derived label.
#[derive(Debug, Clone)]
pub struct EdgeWindow {
    pub hwnd: i64,
    pub label: String,
    pub named: bool,
    pub tab_count: Option<u32>,
    pub z_index: usize,
}

/// How to recognize a configured app's window (Apps tab, sprint 007). A window matches when every
/// set field matches; at least one of `process` / `class` must be set (title alone is ambiguous).
#[derive(Debug, Clone, Default)]
pub struct AppMatcher {
    /// Process image basename, case-insensitive (e.g. `"claude.exe"`). May be unavailable for
    /// elevated processes — use `class` then.
    pub process: Option<String>,
    /// Exact window class (e.g. `"EVERYTHING"`) — needs no process access.
    pub class: Option<String>,
    /// Optional title substring to disambiguate multi-window apps (e.g. exclude "Friends").
    pub title_contains: Option<String>,
}

/// How to launch an app that isn't running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchKind {
    /// Spawn an executable path directly (Win32 apps).
    Exe,
    /// `explorer.exe shell:AppsFolder\<AUMID>` (Store apps, whose install paths are versioned).
    Aumid,
}

/// A launch target for [`launch_app`].
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub kind: LaunchKind,
    /// Exe path (for `Exe`) or AppUserModelID (for `Aumid`).
    pub target: String,
}
