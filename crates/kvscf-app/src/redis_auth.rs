//! Where the Redis password comes from (sprint 022, korg:2434 / WI 2412).
//!
//! The fleet's CD-19 contract (kdashdata handoff korg:2494) for a process that opens the per-host
//! secrets file itself. Transliterated from kpidash-client-win's `auth.rs` (korg:2429), which is in
//! turn kdashdata's parser, deliberately: one file, read by several consumers, must parse one way.
//!
//! 1. **The environment variable named by the auth key** — an explicit override. A `.env` counts,
//!    since [`crate::remote`] loads it into the environment at startup.
//! 2. **`%ProgramData%\khomelab\secrets.env`** — the one per-host copy k-homelab renders,
//!    `KEY='value'` lines. `ProgramData` is read at **run time** and the rung is **skipped** when it
//!    is unset, never defaulted to `C:\ProgramData`: that is what it resolves to on Ken's machines,
//!    not a fact about Windows.
//! 3. **`HKCU\Software\kenhia\kvscf` `KVSCF_REDIS_PASSWORD`, then a `KVSCF_REDIS_PASSWORD`
//!    environment variable** — the sprint 018 locations, deprecated. They still answer so an
//!    install that works keeps working, and they say so.
//!
//! **The key follows the endpoint, not the binary.** Every password in the per-host file has one
//! fleet-wide name, and kvscf talks to two different Redis instances: cleo publishes to rpidash2,
//! whose password is named `CLAUDE_REDISCLI_AUTH` (after the `redis-claude` instance, which kvscf
//! shares), and kwork publishes to rpidash3, whose password is `KVSCF_REDISCLI_AUTH`. So the key
//! is a setting, [`AUTH_KEY_SETTING`], read from the same place as `KVSCF_REDIS_HOST` and
//! defaulting to the default endpoint's name. A lookup order over both names was rejected (Ken,
//! 2026-09-13): on a host whose file held both it would silently present the wrong password.
//!
//! On rung 2, a file that cannot be read and a file without the key both mean **keep looking**,
//! not stop. The file is shared, multi-key and rendered by another repo, so either is a state of
//! the fleet, not a fault in kvscf.
//!
//! Resolved on every connect: a rotation rewrites the file, the next connection error drops the
//! loop back to connect, and the new value is read there. No relaunch.
//!
//! Nothing here prints a value. The environment and the registry are injected rather than read
//! directly, so every rung is testable without mutating the process environment, which
//! `cargo test`'s threads share.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The non-secret setting naming which key in the environment / per-host file holds the password.
pub const AUTH_KEY_SETTING: &str = "KVSCF_REDIS_AUTH_KEY";

/// The key for the default endpoint (rpidash2:6380, `remote::DEFAULT_HOST`). A host that points
/// `KVSCF_REDIS_HOST` elsewhere names its endpoint's key too — kwork: `KVSCF_REDISCLI_AUTH`.
pub const DEFAULT_AUTH_KEY: &str = "CLAUDE_REDISCLI_AUTH";

/// The Windows variable naming the per-machine data directory.
pub const PROGRAMDATA_ENV: &str = "ProgramData";

/// The sprint 018 name, as a registry value and as an environment variable. Deprecated.
pub const LEGACY_NAME: &str = "KVSCF_REDIS_PASSWORD";

/// Which rung answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The environment variable named by the auth key.
    Environment(String),
    PerHostFile {
        path: PathBuf,
        key: String,
    },
    /// `HKCU\Software\kenhia\kvscf` `KVSCF_REDIS_PASSWORD` — deprecated.
    Registry,
    /// A `KVSCF_REDIS_PASSWORD` environment variable — deprecated.
    LegacyEnvironment,
    /// No rung answered: connect without AUTH, and let Redis say whether that is acceptable.
    Nothing,
}

impl Source {
    /// How to name this source to a human — never the value.
    pub fn describe(&self) -> String {
        match self {
            Source::Environment(key) => format!("{key} environment variable"),
            Source::PerHostFile { path, key } => {
                format!("per-host secrets file ({}, {key})", path.display())
            }
            Source::Registry => {
                format!("HKCU\\Software\\kenhia\\kvscf {LEGACY_NAME} (deprecated)")
            }
            Source::LegacyEnvironment => {
                format!("{LEGACY_NAME} environment variable (deprecated)")
            }
            Source::Nothing => "none found - connecting without AUTH".to_string(),
        }
    }

    pub fn is_deprecated(&self) -> bool {
        matches!(self, Source::Registry | Source::LegacyEnvironment)
    }
}

/// The password and where it came from.
#[derive(Clone, PartialEq, Eq)]
pub struct Resolution {
    pub password: Option<String>,
    pub source: Source,
}

