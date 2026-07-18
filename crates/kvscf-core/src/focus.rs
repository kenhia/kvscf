//! Foreground + focus a window by handle.
//!
//! The recipe locked in sprint 001: attach our input thread to the current foreground
//! window's thread, un-minimize *only if needed*, `SetForegroundWindow`, `BringWindowToTop`,
//! then detach. Bare `SetForegroundWindow` was proven to be a no-op you can't trust (returns
//! `true`, nothing happens on screen).
//!
//! WI #465: `SW_RESTORE` un-maximizes an already-maximized window, so we restore *only* when
//! the target is minimized (`IsIconic`). [`focus_with`] optionally maximizes instead.

use windows::Win32::Foundation::{FALSE, HWND, LPARAM, TRUE, WPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, PostMessageW,
    SetForegroundWindow, ShowWindow, SW_MAXIMIZE, SW_RESTORE, WM_CLOSE,
};

/// Restore-if-needed + raise + focus the window with this handle. Preserves an existing
/// maximized state (only un-minimizes). Returns whether `SetForegroundWindow` reported
/// success (not a guarantee of a visible result — see sprint 001 findings; the
/// `AttachThreadInput` combo is what makes it actually land).
pub fn focus(hwnd_raw: i64) -> bool {
    focus_with(hwnd_raw, false)
}

/// Like [`focus`], but when `maximize` is true the target is maximized (`SW_MAXIMIZE`)
/// regardless of its prior state. When false, an existing maximized/normal state is
/// preserved and a minimized window is restored. (WI #465.)
pub fn focus_with(hwnd_raw: i64, maximize: bool) -> bool {
    let hwnd = HWND(hwnd_raw as _);
    unsafe {
        let fg = GetForegroundWindow();
        let cur = GetCurrentThreadId();
        let mut fg_pid = 0u32;
        let fg_thread = GetWindowThreadProcessId(fg, Some(&mut fg_pid));

        let attached = AttachThreadInput(cur, fg_thread, TRUE).as_bool();
        if maximize {
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        } else if IsIconic(hwnd).as_bool() {
            // Only un-minimize; do NOT SW_RESTORE a maximized window (WI #465).
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let ok = SetForegroundWindow(hwnd).as_bool();
        let _ = BringWindowToTop(hwnd);
        if attached {
            let _ = AttachThreadInput(cur, fg_thread, FALSE);
        }
        ok
    }
}

/// Ask a window to close, like clicking its ✕ — posts `WM_CLOSE`. A normal close, so VS Code
/// still prompts on unsaved changes (the Update Assist flow assumes saved). Returns whether the
/// message was posted (not whether the window actually closed).
pub fn close_window(hwnd_raw: i64) -> bool {
    let hwnd = HWND(hwnd_raw as _);
    unsafe { PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok() }
}

/// The un-mitigated bare `SetForegroundWindow`, kept for characterization/testing only.
/// Do not use as the real focus path — it does not un-minimize and is subject to the
/// foreground lock. See sprint 001 focus test #1.
pub fn focus_unmitigated(hwnd_raw: i64) -> bool {
    let hwnd = HWND(hwnd_raw as _);
    unsafe { SetForegroundWindow(hwnd).as_bool() }
}
