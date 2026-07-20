//! AppBar "docked" mode (WI #468) — reserve the **primary monitor's left edge** so maximized
//! windows don't cover kvscf, exactly like the taskbar. Ken only docks on the primary monitor;
//! elsewhere the app stays in floating mode, so this always targets the primary left edge.
//!
//! Minimal-but-correct: register (`ABM_NEW`) → `ABM_QUERYPOS`/`ABM_SETPOS` to reserve the band →
//! move our window into the granted rect. We do not subclass the winit HWND to handle
//! `ABN_POSCHANGED`; instead the app re-asserts [`set_pos`] on a ~1s timer while docked, which
//! keeps it correct after taskbar/resolution changes. Always `remove` on exit.
//!
//! That same timer drives [`fullscreen_app_present`] (WI #481): the taskbar drops behind a
//! fullscreen app, and a docked kvscf should too. Windows *does* have a notification for this
//! (`ABN_FULLSCREENAPP`, delivered to the `uCallbackMessage` we register below) — but reading it
//! would mean subclassing winit's HWND, which this module deliberately avoids, and its behavior
//! for modern *borderless-windowed* fullscreen is unverified. Polling the foreground window on the
//! tick we already run is both simpler and mode-agnostic.

#[cfg(windows)]
mod imp {
    use std::mem::size_of;

    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::Shell::{
        SHAppBarMessage, ABE_LEFT, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetSystemMetrics, GetWindowRect, GetWindowTextW,
        SetWindowPos, HWND_TOP, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOZORDER,
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

    /// Is a **fullscreen app** holding the foreground on the monitor we're docked to?
    ///
    /// True for exclusive fullscreen, borderless-windowed fullscreen, and F11 alike — all three
    /// produce a foreground window covering the monitor's **full** bounds (`rcMonitor`). A merely
    /// *maximized* window does **not** qualify: it respects `rcWork`, which already excludes our
    /// reserved band. That's exactly the line we want, and it's why this is a rect test rather
    /// than a style/caption test.
    ///
    /// Returns false for our own window (so focusing kvscf restores it) and for the desktop shell
    /// (`Progman`/`WorkerW` cover the monitor but aren't fullscreen apps), and only reacts to the
    /// monitor we're on, so a game fullscreened on a second display leaves the dock alone.
    pub fn fullscreen_app_present(our_hwnd_raw: isize) -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.0.is_null() || fg.0 as isize == our_hwnd_raw {
                return false;
            }

            let mut buf = [0u16; 64];
            let n = GetClassNameW(fg, &mut buf);
            let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            if class == "Progman" || class == "WorkerW" {
                return false;
            }

            // Only a fullscreen app on *our* monitor should push us down.
            let our_mon = MonitorFromWindow(HWND(our_hwnd_raw as _), MONITOR_DEFAULTTONEAREST);
            let fg_mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
            if our_mon.0 != fg_mon.0 {
                return false;
            }

            let mut mi = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(fg_mon, &mut mi).as_bool() {
                return false;
            }

            let mut r = RECT::default();
            if GetWindowRect(fg, &mut r).is_err() {
                return false;
            }

            // Covers the whole monitor (>= so an overshooting window still counts).
            r.left <= mi.rcMonitor.left
                && r.top <= mi.rcMonitor.top
                && r.right >= mi.rcMonitor.right
                && r.bottom >= mi.rcMonitor.bottom
        }
    }

    /// Foreground-window diagnostics for the `--probe-fullscreen` verification probe: enough to
    /// see *why* [`fullscreen_app_present`] decided what it did (class, window rect vs monitor
    /// rect), which is how the borderless-vs-exclusive question gets settled empirically.
    pub fn describe_foreground() -> String {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.0.is_null() {
                return "(no foreground window)".to_string();
            }
            let mut cbuf = [0u16; 64];
            let cn = GetClassNameW(fg, &mut cbuf);
            let class = String::from_utf16_lossy(&cbuf[..cn.max(0) as usize]);

            let mut tbuf = [0u16; 128];
            let tn = GetWindowTextW(fg, &mut tbuf);
            let title = String::from_utf16_lossy(&tbuf[..tn.max(0) as usize]);

            let mut r = RECT::default();
            let _ = GetWindowRect(fg, &mut r);

            let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(mon, &mut mi);

            format!(
                "class={:<22} win=({},{})-({},{})  mon=({},{})-({},{})  {}",
                class,
                r.left,
                r.top,
                r.right,
                r.bottom,
                mi.rcMonitor.left,
                mi.rcMonitor.top,
                mi.rcMonitor.right,
                mi.rcMonitor.bottom,
                title
            )
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn register(_hwnd_raw: isize) {}
    pub fn set_pos(_hwnd_raw: isize, _width_px: i32) {}
    pub fn remove(_hwnd_raw: isize) {}
    pub fn fullscreen_app_present(_our_hwnd_raw: isize) -> bool {
        false
    }
    pub fn describe_foreground() -> String {
        "(windows only)".to_string()
    }
}

pub use imp::{describe_foreground, fullscreen_app_present, register, remove, set_pos};
