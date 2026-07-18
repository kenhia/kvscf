//! Win32 enumeration of open VS Code / Insiders windows.
//!
//! `Get-Process` is insufficient (all VS Code windows share one process, one
//! `MainWindowHandle`), so we walk every top-level window with `EnumWindows`, keep the
//! visible + titled ones whose process image is a VS Code build, and parse each title.

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, TRUE};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};

use crate::parse::parse_title;
use crate::{App, Instance};

struct Raw {
    hwnd: isize,
    pid: u32,
    title: String,
}

/// Enumerate open VS Code / Insiders windows in Z-order (topmost first).
pub fn scan() -> Vec<Instance> {
    let mut raws: Vec<Raw> = Vec::new();
    unsafe {
        // EnumWindows returns Err if the callback ever returns FALSE; we always return TRUE.
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut raws as *mut _ as isize));
    }

    let mut out = Vec::new();
    for (z, raw) in raws.into_iter().enumerate() {
        let Some(image) = process_image_basename(raw.pid) else {
            continue;
        };
        if !is_vscode_image(&image) {
            continue;
        }
        let Some(parsed) = parse_title(&raw.title) else {
            continue;
        };
        out.push(Instance {
            hwnd: raw.hwnd as i64,
            app: App::from_image(&image),
            workspace: parsed.workspace,
            remote: parsed.remote,
            active_file: parsed.active_file,
            z_index: z,
        });
    }
    out
}

fn is_vscode_image(image: &str) -> bool {
    matches!(
        image.to_ascii_lowercase().as_str(),
        "code.exe" | "code - insiders.exe" | "code - exploration.exe"
    )
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let raws = &mut *(lparam.0 as *mut Vec<Raw>);

    if !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return TRUE;
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    let read = GetWindowTextW(hwnd, &mut buf);
    if read <= 0 {
        return TRUE;
    }
    let title = String::from_utf16_lossy(&buf[..read as usize]);

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    raws.push(Raw {
        hwnd: hwnd.0 as isize,
        pid,
        title,
    });
    TRUE
}

/// Resolve a PID to its process image basename (e.g. `"Code - Insiders.exe"`).
/// Uses `PROCESS_QUERY_LIMITED_INFORMATION`, which works for same-user processes unelevated.
fn process_image_basename(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        result.ok()?;
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let base = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string();
        Some(base)
    }
}
