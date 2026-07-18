//! Launch configured apps (Apps tab, sprint 007). Exe path (Win32) or
//! `explorer.exe shell:AppsFolder\<AUMID>` (Store apps, whose install paths are versioned).
//!
//! After launching we poll for the app's window and foreground it — a launched app doesn't
//! reliably come to the front on its own (Kindle opened *behind* another window in testing).

use std::thread;
use std::time::Duration;

use crate::enumerate::find_app_window;
use crate::focus::focus;
use crate::{AppMatcher, LaunchKind, LaunchSpec};

const LAUNCH_FOCUS_TRIES: u32 = 40; // ~20s at 500ms — Store/slow apps (Kindle) take a while
const LAUNCH_FOCUS_INTERVAL: Duration = Duration::from_millis(500);

/// Launch an app (returns immediately; detached, no console).
pub fn launch_app(spec: &LaunchSpec) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    match spec.kind {
        LaunchKind::Exe => std::process::Command::new(&spec.target)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ()),
        LaunchKind::Aumid => std::process::Command::new("explorer.exe")
            .arg(format!("shell:AppsFolder\\{}", spec.target))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ()),
    }
}

/// Launch on a background thread, then poll for the app's window (up to ~20s) and foreground it.
pub fn launch_and_focus(spec: &LaunchSpec, matcher: &AppMatcher) {
    let spec = spec.clone();
    let matcher = matcher.clone();
    thread::spawn(move || {
        if launch_app(&spec).is_err() {
            return;
        }
        for _ in 0..LAUNCH_FOCUS_TRIES {
            thread::sleep(LAUNCH_FOCUS_INTERVAL);
            if let Some(hwnd) = find_app_window(&matcher) {
                focus(hwnd);
                return;
            }
        }
    });
}
