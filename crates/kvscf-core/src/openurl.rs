//! Open a URL *in a chosen browser window* — the Launcher verb (sprint 016, korg kvscf #1133).
//!
//! The mechanism, proven by the sprint-015/016 spike (korg kvscf #1132, 8/8 trials on cleo):
//! foreground the target window, let it settle, then spawn `msedge.exe <url>`. Chromium opens
//! the tab in the last-active window of the matching profile, and foregrounding *is* what makes
//! it last-active. No new window is created, and it works from a minimized target.
//!
//! Deliberately NOT built: the synthesized `Ctrl+T` + type + Enter fallback. It was the
//! contingency if the above proved unreliable, and it didn't.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crate::focus::focus_with;

/// Pause between foregrounding the target and spawning Edge.
///
/// The spike measured **0ms working in every trial** — `focus_with`'s `AttachThreadInput`
/// recipe lands synchronously enough that Chromium's MRU is already current. 100ms is
/// deliberate insurance for a busier machine than the idle box the spike ran on; it is
/// invisible against the 3-5s of window-hunting this replaces.
pub const URL_SETTLE: Duration = Duration::from_millis(100);

/// Standard Edge install locations, in probe order.
const EDGE_PATHS: &[&str] = &[
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
];

/// Locate `msedge.exe`. `None` if Edge isn't installed where we expect.
pub fn edge_exe() -> Option<PathBuf> {
    EDGE_PATHS.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Foreground `hwnd` (when given), settle, then open `url` in Edge.
///
/// `hwnd: None` means "no window to target" — the cold-start case where no Edge window is open
/// at all. Edge is launched with the URL and picks its own window, which is the only sane
/// behaviour when there is nothing to choose between.
///
/// Returns whether Edge was successfully spawned. A `false` here is a real failure worth
/// surfacing: the tap did nothing visible.
pub fn open_url_in_window(hwnd: Option<i64>, url: &str) -> bool {
    use std::os::windows::process::CommandExt;
    /// Suppress the console flash on spawn (same flag `launch_app` uses).
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let Some(exe) = edge_exe() else {
        eprintln!("kvscf: cannot open {url} — msedge.exe not found in the standard install dirs");
        return false;
    };

    if let Some(h) = hwnd {
        focus_with(h, false);
        thread::sleep(URL_SETTLE);
    }

    match std::process::Command::new(exe)
        .arg(url)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        Ok(_) => true,
        Err(e) => {
            eprintln!("kvscf: failed to open {url}: {e}");
            false
        }
    }
}
