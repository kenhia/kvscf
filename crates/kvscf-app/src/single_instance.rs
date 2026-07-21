//! Named-mutex single-instance guard. Moved here from lib.rs (WI #496).

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;

/// Returns `true` if this is the first instance.
pub fn acquire() -> bool {
    unsafe {
        match CreateMutexW(None, true, w!("Local\\kvscf-single-instance")) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    // Another instance owns the mutex.
                    return false;
                }
                // HANDLE is Copy with no Drop, so the OS mutex handle stays open for the
                // whole process lifetime (we never CloseHandle it) — exactly what we want.
                let _ = handle;
                true
            }
            // If we can't create the mutex, fail open rather than block startup.
            Err(_) => true,
        }
    }
}
