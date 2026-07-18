//! Foreground + focus a window by handle.
//!
//! The recipe locked in sprint 001: attach our input thread to the current foreground
//! window's thread, `SW_RESTORE` (un-minimize — `SetForegroundWindow` alone never does),
//! `SetForegroundWindow`, `BringWindowToTop`, then detach. Bare `SetForegroundWindow` was
//! proven to be a no-op you can't trust (returns `true`, nothing happens on screen).

use windows::Win32::Foundation::{FALSE, HWND, TRUE};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    ShowWindow, SW_RESTORE,
};

/// Restore + raise + focus the window with this handle. Returns whether
/// `SetForegroundWindow` reported success (note: not a guarantee of a visible result —
/// see sprint 001 findings; the `SW_RESTORE` + `AttachThreadInput` combo is what makes it
/// actually land).
pub fn focus(hwnd_raw: i64) -> bool {
    let hwnd = HWND(hwnd_raw as _);
    unsafe {
        let fg = GetForegroundWindow();
        let cur = GetCurrentThreadId();
        let mut fg_pid = 0u32;
        let fg_thread = GetWindowThreadProcessId(fg, Some(&mut fg_pid));

        let attached = AttachThreadInput(cur, fg_thread, TRUE).as_bool();
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let ok = SetForegroundWindow(hwnd).as_bool();
        let _ = BringWindowToTop(hwnd);
        if attached {
            let _ = AttachThreadInput(cur, fg_thread, FALSE);
        }
        ok
    }
}

/// The un-mitigated bare `SetForegroundWindow`, kept for characterization/testing only.
/// Do not use as the real focus path — it does not un-minimize and is subject to the
/// foreground lock. See sprint 001 focus test #1.
pub fn focus_unmitigated(hwnd_raw: i64) -> bool {
    let hwnd = HWND(hwnd_raw as _);
    unsafe { SetForegroundWindow(hwnd).as_bool() }
}
