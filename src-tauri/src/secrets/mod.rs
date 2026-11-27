//! OS keychain access (SEC-01) and Linux Secret Service detection with a guided setup (SEC-11).
//!
//! macOS uses the login Keychain. Linux requires a running Secret Service (gnome-keyring, KeePassXC or
//! KWallet's Secret Service). There is intentionally no encrypted-file fallback: when no service is
//! available the app refuses to store secrets and shows the setup guide instead.

mod guide;

pub use guide::{linux_setup_guide, SetupGuide};

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

pub const SERVICE_NAME: &str = "io.backant.social-growth-helper";
const PROBE_KEY: &str = "__sgh_probe__";

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("no Secret Service is available: {0}. Secrets are not stored without one; follow the setup guide")]
    ServiceUnavailable(String),
    #[error("keychain error: {0}")]
    Backend(String),
}

pub trait SecretBackend: Send + Sync {
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError>;
    fn get(&self, key: &str) -> Result<Option<String>, SecretError>;
    fn delete(&self, key: &str) -> Result<(), SecretError>;
}

/// Real OS keychain via the `keyring` crate.
pub struct KeyringBackend {
    service: String,
}

impl KeyringBackend {
    pub fn new() -> Self {
        Self { service: SERVICE_NAME.to_string() }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&self.service, key).map_err(map_err)
    }
}

impl Default for KeyringBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn map_err(e: keyring::Error) -> SecretError {
    match e {
        keyring::Error::NoStorageAccess(inner) => SecretError::ServiceUnavailable(inner.to_string()),
        keyring::Error::PlatformFailure(inner) if cfg!(target_os = "linux") => {
            SecretError::ServiceUnavailable(inner.to_string())
        }
        other => SecretError::Backend(other.to_string()),
    }
}

impl SecretBackend for KeyringBackend {
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        let entry = self.entry(key)?;
        // With ad-hoc code signing on macOS, updating an existing login Keychain item in place
        // preserves the older build's CDHash ACL and can re-prompt on access. Deleting first so
        // `set_password` adds a fresh entry binds the ACL to the current binary.
        #[cfg(target_os = "macos")]
        {
            let _ = entry.delete_credential();
        }
        entry.set_password(value).map_err(map_err)
    }

    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        match self.entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_err(e)),
        }
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }
}

/// In-memory backend for tests; can simulate a missing Secret Service.
#[derive(Default)]
pub struct MemoryBackend {
    items: Mutex<HashMap<String, String>>,
    unavailable: Option<String>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn unavailable(reason: &str) -> Self {
        Self { items: Mutex::default(), unavailable: Some(reason.to_string()) }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.items.lock().unwrap().contains_key(key)
    }

    fn check(&self) -> Result<(), SecretError> {
        match &self.unavailable {
            Some(r) => Err(SecretError::ServiceUnavailable(r.clone())),
            None => Ok(()),
        }
    }
}

impl SecretBackend for MemoryBackend {
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.check()?;
        self.items.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        self.check()?;
        Ok(self.items.lock().unwrap().get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        self.check()?;
        self.items.lock().unwrap().remove(key);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Linux,
    Other,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Other
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SecretsStatus {
    pub platform: Platform,
    pub available: bool,
    pub backend: String,
    pub issue: Option<String>,
    pub guide: Option<SetupGuide>,
}

/// Round-trips a throwaway item to prove the keychain / Secret Service really works.
fn probe(backend: &dyn SecretBackend) -> Result<(), SecretError> {
    backend.set(PROBE_KEY, "ok")?;
    let read = backend.get(PROBE_KEY);
    let _ = backend.delete(PROBE_KEY);
    match read? {
        Some(v) if v == "ok" => Ok(()),
        _ => Err(SecretError::Backend("probe item could not be read back".into())),
    }
}

/// Pure detector: platform and session-bus presence are passed in so Linux behaviour is testable on a Mac.
///
/// On macOS, the login Keychain is a built-in OS subsystem, so we report it ready without running a
/// synthetic write/read/delete probe that would trigger macOS Keychain permission prompts during
/// navigation or diagnostics. Linux keeps the live Secret Service probe so missing/locked daemons
/// surface the setup guide with no encrypted-file fallback.
pub fn detect_status(platform: Platform, backend: &dyn SecretBackend, has_session_bus: bool) -> SecretsStatus {
    let backend_name = match platform {
        Platform::Macos => "macOS Keychain",
        Platform::Linux => "Secret Service (libsecret)",
        Platform::Other => "unsupported platform",
    }
    .to_string();

    let outcome: Result<(), String> = match platform {
        Platform::Macos => Ok(()),
        Platform::Other => Err("only macOS and Linux with a Secret Service are supported".into()),
        Platform::Linux if !has_session_bus => {
            Err("no D-Bus session bus found (DBUS_SESSION_BUS_ADDRESS is unset), so no Secret Service can be reached".into())
        }
        Platform::Linux => probe(backend).map_err(|e| e.to_string()),
    };

    match outcome {
        Ok(()) => SecretsStatus { platform, available: true, backend: backend_name, issue: None, guide: None },
        Err(issue) => SecretsStatus {
            platform,
            available: false,
            backend: backend_name,
            issue: Some(issue),
            guide: (platform == Platform::Linux).then(linux_setup_guide),
        },
    }
}

pub fn session_bus_present() -> bool {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").map(|v| !v.is_empty()).unwrap_or(false) {
        return true;
    }
    #[cfg(unix)]
    {
        // Same fallback libdbus uses: $XDG_RUNTIME_DIR/bus.
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            return std::path::Path::new(&dir).join("bus").exists();
        }
    }
    false
}

