//! AppBar "docked" mode (WI #468) — reserve the **primary monitor's left edge** so maximized
//! windows don't cover kvscf, exactly like the taskbar. Ken only docks on the primary monitor;
//! elsewhere the app stays in floating mode, so this always targets the primary left edge.
//!
//! Minimal-but-correct: register (`ABM_NEW`) → `ABM_QUERYPOS`/`ABM_SETPOS` to reserve the band →
//! move our window into the granted rect. We do not subclass the winit HWND to handle
//! `ABN_POSCHANGED`; instead the app re-asserts [`set_pos`] on a ~1s timer while docked, which
//! keeps it correct after taskbar/resolution changes. Always `remove` on exit.

#[cfg(windows)]
mod imp {
    use std::mem::size_of;

    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::Shell::{
        SHAppBarMessage, ABE_LEFT, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SetWindowPos, HWND_TOP, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOZORDER,
    };

    // Notification message Windows would post to our HWND (WM_USER-based). We don't process
    // it (winit owns the message loop); the ~1s re-assert covers repositioning instead.
    const CALLBACK_MSG: u32 = 0x0400 + 0x101;

    fn base(hwnd: HWND) -> APPBARDATA {
        APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            ..Default::default()
        }
    }

    pub fn register(hwnd_raw: isize) {
        let hwnd = HWND(hwnd_raw as _);
        let mut abd = base(hwnd);
        abd.uCallbackMessage = CALLBACK_MSG;
        unsafe {
            SHAppBarMessage(ABM_NEW, &mut abd);
        }
    }

    pub fn set_pos(hwnd_raw: isize, width_px: i32) {
        let hwnd = HWND(hwnd_raw as _);
        let width_px = width_px.max(80);
        unsafe {
            // Primary monitor is at origin (0,0); reserve a left band full-height.
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            let mut abd = base(hwnd);
            abd.uEdge = ABE_LEFT;
            abd.rc = RECT {
                left: 0,
                top: 0,
                right: width_px,
                bottom: screen_h,
            };
            // QUERYPOS lets Windows trim the band for the taskbar / other appbars…
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            // …then pin our width against the left edge and commit.
            abd.rc.left = 0;
            abd.rc.right = width_px;
            SHAppBarMessage(ABM_SETPOS, &mut abd);

            let r = abd.rc;
            let _ = SetWindowPos(
                hwnd,
                HWND_TOP,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    pub fn remove(hwnd_raw: isize) {
        let hwnd = HWND(hwnd_raw as _);
        let mut abd = base(hwnd);
        unsafe {
            SHAppBarMessage(ABM_REMOVE, &mut abd);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn register(_hwnd_raw: isize) {}
    pub fn set_pos(_hwnd_raw: isize, _width_px: i32) {}
    pub fn remove(_hwnd_raw: isize) {}
}

pub use imp::{register, remove, set_pos};
