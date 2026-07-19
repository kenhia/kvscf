//! Robust current-user registry access.
//!
//! Bug this fixes (observed live, reboot-reproducible): a process launched **before its user
//! profile hive is fully mounted** — e.g. kvscf auto-restored ~2.5 min into a slow-Windows-Update
//! boot — gets the `HKEY_CURRENT_USER` *pseudo-handle* cached to the empty `HKU\.DEFAULT` hive for
//! its entire lifetime. Our config reload runs every second, but re-reading through the same cached
//! `HKEY_CURRENT_USER` keeps hitting `.DEFAULT`, so it never recovers: the Apps tab shows "No apps
//! configured" until the app is restarted, even though the registry is perfectly intact.
//!
//! [`RegOpenCurrentUser`] re-resolves the *real* current-user hive from the thread token on every
//! call, bypassing that cached binding. So once the profile mounts, the next 1-second reload
//! self-heals. We open once, wrap the handle in a (winreg) [`RegKey`] so the existing get/enum/set
//! code is unchanged, and let its `Drop` close the handle.

use winreg::RegKey;
use windows::Win32::System::Registry::{RegOpenCurrentUser, HKEY, KEY_READ, KEY_WRITE};

/// A freshly-resolved handle to the current user's registry hive root (`HKU\<SID>`), opened for
/// read+write. Use [`key`](UserRoot::key) to open subkeys under it exactly as with a winreg root.
pub struct UserRoot(RegKey);

impl UserRoot {
    /// Resolve the real current-user hive. `None` if it isn't available yet (e.g. the profile
    /// hasn't mounted) — callers treat that as "no config this pass" and retry next reload.
    pub fn open() -> Option<UserRoot> {
        let mut hkey = HKEY::default();
        // SAFETY: standard Win32 call; `hkey` is a valid out-param.
        unsafe {
            if RegOpenCurrentUser((KEY_READ | KEY_WRITE).0, &mut hkey).is_err() {
                return None;
            }
        }
        // winreg's RegKey owns the handle and RegCloseKey's it on drop (RegCloseKey on a real hive
        // handle closes it exactly once; `predef` just means "don't synthesize a new handle").
        // winreg's HKEY is an isize; the windows-crate HKEY wraps a pointer of the same value.
        Some(UserRoot(RegKey::predef(hkey.0 as isize)))
    }

    /// The hive root, for `open_subkey` / `create_subkey` under `Software\…`.
    pub fn key(&self) -> &RegKey {
        &self.0
    }
}
