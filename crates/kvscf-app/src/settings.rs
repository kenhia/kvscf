//! Persisted app settings — `HKCU\Software\kenhia\kvscf` on Windows, in-memory defaults
//! elsewhere. Moved here from lib.rs (WI #496).

/// Defaults: everything off (auto-hide default off, per request).
#[derive(Default)]
pub struct Settings {
    pub maximize_on_focus: bool,
    pub auto_hide: bool,
    pub docked: bool,
}

#[cfg(windows)]
mod imp {
    use super::Settings;
    use crate::userreg::UserRoot;

    const PATH: &str = r"Software\kenhia\kvscf";

    pub fn load() -> Settings {
        let mut s = Settings::default();
        // Real user hive (see `userreg`) — a boot-cached HKCU would silently read the wrong hive.
        if let Some(key) = UserRoot::open().and_then(|u| u.key().open_subkey(PATH).ok()) {
            let get = |name: &str| key.get_value::<u32, _>(name).ok().map(|v| v != 0);
            if let Some(v) = get("maximize_on_focus") {
                s.maximize_on_focus = v;
            }
            if let Some(v) = get("auto_hide") {
                s.auto_hide = v;
            }
            if let Some(v) = get("docked") {
                s.docked = v;
            }
        }
        s
    }

    pub fn save(s: &Settings) {
        if let Some((key, _)) = UserRoot::open().and_then(|u| u.key().create_subkey(PATH).ok()) {
            let _ = key.set_value("maximize_on_focus", &(s.maximize_on_focus as u32));
            let _ = key.set_value("auto_hide", &(s.auto_hide as u32));
            let _ = key.set_value("docked", &(s.docked as u32));
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Settings;
    pub fn load() -> Settings {
        Settings::default()
    }
    pub fn save(_s: &Settings) {}
}

pub use imp::{load, save};