/// Redacting by hand: a derived `Debug` would put the password in any `{:?}`, including an
/// `assert_eq!` failure.
impl std::fmt::Debug for Resolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolution")
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("source", &self.source)
            .finish()
    }
}

/// The key to look for: [`AUTH_KEY_SETTING`] if set and non-empty, else [`DEFAULT_AUTH_KEY`].
pub fn auth_key_in<F>(env: &F) -> String
where
    F: Fn(&str) -> Option<OsString>,
{
    env(AUTH_KEY_SETTING)
        .and_then(|v| v.into_string().ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_AUTH_KEY.to_string())
}

/// Pull `key` out of an `EnvironmentFile`-shaped text.
///
/// kdashdata's `parse_env_file`, by way of kpidash-client-win, with the key a parameter. `KEY=value`
/// per line, `#` comments, optional surrounding single or double quotes, last assignment wins, and
/// an empty assignment is "not set" rather than "the empty password". k-homelab writes
/// `KEY='value'` (handoff korg:2480) — the single quotes are stripped, never re-interpreted.
pub fn parse_env_file(text: &str, key: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("Environment=").unwrap_or(line);
        let Some(value) = line.strip_prefix(key).and_then(|r| r.strip_prefix('=')) else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        if !value.is_empty() {
            found = Some(value.to_string());
        }
    }
    found
}

/// `%ProgramData%\khomelab\secrets.env`, or `None` when `ProgramData` is unset or empty — skipped,
/// never guessed.
pub fn per_host_file_in<F>(env: &F) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<OsString>,
{
    env(PROGRAMDATA_ENV)
        .filter(|v| !v.is_empty())
        .map(|dir| PathBuf::from(dir).join("khomelab").join("secrets.env"))
}

/// Resolve `key` against an arbitrary environment and registry. `registry` is only consulted if
/// neither the environment nor the per-host file answered.
pub fn resolve_in<F, R>(key: &str, env: F, registry: R) -> Resolution
where
    F: Fn(&str) -> Option<OsString>,
    R: FnOnce() -> Option<String>,
{
    let answer = |password: String, source: Source| Resolution {
        password: Some(password),
        source,
    };
    let non_empty = |name: &str| {
        env(name)
            .and_then(|v| v.into_string().ok())
            .filter(|v| !v.is_empty())
    };

    if let Some(password) = non_empty(key) {
        return answer(password, Source::Environment(key.to_string()));
    }

    if let Some(path) = per_host_file_in(&env) {
        // Missing, unreadable (the file is created elevated), or without our key: keep looking.
        if let Some(password) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| parse_env_file(&text, key))
        {
            let key = key.to_string();
            return answer(password, Source::PerHostFile { path, key });
        }
    }

    if let Some(password) = registry().filter(|v| !v.is_empty()) {
        return answer(password, Source::Registry);
    }

    if let Some(password) = non_empty(LEGACY_NAME) {
        return answer(password, Source::LegacyEnvironment);
    }

    Resolution {
        password: None,
        source: Source::Nothing,
    }
}

static LAST_SOURCE: Mutex<Option<Source>> = Mutex::new(None);
static WARNED_DEPRECATED: AtomicBool = AtomicBool::new(false);

