//! PulseAudio server-socket and authentication-cookie discovery.
//!
//! Everything in this module is platform-independent and free of PulseAudio protocol
//! types, so it compiles — and is unit-tested — on every host, not just Linux.
//!
//! Environment access goes through the [`EnvProvider`] trait rather than
//! [`std::env::var`] directly. Tests inject a [`MapEnv`] instead of mutating the
//! process environment, which would race with other tests running in the same
//! process.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oxisound_core::OxiSoundError;

/// Path of the native-protocol socket relative to a PulseAudio runtime directory.
pub const PULSE_NATIVE_SOCKET_RELATIVE: &str = "pulse/native";

/// Length in bytes of a well-formed PulseAudio authentication cookie.
///
/// PulseAudio writes exactly this many random bytes to `~/.config/pulse/cookie`.
/// A shorter file is still forwarded to the server (which will reject it), but a
/// warning is logged so the misconfiguration is visible.
pub const PULSE_COOKIE_LENGTH: usize = 256;

/// Read-only view of the process environment, injectable for tests.
pub trait EnvProvider {
    /// Returns the value of an environment variable, or `None` when unset or empty.
    fn var(&self, key: &str) -> Option<String>;

    /// Returns the current user's home directory.
    fn home_dir(&self) -> Option<PathBuf>;

    /// Returns the real user id of the current process, or `None` when unavailable.
    fn uid(&self) -> Option<u32>;

    /// Returns `true` when `path` exists and is reachable.
    fn path_exists(&self, path: &Path) -> bool;
}

/// [`EnvProvider`] backed by the real process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnv;

impl EnvProvider for ProcessEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }

    fn home_dir(&self) -> Option<PathBuf> {
        // `$HOME` only — deliberately not `std::env::home_dir()`, so the lookup is
        // fully injectable and has no platform-specific fallback behaviour.
        self.var("HOME").map(PathBuf::from)
    }

    fn uid(&self) -> Option<u32> {
        real_uid()
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// Returns the real user id of the current process without any C FFI.
///
/// `/proc/self` is owned by the process's real uid, so a plain `stat` yields it.
/// Returns `None` on platforms without `procfs` (every non-Linux target), where
/// the `/run/user/<uid>` fallback is not applicable anyway.
#[cfg(unix)]
fn real_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

#[cfg(not(unix))]
fn real_uid() -> Option<u32> {
    None
}

/// In-memory [`EnvProvider`] for tests: no process-global state is touched.
#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    vars: BTreeMap<String, String>,
    home: Option<PathBuf>,
    uid: Option<u32>,
    existing: Vec<PathBuf>,
}

impl MapEnv {
    /// Creates an empty environment: no variables, no home directory, no uid.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets an environment variable.
    #[must_use]
    pub fn with_var(mut self, key: &str, value: &str) -> Self {
        self.vars.insert(key.to_string(), value.to_string());
        self
    }

    /// Sets the home directory reported by [`EnvProvider::home_dir`].
    #[must_use]
    pub fn with_home(mut self, home: impl Into<PathBuf>) -> Self {
        self.home = Some(home.into());
        self
    }

    /// Sets the uid reported by [`EnvProvider::uid`].
    #[must_use]
    pub fn with_uid(mut self, uid: u32) -> Self {
        self.uid = Some(uid);
        self
    }

    /// Marks `path` as existing for [`EnvProvider::path_exists`].
    #[must_use]
    pub fn with_existing(mut self, path: impl Into<PathBuf>) -> Self {
        self.existing.push(path.into());
        self
    }
}

impl EnvProvider for MapEnv {
    fn var(&self, key: &str) -> Option<String> {
        self.vars.get(key).filter(|v| !v.is_empty()).cloned()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn uid(&self) -> Option<u32> {
        self.uid
    }

    fn path_exists(&self, path: &Path) -> bool {
        self.existing.iter().any(|p| p == path)
    }
}

/// Extracts the unix socket path from a single `$PULSE_SERVER` entry.
///
/// PulseAudio accepts several spellings, all of which are handled here:
///
/// - `unix:/run/user/1000/pulse/native`
/// - `{6c4c…}unix:/run/user/1000/pulse/native` — a machine-id prefix in braces
///
/// Entries that name a remote transport (`tcp:`, `tcp4:`, `tcp6:`, or a bare
/// hostname) yield `None`.
#[must_use]
pub fn unix_path_from_server_entry(entry: &str) -> Option<PathBuf> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }
    // Strip an optional `{machine-id}` prefix.
    let rest = match entry.strip_prefix('{') {
        Some(after_brace) => match after_brace.split_once('}') {
            Some((_, tail)) => tail,
            // Unterminated brace: nothing usable.
            None => return None,
        },
        None => entry,
    };
    let path = rest.strip_prefix("unix:")?;
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

