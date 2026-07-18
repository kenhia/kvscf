//! kvscf-core — enumerate open VS Code / Insiders windows, parse their titles into
//! structured [`Instance`]s, and foreground+focus a chosen window.
//!
//! The Win32 pieces ([`scan`], [`focus`]) are Windows-only; [`parse`] is pure and
//! portable so its unit tests run on any dev box (e.g. Linux).

pub mod parse;

pub use parse::{parse_edge_title, parse_title, EdgeTitle, ParsedTitle};

#[cfg(windows)]
mod enumerate;
#[cfg(windows)]
mod focus;

#[cfg(windows)]
pub use enumerate::{scan, scan_all, scan_edge};
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