/// Keychain wrapper that refuses to store anything unless the OS secret store is available, and caches
/// secrets in-process after the first read or write so the OS keychain is not re-prompted repeatedly
/// during a session.
pub struct SecretsService {
    backend: Box<dyn SecretBackend>,
    platform: Platform,
    has_session_bus: bool,
    cache: Mutex<HashMap<String, String>>,
}

impl SecretsService {
    pub fn system() -> Self {
        Self {
            backend: Box::new(KeyringBackend::new()),
            platform: Platform::current(),
            has_session_bus: session_bus_present(),
            cache: Mutex::default(),
        }
    }

    pub fn with_backend(backend: Box<dyn SecretBackend>, platform: Platform, has_session_bus: bool) -> Self {
        Self { backend, platform, has_session_bus, cache: Mutex::default() }
    }

    pub fn status(&self) -> SecretsStatus {
        detect_status(self.platform, self.backend.as_ref(), self.has_session_bus)
    }

    fn require_platform_ready(&self) -> Result<(), SecretError> {
        match self.platform {
            Platform::Other => Err(SecretError::ServiceUnavailable(
                "only macOS and Linux with a Secret Service are supported".into(),
            )),
            Platform::Linux if !self.has_session_bus => Err(SecretError::ServiceUnavailable(
                "no D-Bus session bus found (DBUS_SESSION_BUS_ADDRESS is unset), so no Secret Service can be reached".into(),
            )),
            Platform::Macos | Platform::Linux => Ok(()),
        }
    }

    /// `key` is an opaque keychain_ref (for example `provider:anthropic_api`); SQLite stores only the ref.
    pub fn store(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.require_platform_ready()?;
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        self.backend.set(key, value)?;
        guard.insert(key.to_string(), value.to_string());
        Ok(())
    }

    pub fn load(&self, key: &str) -> Result<Option<String>, SecretError> {
        self.require_platform_ready()?;
        // Hold the cache lock across the cold-miss backend read so concurrent callers on the
        // first AI call serialize instead of stacking duplicate macOS Keychain password prompts.
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = guard.get(key) {
            return Ok(Some(cached.clone()));
        }
        let value = self.backend.get(key)?;
        if let Some(ref secret) = value {
            guard.insert(key.to_string(), secret.clone());
        }
        Ok(value)
    }