/// Resolves the PulseAudio native-protocol socket path.
///
/// Candidates are tried in the order PulseAudio's own client library uses:
///
/// 1. `$PULSE_SERVER` — honoured unconditionally when it names a unix socket,
///    even if the path does not exist yet, because it is an explicit user choice.
///    When it is set but names only remote transports, an
///    [`OxiSoundError::Unsupported`] is returned rather than silently falling
///    through to the local server.
/// 2. `$PULSE_RUNTIME_PATH/native`, then `$PULSE_RUNTIME_PATH/pulse/native`.
/// 3. `$XDG_RUNTIME_DIR/pulse/native`.
/// 4. `/run/user/<uid>/pulse/native`, with the uid read from `/proc/self`.
///
/// Candidates 2–4 must exist to be selected.
///
/// # Errors
///
/// Returns [`OxiSoundError::Unsupported`] when `$PULSE_SERVER` names only remote
/// transports, and [`OxiSoundError::Device`] when no candidate socket was found.
pub fn resolve_server_socket<E: EnvProvider + ?Sized>(env: &E) -> Result<PathBuf, OxiSoundError> {
    if let Some(server) = env.var("PULSE_SERVER") {
        let mut saw_entry = false;
        for entry in server.split_whitespace() {
            saw_entry = true;
            if let Some(path) = unix_path_from_server_entry(entry) {
                return Ok(path);
            }
        }
        if saw_entry {
            return Err(OxiSoundError::Unsupported(format!(
                "PULSE_SERVER=\"{server}\" names no unix socket; oxisound-pulse speaks the \
                 PulseAudio native protocol over a unix socket only (remote tcp: servers are \
                 out of scope)"
            )));
        }
    }

    let mut tried: Vec<PathBuf> = Vec::new();

    if let Some(runtime) = env.var("PULSE_RUNTIME_PATH") {
        // `$PULSE_RUNTIME_PATH` normally already points at the `pulse` directory,
        // but some setups export the parent instead; probe both spellings.
        let base = PathBuf::from(runtime);
        for candidate in [base.join("native"), base.join(PULSE_NATIVE_SOCKET_RELATIVE)] {
            if env.path_exists(&candidate) {
                return Ok(candidate);
            }
            tried.push(candidate);
        }
    }

    if let Some(xdg) = env.var("XDG_RUNTIME_DIR") {
        let candidate = PathBuf::from(xdg).join(PULSE_NATIVE_SOCKET_RELATIVE);
        if env.path_exists(&candidate) {
            return Ok(candidate);
        }
        tried.push(candidate);
    }

    if let Some(uid) = env.uid() {
        let candidate =
            PathBuf::from(format!("/run/user/{uid}")).join(PULSE_NATIVE_SOCKET_RELATIVE);
        if env.path_exists(&candidate) {
            return Ok(candidate);
        }
        tried.push(candidate);
    }

    Err(OxiSoundError::Device(format!(
        "no PulseAudio (or pipewire-pulse) socket found; tried: {}",
        format_candidates(&tried)
    )))
}

fn format_candidates(tried: &[PathBuf]) -> String {
    if tried.is_empty() {
        return "$PULSE_SERVER, $PULSE_RUNTIME_PATH, $XDG_RUNTIME_DIR, /run/user/<uid> \
                (none were set)"
            .to_string();
    }
    tried
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolves the path of the PulseAudio authentication cookie.
///
/// Candidates, in order:
///
/// 1. `$PULSE_COOKIE`
/// 2. `$XDG_CONFIG_HOME/pulse/cookie`
/// 3. `$HOME/.config/pulse/cookie`
/// 4. `$HOME/.pulse-cookie`
///
/// Returns `None` when none of them exists. That is not an error: servers reached
/// over a same-uid unix socket — notably `pipewire-pulse`, and PulseAudio itself
/// with `auth-anonymous=1` — accept a cookie-less `AUTH` command.
#[must_use]
pub fn resolve_cookie_path<E: EnvProvider + ?Sized>(env: &E) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(explicit) = env.var("PULSE_COOKIE") {
        candidates.push(PathBuf::from(explicit));
    }
    if let Some(config_home) = env.var("XDG_CONFIG_HOME") {
        candidates.push(PathBuf::from(config_home).join("pulse/cookie"));
    }
    if let Some(home) = env.home_dir() {
        candidates.push(home.join(".config/pulse/cookie"));
        candidates.push(home.join(".pulse-cookie"));
    }
    candidates.into_iter().find(|p| env.path_exists(p))
}

