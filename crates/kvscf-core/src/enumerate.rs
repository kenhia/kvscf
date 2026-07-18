//! Win32 enumeration of open windows, filtered per app.
//!
//! `Get-Process` is insufficient — VS Code and Edge each host many windows in one process — so we
//! walk every top-level window with `EnumWindows` and resolve each window's process image, then
//! dispatch to the VS Code or Edge parser.

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, TRUE};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible,
};

use crate::parse::{parse_edge_title, parse_title};
use crate::{App, AppMatcher, EdgeWindow, Instance};

struct Raw {
    hwnd: isize,
    pid: u32,
    class: String,
    title: String,
}

/// A visible, titled top-level window with its process image resolved.
struct ImagedWin {
    hwnd: i64,
    image: String,
    class: String,
    title: String,
}

/// Every visible, titled top-level window (in Z-order), with its process image basename.
fn raw_windows() -> Vec<ImagedWin> {
    let mut raws: Vec<Raw> = Vec::new();
    unsafe {
        // EnumWindows returns Err if the callback ever returns FALSE; we always return TRUE.
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut raws as *mut _ as isize));
    }
    raws.into_iter()
        .map(|raw| ImagedWin {
            hwnd: raw.hwnd as i64,
            image: process_image_basename(raw.pid).unwrap_or_default(),
            class: raw.class,
            title: raw.title,
        })
        .collect()
}

/// Find the first (topmost) window matching an [`AppMatcher`] — for the Apps tab (sprint 007).
pub fn find_app_window(m: &AppMatcher) -> Option<i64> {
    // At least one of process/class must be set, else nothing matches.
    if m.process.is_none() && m.class.is_none() {
        return None;
    }
    raw_windows()
        .into_iter()
        .find(|w| {
            m.process
                .as_deref()
                .map(|p| w.image.eq_ignore_ascii_case(p))
                .unwrap_or(true)
                && m.class.as_deref().map(|c| w.class == c).unwrap_or(true)
                && m.title_contains
                    .as_deref()
                    .map(|t| w.title.contains(t))
                    .unwrap_or(true)
        })
        .map(|w| w.hwnd)
}

/// Open VS Code / Insiders windows.
pub fn scan() -> Vec<Instance> {
    raw_windows()
        .into_iter()
        .enumerate()
        .filter_map(|(z, w)| vscode_instance(&w, z))
        .collect()
}

/// Open Microsoft Edge windows.
pub fn scan_edge() -> Vec<EdgeWindow> {
    raw_windows()
        .into_iter()
        .enumerate()
        .filter_map(|(z, w)| edge_window(&w, z))
        .collect()
}

/// Both VS Code and Edge windows from a single enumeration pass (what the app uses).
pub fn scan_all() -> (Vec<Instance>, Vec<EdgeWindow>) {
    let mut code = Vec::new();
    let mut edge = Vec::new();
    for (z, w) in raw_windows().into_iter().enumerate() {
        if let Some(inst) = vscode_instance(&w, z) {
            code.push(inst);
        } else if let Some(ew) = edge_window(&w, z) {
            edge.push(ew);
        }
    }
    (code, edge)
}

fn vscode_instance(w: &ImagedWin, z: usize) -> Option<Instance> {
    if !is_vscode_image(&w.image) {
        return None;
    }
    let parsed = parse_title(&w.title)?;
    Some(Instance {
        hwnd: w.hwnd,
        app: App::from_image(&w.image),
        workspace: parsed.workspace,
        remote: parsed.remote,
        active_file: parsed.active_file,
        z_index: z,
    })
}

fn edge_window(w: &ImagedWin, z: usize) -> Option<EdgeWindow> {
    if !is_edge_image(&w.image) {
        return None;
    }
    let parsed = parse_edge_title(&w.title)?;
    Some(EdgeWindow {
        hwnd: w.hwnd,
        label: parsed.label,
        named: parsed.named,
        tab_count: parsed.tab_count,
        z_index: z,
    })
}

fn is_vscode_image(image: &str) -> bool {
    matches!(
        image.to_ascii_lowercase().as_str(),
        "code.exe" | "code - insiders.exe" | "code - exploration.exe"
    )
}

fn is_edge_image(image: &str) -> bool {
    image.eq_ignore_ascii_case("msedge.exe")
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

    let mut cbuf = [0u16; 256];
    let clen = GetClassNameW(hwnd, &mut cbuf);
    let class = String::from_utf16_lossy(&cbuf[..clen.max(0) as usize]);

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    raws.push(Raw {
        hwnd: hwnd.0 as isize,
        pid,
        class,
        title,
    });
    TRUE
}

/// Resolve a PID to its process image basename (e.g. `"msedge.exe"`).
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