    pub fn remove(&self, key: &str) -> Result<(), SecretError> {
        self.require_platform_ready()?;
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        self.backend.delete(key)?;
        guard.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CountingBackend {
        inner: MemoryBackend,
        sets: AtomicUsize,
        gets: AtomicUsize,
        deletes: AtomicUsize,
    }

    impl CountingBackend {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                inner: MemoryBackend::new(),
                sets: AtomicUsize::new(0),
                gets: AtomicUsize::new(0),
                deletes: AtomicUsize::new(0),
            })
        }

        fn total_ops(&self) -> usize {
            self.sets.load(Ordering::SeqCst)
                + self.gets.load(Ordering::SeqCst)
                + self.deletes.load(Ordering::SeqCst)
        }
    }

    struct SharedCounting(Arc<CountingBackend>);

    impl SecretBackend for SharedCounting {
        fn set(&self, k: &str, v: &str) -> Result<(), SecretError> {
            self.0.sets.fetch_add(1, Ordering::SeqCst);
            self.0.inner.set(k, v)
        }

        fn get(&self, k: &str) -> Result<Option<String>, SecretError> {
            self.0.gets.fetch_add(1, Ordering::SeqCst);
            self.0.inner.get(k)
        }

        fn delete(&self, k: &str) -> Result<(), SecretError> {
            self.0.deletes.fetch_add(1, Ordering::SeqCst);
            self.0.inner.delete(k)
        }
    }

    #[test]
    fn available_service_stores_and_loads() {
        let svc = SecretsService::with_backend(Box::new(MemoryBackend::new()), Platform::Macos, true);
        assert!(svc.status().available);
        svc.store("provider:abc", "tok").unwrap();
        assert_eq!(svc.load("provider:abc").unwrap().as_deref(), Some("tok"));
        svc.remove("provider:abc").unwrap();
        assert_eq!(svc.load("provider:abc").unwrap(), None);
    }

    #[test]
    fn macos_status_does_not_touch_keychain() {
        let counter = CountingBackend::new();
        let st = detect_status(Platform::Macos, &SharedCounting(counter.clone()), true);
        assert!(st.available);
        assert!(st.guide.is_none());
        assert_eq!(counter.total_ops(), 0, "macOS status must not write/read/delete probe items");
    }

    #[test]
    fn linux_probe_leaves_no_residue() {
        let mem = Arc::new(MemoryBackend::new());
        struct Shared(Arc<MemoryBackend>);
        impl SecretBackend for Shared {
            fn set(&self, k: &str, v: &str) -> Result<(), SecretError> { self.0.set(k, v) }
            fn get(&self, k: &str) -> Result<Option<String>, SecretError> { self.0.get(k) }
            fn delete(&self, k: &str) -> Result<(), SecretError> { self.0.delete(k) }
        }
        let st = detect_status(Platform::Linux, &Shared(mem.clone()), true);
        assert!(st.available);
        assert!(!mem.contains(PROBE_KEY));
    }

    #[test]
    fn load_caches_in_process_and_never_runs_probe() {
        let counter = CountingBackend::new();
        counter.inner.set("provider:anthropic_api", "sk-ant-123").unwrap();
        let svc = SecretsService::with_backend(Box::new(SharedCounting(counter.clone())), Platform::Macos, true);

        assert_eq!(svc.load("provider:anthropic_api").unwrap().as_deref(), Some("sk-ant-123"));
        assert_eq!(svc.load("provider:anthropic_api").unwrap().as_deref(), Some("sk-ant-123"));
        assert_eq!(svc.load("provider:anthropic_api").unwrap().as_deref(), Some("sk-ant-123"));

        assert_eq!(counter.gets.load(Ordering::SeqCst), 1, "only the first load should read OS keychain");
        assert_eq!(counter.sets.load(Ordering::SeqCst), 0, "load must never write probe items");
        assert_eq!(counter.deletes.load(Ordering::SeqCst), 0, "load must never delete probe items");
    }

    #[test]
    fn store_populates_cache_and_remove_evicts_it() {
        let counter = CountingBackend::new();
        let svc = SecretsService::with_backend(Box::new(SharedCounting(counter.clone())), Platform::Macos, true);

        svc.store("provider:xai_api", "xai-key-1").unwrap();
        assert_eq!(svc.load("provider:xai_api").unwrap().as_deref(), Some("xai-key-1"));
        assert_eq!(counter.sets.load(Ordering::SeqCst), 1);
        assert_eq!(counter.gets.load(Ordering::SeqCst), 0, "store already cached the secret");

        svc.store("provider:xai_api", "xai-key-2").unwrap();
        assert_eq!(svc.load("provider:xai_api").unwrap().as_deref(), Some("xai-key-2"));
        assert_eq!(counter.gets.load(Ordering::SeqCst), 0);

        svc.remove("provider:xai_api").unwrap();
        assert_eq!(svc.load("provider:xai_api").unwrap(), None);
        assert_eq!(counter.deletes.load(Ordering::SeqCst), 1);
        assert_eq!(counter.gets.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_load_is_not_cached() {
        struct FlakyBackend {
            fail_first: std::sync::atomic::AtomicBool,
        }
        impl SecretBackend for FlakyBackend {
            fn set(&self, _k: &str, _v: &str) -> Result<(), SecretError> { Ok(()) }
            fn get(&self, _k: &str) -> Result<Option<String>, SecretError> {
                if self.fail_first.swap(false, Ordering::SeqCst) {
                    Err(SecretError::Backend("user canceled".into()))
                } else {
                    Ok(Some("sk-ok".into()))
                }
            }
            fn delete(&self, _k: &str) -> Result<(), SecretError> { Ok(()) }
        }
        let svc = SecretsService::with_backend(
            Box::new(FlakyBackend { fail_first: std::sync::atomic::AtomicBool::new(true) }),
            Platform::Macos,
            true,
        );
        assert!(svc.load("provider:anthropic_api").is_err());
        assert_eq!(svc.load("provider:anthropic_api").unwrap().as_deref(), Some("sk-ok"));
    }

    #[test]
    fn concurrent_first_loads_only_touch_keychain_once() {
        use std::sync::Barrier;
        use std::time::Duration;

        struct SlowCountingBackend {
            gets: AtomicUsize,
        }
        impl SecretBackend for SlowCountingBackend {
            fn set(&self, _k: &str, _v: &str) -> Result<(), SecretError> { Ok(()) }
            fn get(&self, _k: &str) -> Result<Option<String>, SecretError> {
                self.gets.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(25));
                Ok(Some("sk-shared".into()))
            }
            fn delete(&self, _k: &str) -> Result<(), SecretError> { Ok(()) }
        }
        struct SharedSlow(Arc<SlowCountingBackend>);
        impl SecretBackend for SharedSlow {
            fn set(&self, k: &str, v: &str) -> Result<(), SecretError> { self.0.set(k, v) }
            fn get(&self, k: &str) -> Result<Option<String>, SecretError> { self.0.get(k) }
            fn delete(&self, k: &str) -> Result<(), SecretError> { self.0.delete(k) }
        }

        let backend = Arc::new(SlowCountingBackend { gets: AtomicUsize::new(0) });
        let svc = Arc::new(SecretsService::with_backend(
            Box::new(SharedSlow(backend.clone())),
            Platform::Macos,
            true,
        ));
        let barrier = Arc::new(Barrier::new(4));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let svc = svc.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    svc.load("provider:anthropic_api").unwrap()
                })
            })
            .collect();

        for h in handles {
            assert_eq!(h.join().unwrap().as_deref(), Some("sk-shared"));
        }
        assert_eq!(
            backend.gets.load(Ordering::SeqCst),
            1,
            "concurrent cold loads for the same key must coalesce into one OS keychain read"
        );
    }

    #[test]
    fn linux_without_secret_service_refuses_and_shows_guide() {
        let svc = SecretsService::with_backend(
            Box::new(MemoryBackend::unavailable("org.freedesktop.secrets was not provided by any .service files")),
            Platform::Linux,
            true,
        );
        let st = svc.status();
        assert!(!st.available);
        assert!(st.issue.unwrap().contains("org.freedesktop.secrets"));
        let guide = st.guide.expect("guide is shown on Linux");
        assert!(guide.no_file_fallback);
        let err = svc.store("provider:x", "secret").unwrap_err();
        assert!(matches!(err, SecretError::ServiceUnavailable(_)));
        assert!(svc.load("provider:x").is_err());
    }

    #[test]
    fn linux_without_session_bus_is_reported_before_probing() {
        let svc = SecretsService::with_backend(Box::new(MemoryBackend::new()), Platform::Linux, false);
        let st = svc.status();
        assert!(!st.available);
        assert!(st.issue.unwrap().contains("D-Bus"));
        assert!(svc.store("k", "v").is_err(), "healthy backend must still be refused without a bus");
    }

    #[test]
    fn linux_with_service_running_works() {
        let svc = SecretsService::with_backend(Box::new(MemoryBackend::new()), Platform::Linux, true);
        let st = svc.status();
        assert!(st.available && st.guide.is_none());
        svc.store("k", "v").unwrap();
    }

    #[test]
    fn unsupported_platform_is_unavailable() {
        let st = detect_status(Platform::Other, &MemoryBackend::new(), true);
        assert!(!st.available);
        assert!(st.guide.is_none());
    }
}