/// Reads a cookie file.
///
/// Returns `None` when the file cannot be read. A file whose length differs from
/// [`PULSE_COOKIE_LENGTH`] is still returned — the server is the authority on
/// whether it authenticates — but a warning is logged.
#[must_use]
pub fn load_cookie(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => {
            if bytes.len() != PULSE_COOKIE_LENGTH {
                log::warn!(
                    "PulseAudio cookie {} is {} bytes, expected {PULSE_COOKIE_LENGTH}; \
                     authentication may fail",
                    path.display(),
                    bytes.len()
                );
            }
            Some(bytes)
        }
        Err(err) => {
            log::debug!("could not read PulseAudio cookie {}: {err}", path.display());
            None
        }
    }
}

/// Resolves and reads the authentication cookie in one step.
///
/// Returns `None` when no cookie file was found or it could not be read.
#[must_use]
pub fn load_cookie_from_env<E: EnvProvider + ?Sized>(env: &E) -> Option<Vec<u8>> {
    resolve_cookie_path(env).as_deref().and_then(load_cookie)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_server_unix_prefix_wins() {
        let env = MapEnv::new()
            .with_var("PULSE_SERVER", "unix:/tmp/does-not-exist/native")
            .with_var("XDG_RUNTIME_DIR", "/run/user/1000")
            .with_existing("/run/user/1000/pulse/native");
        let path = resolve_server_socket(&env).expect("PULSE_SERVER should resolve");
        assert_eq!(path, PathBuf::from("/tmp/does-not-exist/native"));
    }

    #[test]
    fn pulse_server_machine_id_prefix_is_stripped() {
        let entry = "{6c4cb0b6f0a0}unix:/run/user/1000/pulse/native";
        assert_eq!(
            unix_path_from_server_entry(entry),
            Some(PathBuf::from("/run/user/1000/pulse/native"))
        );
    }

    #[test]
    fn pulse_server_remote_only_is_unsupported() {
        let env = MapEnv::new().with_var("PULSE_SERVER", "tcp:example.invalid:4713");
        let err = resolve_server_socket(&env).expect_err("tcp: must not resolve");
        assert_eq!(err.kind(), "unsupported");
    }

    #[test]
    fn pulse_server_list_picks_first_unix_entry() {
        let env = MapEnv::new().with_var(
            "PULSE_SERVER",
            "tcp:example.invalid:4713 unix:/run/user/7/pulse/native",
        );
        let path = resolve_server_socket(&env).expect("unix entry should win");
        assert_eq!(path, PathBuf::from("/run/user/7/pulse/native"));
    }

    #[test]
    fn pulse_runtime_path_native_before_xdg() {
        let env = MapEnv::new()
            .with_var("PULSE_RUNTIME_PATH", "/run/user/1000/pulse")
            .with_var("XDG_RUNTIME_DIR", "/run/user/1000")
            .with_existing("/run/user/1000/pulse/native");
        let path = resolve_server_socket(&env).expect("runtime path should resolve");
        assert_eq!(path, PathBuf::from("/run/user/1000/pulse/native"));
    }

    #[test]
    fn pulse_runtime_path_parent_spelling_also_probed() {
        let env = MapEnv::new()
            .with_var("PULSE_RUNTIME_PATH", "/run/user/1000")
            .with_existing("/run/user/1000/pulse/native");
        let path = resolve_server_socket(&env).expect("parent spelling should resolve");
        assert_eq!(path, PathBuf::from("/run/user/1000/pulse/native"));
    }

    #[test]
    fn xdg_runtime_dir_used_when_present() {
        let env = MapEnv::new()
            .with_var("XDG_RUNTIME_DIR", "/run/user/1234")
            .with_existing("/run/user/1234/pulse/native");
        let path = resolve_server_socket(&env).expect("xdg should resolve");
        assert_eq!(path, PathBuf::from("/run/user/1234/pulse/native"));
    }

    #[test]
    fn uid_fallback_used_when_no_env_matches() {
        let env = MapEnv::new()
            .with_var("XDG_RUNTIME_DIR", "/run/user/nope")
            .with_uid(4242)
            .with_existing("/run/user/4242/pulse/native");
        let path = resolve_server_socket(&env).expect("uid fallback should resolve");
        assert_eq!(path, PathBuf::from("/run/user/4242/pulse/native"));
    }

    #[test]
    fn no_candidate_yields_device_error_listing_attempts() {
        let env = MapEnv::new()
            .with_var("XDG_RUNTIME_DIR", "/run/user/9")
            .with_uid(9);
        let err = resolve_server_socket(&env).expect_err("nothing exists");
        assert_eq!(err.kind(), "device");
        assert!(
            err.to_string().contains("/run/user/9/pulse/native"),
            "error should list the candidates it tried: {err}"
        );
    }

    #[test]
    fn empty_env_var_is_treated_as_unset() {
        let env = MapEnv::new().with_var("PULSE_SERVER", "");
        let err = resolve_server_socket(&env).expect_err("nothing to resolve");
        // Empty PULSE_SERVER must NOT be reported as an unsupported remote server.
        assert_eq!(err.kind(), "device");
    }

    #[test]
    fn cookie_precedence_explicit_then_xdg_then_home() {
        let env = MapEnv::new()
            .with_var("PULSE_COOKIE", "/etc/pulse/cookie")
            .with_var("XDG_CONFIG_HOME", "/home/u/.config")
            .with_home("/home/u")
            .with_existing("/etc/pulse/cookie")
            .with_existing("/home/u/.config/pulse/cookie")
            .with_existing("/home/u/.pulse-cookie");
        assert_eq!(
            resolve_cookie_path(&env),
            Some(PathBuf::from("/etc/pulse/cookie"))
        );

        let env = MapEnv::new()
            .with_var("XDG_CONFIG_HOME", "/home/u/.config")
            .with_home("/home/u")
            .with_existing("/home/u/.config/pulse/cookie")
            .with_existing("/home/u/.pulse-cookie");
        assert_eq!(
            resolve_cookie_path(&env),
            Some(PathBuf::from("/home/u/.config/pulse/cookie"))
        );

        let env = MapEnv::new()
            .with_home("/home/u")
            .with_existing("/home/u/.pulse-cookie");
        assert_eq!(
            resolve_cookie_path(&env),
            Some(PathBuf::from("/home/u/.pulse-cookie"))
        );
    }

    #[test]
    fn cookie_absent_is_not_an_error() {
        let env = MapEnv::new().with_home("/home/u");
        assert_eq!(resolve_cookie_path(&env), None);
        assert_eq!(load_cookie_from_env(&env), None);
    }

    #[test]
    fn load_cookie_reads_exact_bytes_and_tolerates_short_files() {
        let dir = std::env::temp_dir().join("oxisound-pulse-cookie-test");
        std::fs::create_dir_all(&dir).expect("temp dir");

        let full = dir.join("full-cookie");
        let bytes = vec![0xA5u8; PULSE_COOKIE_LENGTH];
        std::fs::write(&full, &bytes).expect("write cookie");
        assert_eq!(load_cookie(&full), Some(bytes));

        let short = dir.join("short-cookie");
        std::fs::write(&short, [1u8, 2, 3]).expect("write short cookie");
        assert_eq!(load_cookie(&short), Some(vec![1, 2, 3]));

        let missing = dir.join("no-such-cookie");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(load_cookie(&missing), None);

        let _ = std::fs::remove_file(&full);
        let _ = std::fs::remove_file(&short);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn non_unix_entries_yield_none() {
        for entry in [
            "",
            "   ",
            "tcp:host:4713",
            "tcp6:[::1]:4713",
            "{abc",
            "unix:",
        ] {
            assert_eq!(
                unix_path_from_server_entry(entry),
                None,
                "entry {entry:?} must not resolve to a unix path"
            );
        }
    }
}