/// Say where the password came from — when that changes, not on every reconnect (the publisher and
/// subscriber both resolve) — and warn once per process while a deprecated rung is answering.
pub fn announce(key: &str, resolution: &Resolution) {
    {
        let mut last = LAST_SOURCE.lock().unwrap_or_else(|p| p.into_inner());
        if last.as_ref() != Some(&resolution.source) {
            eprintln!(
                "kvscf: redis password from {}",
                resolution.source.describe()
            );
            *last = Some(resolution.source.clone());
        }
    }
    if resolution.source.is_deprecated() && !WARNED_DEPRECATED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "kvscf: {LEGACY_NAME} is deprecated - put {key} in %ProgramData%\\khomelab\\secrets.env \
             (or the environment) and delete the old value"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

    /// A fake environment. Every test builds its own, so none of them touch the process
    /// environment `cargo test`'s threads share.
    fn fake_env(pairs: &[(&str, &str)]) -> HashMap<String, OsString> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(*v)))
            .collect()
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kvscf-redis-auth-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_per_host(programdata: &Path, text: &str) -> PathBuf {
        let dir = programdata.join("khomelab");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("secrets.env");
        std::fs::write(&file, text).unwrap();
        file
    }

    const K: &str = DEFAULT_AUTH_KEY;

    #[test]
    fn the_default_key_pairs_with_the_default_endpoint() {
        let env = fake_env(&[]);
        assert_eq!(
            auth_key_in(&|k| env.get(k).cloned()),
            "CLAUDE_REDISCLI_AUTH"
        );
    }

    #[test]
    fn the_setting_names_another_endpoints_key() {
        let env = fake_env(&[(AUTH_KEY_SETTING, " KVSCF_REDISCLI_AUTH ")]);
        assert_eq!(auth_key_in(&|k| env.get(k).cloned()), "KVSCF_REDISCLI_AUTH");
        let empty = fake_env(&[(AUTH_KEY_SETTING, "")]);
        assert_eq!(auth_key_in(&|k| empty.get(k).cloned()), DEFAULT_AUTH_KEY);
    }

    #[test]
    fn the_per_host_files_single_quoted_shape_parses() {
        let text = "REDISCLI_AUTH='fleet'\n\
                    CLAUDE_REDISCLI_AUTH='desk $password'\n\
                    HF_TOKEN='third'\n";
        assert_eq!(parse_env_file(text, K).as_deref(), Some("desk $password"));
    }

    #[test]
    fn a_neighbour_whose_name_contains_ours_is_not_ours() {
        // The fleet's three Redis keys nest inside one another's names.
        let text = "REDISCLI_AUTH='a'\n\
                    KVSCF_REDISCLI_AUTH='b'\n\
                    CLAUDE_REDISCLI_AUTH_OLD='c'\n";
        assert_eq!(parse_env_file(text, K), None);
        assert_eq!(
            parse_env_file(text, "KVSCF_REDISCLI_AUTH").as_deref(),
            Some("b")
        );
        assert_eq!(parse_env_file(text, "REDISCLI_AUTH").as_deref(), Some("a"));
    }

    #[test]
    fn double_quotes_comments_and_crlf_are_handled() {
        let text = "# rendered by k-homelab\r\nCLAUDE_REDISCLI_AUTH=\"quoted value\"\r\n";
        assert_eq!(parse_env_file(text, K).as_deref(), Some("quoted value"));
    }

    #[test]
    fn the_last_assignment_wins_and_an_empty_one_is_unset() {
        let text = "CLAUDE_REDISCLI_AUTH='first'\nCLAUDE_REDISCLI_AUTH='second'\n";
        assert_eq!(parse_env_file(text, K).as_deref(), Some("second"));
        let text = "CLAUDE_REDISCLI_AUTH='first'\nCLAUDE_REDISCLI_AUTH=''\n";
        assert_eq!(parse_env_file(text, K).as_deref(), Some("first"));
    }

    #[test]
    fn the_environment_beats_the_file_and_the_registry() {
        let dir = tmpdir("env-wins");
        write_per_host(&dir, "CLAUDE_REDISCLI_AUTH='from-file'\n");
        let env = fake_env(&[(K, "from-env"), (PROGRAMDATA_ENV, dir.to_str().unwrap())]);
        let res = resolve_in(K, |k| env.get(k).cloned(), || Some("from-reg".into()));
        assert_eq!(res.source, Source::Environment(K.into()));
        assert_eq!(res.password.as_deref(), Some("from-env"));
    }

    #[test]
    fn the_per_host_file_beats_the_registry() {
        let dir = tmpdir("file-wins");
        let file = write_per_host(&dir, "CLAUDE_REDISCLI_AUTH='from-file'\n");
        let env = fake_env(&[(PROGRAMDATA_ENV, dir.to_str().unwrap())]);
        let res = resolve_in(K, |k| env.get(k).cloned(), || Some("from-reg".into()));
        assert_eq!(
            res.source,
            Source::PerHostFile {
                path: file,
                key: K.into()
            }
        );
        assert_eq!(res.password.as_deref(), Some("from-file"));
    }

    #[test]
    fn kwork_reads_its_own_key_once_the_setting_names_it() {
        // A file holding both desks' passwords: the setting, not an order, decides.
        let dir = tmpdir("kwork");
        write_per_host(
            &dir,
            "CLAUDE_REDISCLI_AUTH='rpidash2'\nKVSCF_REDISCLI_AUTH='rpidash3'\n",
        );
        let env = fake_env(&[
            (PROGRAMDATA_ENV, dir.to_str().unwrap()),
            (AUTH_KEY_SETTING, "KVSCF_REDISCLI_AUTH"),
        ]);
        let key = auth_key_in(&|k| env.get(k).cloned());
        let res = resolve_in(&key, |k| env.get(k).cloned(), || None);
        assert_eq!(res.password.as_deref(), Some("rpidash3"));
    }

    #[test]
    fn without_the_setting_a_kwork_file_falls_through_to_the_registry() {
        // Pins WI 2404's ordering condition: until kwork's `.env` names its key, its file does not
        // answer and the registry value is the only thing authenticating. Delete that first and
        // kwork connects without AUTH.
        let dir = tmpdir("kwork-unset");
        write_per_host(&dir, "KVSCF_REDISCLI_AUTH='rpidash3'\n");
        let env = fake_env(&[(PROGRAMDATA_ENV, dir.to_str().unwrap())]);
        let key = auth_key_in(&|k| env.get(k).cloned());
        let res = resolve_in(&key, |k| env.get(k).cloned(), || Some("from-reg".into()));
        assert_eq!(res.source, Source::Registry);
        let res = resolve_in(&key, |k| env.get(k).cloned(), || None);
        assert_eq!(res.source, Source::Nothing);
    }

    #[test]
    fn an_unset_or_empty_programdata_skips_the_rung_rather_than_guessing() {
        let unset = fake_env(&[]);
        assert_eq!(per_host_file_in(&|k| unset.get(k).cloned()), None);
        let empty = fake_env(&[(PROGRAMDATA_ENV, "")]);
        assert_eq!(per_host_file_in(&|k| empty.get(k).cloned()), None);
        let res = resolve_in(K, |k| unset.get(k).cloned(), || Some("from-reg".into()));
        assert_eq!(res.source, Source::Registry);
    }

    #[test]
    fn the_rung_follows_programdata_wherever_it_points() {
        let env = fake_env(&[(PROGRAMDATA_ENV, "E:\\machine-data")]);
        assert_eq!(
            per_host_file_in(&|k| env.get(k).cloned()),
            Some(PathBuf::from("E:\\machine-data\\khomelab\\secrets.env"))
        );
    }

    #[test]
    fn a_file_without_our_key_keeps_looking() {
        let dir = tmpdir("no-key");
        write_per_host(&dir, "REDISCLI_AUTH='not ours'\n");
        let env = fake_env(&[(PROGRAMDATA_ENV, dir.to_str().unwrap())]);
        let res = resolve_in(K, |k| env.get(k).cloned(), || Some("from-reg".into()));
        assert_eq!(res.source, Source::Registry);
    }

    #[test]
    fn an_unreadable_file_keeps_looking() {
        // A directory where the file should be: exists, cannot be read.
        let dir = tmpdir("unreadable");
        std::fs::create_dir_all(dir.join("khomelab").join("secrets.env")).unwrap();
        let env = fake_env(&[(PROGRAMDATA_ENV, dir.to_str().unwrap())]);
        let res = resolve_in(K, |k| env.get(k).cloned(), || Some("from-reg".into()));
        assert_eq!(res.source, Source::Registry);
    }

    #[test]
    fn a_missing_file_keeps_looking() {
        let dir = tmpdir("missing");
        let env = fake_env(&[(PROGRAMDATA_ENV, dir.to_str().unwrap())]);
        let res = resolve_in(K, |k| env.get(k).cloned(), || Some("from-reg".into()));
        assert_eq!(res.source, Source::Registry);
    }

    #[test]
    fn the_legacy_environment_variable_is_the_last_rung() {
        let env = fake_env(&[(LEGACY_NAME, "old-env")]);
        let res = resolve_in(K, |k| env.get(k).cloned(), || Some("from-reg".into()));
        assert_eq!(
            res.source,
            Source::Registry,
            "registry outranks it, as in 018"
        );
        let res = resolve_in(K, |k| env.get(k).cloned(), || None);
        assert_eq!(res.source, Source::LegacyEnvironment);
        assert!(res.source.is_deprecated());
    }

    #[test]
    fn the_registry_is_not_read_when_a_better_rung_answers() {
        let env = fake_env(&[(K, "from-env")]);
        let res = resolve_in(K, |k| env.get(k).cloned(), || panic!("registry read"));
        assert_eq!(res.source, Source::Environment(K.into()));
    }

    #[test]
    fn nothing_anywhere_means_no_auth() {
        let dir = tmpdir("nothing");
        let env = fake_env(&[(PROGRAMDATA_ENV, dir.to_str().unwrap())]);
        let res = resolve_in(K, |k| env.get(k).cloned(), || Some(String::new()));
        assert_eq!(res.source, Source::Nothing);
        assert_eq!(res.password, None);
    }

    #[test]
    fn neither_debug_nor_the_description_carries_the_password() {
        let dir = tmpdir("redact");
        write_per_host(&dir, "CLAUDE_REDISCLI_AUTH='hunter2'\n");
        let env = fake_env(&[(PROGRAMDATA_ENV, dir.to_str().unwrap())]);
        let res = resolve_in(K, |k| env.get(k).cloned(), || None);
        assert!(!format!("{res:?}").contains("hunter2"));
        assert!(!res.source.describe().contains("hunter2"));
    }
}
