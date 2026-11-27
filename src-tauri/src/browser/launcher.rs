//! Clean Chrome launch (BRW-01), loopback port allocation (SEC-03) and stale-process reclaim (BRW-16).

use super::{BrowserError, ChromeLaunchConfig};
use serde::{Deserialize, Serialize};
use std::io;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

/// Flag prefixes that must never appear on the Chrome command line. Their absence keeps
/// `navigator.webdriver` naturally false and the fingerprint untouched.
pub const FORBIDDEN_FLAGS: &[&str] = &[
    "--enable-automation",
    "--headless",
    "--disable-blink-features",
    "--disable-web-security",
    "--user-agent",
    "--no-sandbox",
];

pub fn allocate_loopback_port() -> Result<u16, BrowserError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

pub fn build_chrome_args(cfg: &ChromeLaunchConfig, debug_port: u16) -> Vec<String> {
    if cfg.invisible {
        return vec![
            format!("--user-data-dir={}", cfg.profile_dir.display()),
            "--headless=new".to_string(),
            "--hide-scrollbars".to_string(),
            "--mute-audio".to_string(),
            format!("--remote-debugging-port={debug_port}"),
            "--remote-debugging-address=127.0.0.1".to_string(),
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
            format!("--window-size={},{}", cfg.width, cfg.height),
            cfg.start_url.clone(),
        ];
    }
    vec![
        format!("--user-data-dir={}", cfg.profile_dir.display()),
        format!("--app={}", cfg.start_url),
        format!("--remote-debugging-port={debug_port}"),
        "--remote-debugging-address=127.0.0.1".to_string(),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        format!("--window-size={},{}", cfg.width, cfg.height),
    ]
}

/// Handle to a spawned Chrome process.
///
/// On macOS this wraps a direct `posix_spawnp` PID launched with TCC responsibility disclaimed
/// (`responsibility_spawnattrs_setdisclaim`), so macOS attributes Chrome's own bundle/TCC checks
/// (such as App Management `kTCCServiceSystemPolicyAppBundles`) to Chrome instead of this app.
#[cfg(target_os = "macos")]
pub struct SpawnedChrome {
    pid: libc::pid_t,
    status: Option<ExitStatus>,
}

#[cfg(not(target_os = "macos"))]
pub struct SpawnedChrome(std::process::Child);

#[cfg(target_os = "macos")]
impl SpawnedChrome {
    pub fn id(&self) -> u32 {
        self.pid as u32
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        use std::os::unix::process::ExitStatusExt;

        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        let mut raw: libc::c_int = 0;
        loop {
            let rc = unsafe { libc::waitpid(self.pid, &mut raw, libc::WNOHANG) };
            if rc == 0 {
                return Ok(None);
            }
            if rc == self.pid {
                let status = ExitStatus::from_raw(raw);
                self.status = Some(status);
                return Ok(Some(status));
            }
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
    }

    pub fn kill(&mut self) -> io::Result<()> {
        if self.status.is_some() {
            return Ok(());
        }
        let rc = unsafe { libc::kill(self.pid, libc::SIGKILL) };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(err)
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        use std::os::unix::process::ExitStatusExt;

        if let Some(status) = self.status {
            return Ok(status);
        }
        let mut raw: libc::c_int = 0;
        loop {
            let rc = unsafe { libc::waitpid(self.pid, &mut raw, 0) };
            if rc == self.pid {
                let status = ExitStatus::from_raw(raw);
                self.status = Some(status);
                return Ok(status);
            }
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl SpawnedChrome {
    pub fn id(&self) -> u32 {
        self.0.id()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.0.try_wait()
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.0.kill()
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.0.wait()
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::SpawnedChrome;
    use std::ffi::{CString, OsStr};
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    struct SpawnAttr(libc::posix_spawnattr_t);

    impl SpawnAttr {
        fn new() -> io::Result<Self> {
            let mut raw: libc::posix_spawnattr_t = std::ptr::null_mut();
            let rc = unsafe { libc::posix_spawnattr_init(&mut raw) };
            if rc != 0 {
                return Err(io::Error::from_raw_os_error(rc));
            }
            Ok(Self(raw))
        }

        fn as_mut_ptr(&mut self) -> *mut libc::posix_spawnattr_t {
            &mut self.0
        }

        fn as_ptr(&self) -> *const libc::posix_spawnattr_t {
            &self.0
        }
    }

    impl Drop for SpawnAttr {
        fn drop(&mut self) {
            unsafe {
                libc::posix_spawnattr_destroy(&mut self.0);
            }
        }
    }

    struct FileActions(libc::posix_spawn_file_actions_t);

    impl FileActions {
        fn new() -> io::Result<Self> {
            let mut raw: libc::posix_spawn_file_actions_t = std::ptr::null_mut();
            let rc = unsafe { libc::posix_spawn_file_actions_init(&mut raw) };
            if rc != 0 {
                return Err(io::Error::from_raw_os_error(rc));
            }
            Ok(Self(raw))
        }

        fn as_mut_ptr(&mut self) -> *mut libc::posix_spawn_file_actions_t {
            &mut self.0
        }

        fn as_ptr(&self) -> *const libc::posix_spawn_file_actions_t {
            &self.0
        }
    }

    impl Drop for FileActions {
        fn drop(&mut self) {
            unsafe {
                libc::posix_spawn_file_actions_destroy(&mut self.0);
            }
        }
    }

    fn os_to_cstring(s: &OsStr) -> io::Result<CString> {
        CString::new(s.as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "argument contains interior NUL"))
    }

    pub(super) fn disclaim_responsibility(attr: *mut libc::posix_spawnattr_t) -> Result<(), String> {
        type DisclaimFn = unsafe extern "C" fn(*mut libc::posix_spawnattr_t, libc::c_int) -> libc::c_int;
        let sym = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"responsibility_spawnattrs_setdisclaim".as_ptr()) };
        if sym.is_null() {
            return Err("responsibility_spawnattrs_setdisclaim symbol not found".to_string());
        }
        let f: DisclaimFn = unsafe { std::mem::transmute(sym) };
        let rc = unsafe { f(attr, 1) };
        if rc != 0 {
            return Err(format!("responsibility_spawnattrs_setdisclaim failed with status {rc}"));
        }
        Ok(())
    }

    pub(super) fn spawn_disclaimed(program: &Path, args: &[String]) -> io::Result<SpawnedChrome> {
        let c_program = os_to_cstring(program.as_os_str())?;
        let mut argv: Vec<CString> = Vec::with_capacity(args.len() + 1);
        argv.push(c_program.clone());
        for arg in args {
            argv.push(os_to_cstring(OsStr::new(arg))?);
        }
        let argv_ptrs: Vec<*mut libc::c_char> = argv
            .iter()
            .map(|s| s.as_ptr() as *mut libc::c_char)
            .chain(std::iter::once(std::ptr::null_mut()))
            .collect();

        let envp: Vec<CString> = std::env::vars_os()
            .filter_map(|(k, v)| {
                let mut entry = Vec::with_capacity(k.len() + 1 + v.len());
                entry.extend_from_slice(k.as_bytes());
                entry.push(b'=');
                entry.extend_from_slice(v.as_bytes());
                CString::new(entry).ok()
            })
            .collect();
        let envp_ptrs: Vec<*mut libc::c_char> = envp
            .iter()
            .map(|s| s.as_ptr() as *mut libc::c_char)
            .chain(std::iter::once(std::ptr::null_mut()))
            .collect();

        let mut attr = SpawnAttr::new()?;
        let flags = (libc::POSIX_SPAWN_CLOEXEC_DEFAULT | libc::POSIX_SPAWN_SETSIGDEF | libc::POSIX_SPAWN_SETSIGMASK) as libc::c_short;
        let rc = unsafe { libc::posix_spawnattr_setflags(attr.as_mut_ptr(), flags) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
        }

        let mut empty_mask: libc::sigset_t = 0;
        unsafe { libc::sigemptyset(&mut empty_mask) };
        let rc = unsafe { libc::posix_spawnattr_setsigmask(attr.as_mut_ptr(), &empty_mask) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
        }

        let mut sigdef: libc::sigset_t = 0;
        unsafe {
            libc::sigemptyset(&mut sigdef);
            libc::sigaddset(&mut sigdef, libc::SIGPIPE);
        }
        let rc = unsafe { libc::posix_spawnattr_setsigdefault(attr.as_mut_ptr(), &sigdef) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
        }

        if let Err(err) = disclaim_responsibility(attr.as_mut_ptr()) {
            log::warn!("unable to disclaim macOS TCC responsibility for Chrome: {err}");
        }

        let mut actions = FileActions::new()?;
        for (fd, oflag) in [(0, libc::O_RDONLY), (1, libc::O_WRONLY), (2, libc::O_WRONLY)] {
            let rc = unsafe {
                libc::posix_spawn_file_actions_addopen(actions.as_mut_ptr(), fd, c"/dev/null".as_ptr(), oflag, 0)
            };
            if rc != 0 {
                return Err(io::Error::from_raw_os_error(rc));
            }
        }

        let mut pid: libc::pid_t = 0;
        let rc = unsafe {
            libc::posix_spawnp(
                &mut pid,
                c_program.as_ptr(),
                actions.as_ptr(),
                attr.as_ptr(),
                argv_ptrs.as_ptr(),
                envp_ptrs.as_ptr(),
            )
        };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
        }
        Ok(SpawnedChrome { pid, status: None })
    }
}

/// Spawns genuine Chrome with zero automation flags.
pub fn spawn_clean_chrome(cfg: &ChromeLaunchConfig, debug_port: u16) -> Result<SpawnedChrome, BrowserError> {
    std::fs::create_dir_all(&cfg.profile_dir)?;
    let args = build_chrome_args(cfg, debug_port);
    #[cfg(target_os = "macos")]
    {
        Ok(macos::spawn_disclaimed(&cfg.binary_path, &args)?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(SpawnedChrome(
            Command::new(&cfg.binary_path)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StaleRecord {
    pub pid: u32,
    pub port: u16,
    pub profile_dir: String,
}

/// Sibling file next to the profile directory; it never lives inside Chrome's own profile.
pub fn record_path(profile_dir: &Path) -> PathBuf {
    let name = profile_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "profile".into());
    profile_dir.with_file_name(format!("{name}.chrome.json"))
}

pub fn write_record(rec: &StaleRecord) -> Result<(), BrowserError> {
    let path = record_path(Path::new(&rec.profile_dir));
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, serde_json::to_vec(rec).expect("record serializes"))?;
    Ok(())
}

pub fn read_record(profile_dir: &Path) -> Option<StaleRecord> {
    serde_json::from_slice(&std::fs::read(record_path(profile_dir)).ok()?).ok()
}

pub fn clear_record(profile_dir: &Path) {
    let _ = std::fs::remove_file(record_path(profile_dir));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimOutcome {
    NothingToReclaim,
    ClosedGracefully,
    Killed,
}

#[cfg(unix)]
pub fn process_command_line(pid: u32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-ww", "-p", &pid.to_string(), "-o", "state=,command="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let (state, cmd) = s.trim().split_once(char::is_whitespace)?;
    if state.starts_with('Z') {
        return None;
    }
    let cmd = cmd.trim();
    (!cmd.is_empty()).then(|| cmd.to_string())
}

#[cfg(not(unix))]
pub fn process_command_line(_pid: u32) -> Option<String> {
    None
}

#[cfg(unix)]
fn signal(pid: u32, sig: &str) {
    let _ = Command::new("kill").args([sig, &pid.to_string()]).stderr(Stdio::null()).status();
}

#[cfg(not(unix))]
fn signal(_pid: u32, _sig: &str) {}

async fn wait_for_exit(pid: u32, within: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + within;
    while tokio::time::Instant::now() < deadline {
        if process_command_line(pid).is_none() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    process_command_line(pid).is_none()
}

/// Closes a Chrome left behind by a crashed or force-killed app, but only if the recorded pid still
/// belongs to a process running with this exact `--user-data-dir` (never an unrelated process).
pub async fn reclaim_stale_chrome(profile_dir: &Path) -> ReclaimOutcome {
    let Some(rec) = read_record(profile_dir) else {
        return ReclaimOutcome::NothingToReclaim;
    };
    let owns = process_command_line(rec.pid)
        .map(|cmd| cmd.contains(&format!("--user-data-dir={}", profile_dir.display())) || cmd.contains(&rec.profile_dir))
        .unwrap_or(false);
    if !owns {
        clear_record(profile_dir);
        return ReclaimOutcome::NothingToReclaim;
    }

    if let Ok(v) = super::http::get_json(rec.port, "/json/version", Duration::from_secs(2)).await {
        if let Some(url) = v["webSocketDebuggerUrl"].as_str() {
            if let Ok(session) = super::cdp::CleanCdpSession::connect(url).await {
                let _ = session.send("Browser.close", serde_json::json!({}), None).await;
            }
        }
        if wait_for_exit(rec.pid, Duration::from_secs(8)).await {
            clear_record(profile_dir);
            return ReclaimOutcome::ClosedGracefully;
        }
    }

    signal(rec.pid, "-TERM");
    if !wait_for_exit(rec.pid, Duration::from_secs(4)).await {
        signal(rec.pid, "-KILL");
        wait_for_exit(rec.pid, Duration::from_secs(2)).await;
    }
    clear_record(profile_dir);
    ReclaimOutcome::Killed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ChromeLaunchConfig {
        ChromeLaunchConfig {
            binary_path: "/x/chrome".into(),
            profile_dir: "/data/browser-profiles/x-default".into(),
            start_url: "https://x.com/home".into(),
            width: 1100,
            height: 900,
            invisible: false,
        }
    }

    #[test]
    fn invisible_args_use_new_headless_without_app_window() {
        let args = build_chrome_args(&ChromeLaunchConfig { invisible: true, ..cfg() }, 4242);
        assert!(args.contains(&"--headless=new".to_string()));
        assert!(args.contains(&"--hide-scrollbars".to_string()));
        assert!(args.contains(&"--mute-audio".to_string()));
        assert!(!args.iter().any(|a| a.starts_with("--app=")));
        assert!(args.contains(&"https://x.com/home".to_string()));
    }

    #[test]
    fn args_are_clean_and_loopback_only() {
        let args = build_chrome_args(&cfg(), 4242);
        for flag in FORBIDDEN_FLAGS {
            assert!(!args.iter().any(|a| a.starts_with(flag)), "forbidden flag {flag} present");
        }
        assert!(args.contains(&"--app=https://x.com/home".to_string()));
        assert!(args.contains(&"--user-data-dir=/data/browser-profiles/x-default".to_string()));
        assert!(args.contains(&"--remote-debugging-port=4242".to_string()));
        assert!(args.contains(&"--remote-debugging-address=127.0.0.1".to_string()));
        assert!(args.contains(&"--no-first-run".to_string()));
        assert!(args.contains(&"--no-default-browser-check".to_string()));
    }

    #[test]
    fn allocated_port_is_free_and_nonzero() {
        let p = allocate_loopback_port().unwrap();
        assert!(p > 0);
        TcpListener::bind(("127.0.0.1", p)).unwrap();
    }

    #[test]
    fn record_lives_beside_not_inside_the_profile() {
        let p = Path::new("/data/browser-profiles/x-default");
        let r = record_path(p);
        assert_eq!(r, PathBuf::from("/data/browser-profiles/x-default.chrome.json"));
        assert!(!r.starts_with(p));
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::process::Child;

        fn spawn_marker(profile: &Path) -> Child {
            // Use the POSIX `read` builtin with a piped stdin rather than `sleep`: on macOS,
            // `/bin/sh` (bash) tail-call `exec`s a single external command like `sleep 120`,
            // replacing argv and dropping `--user-data-dir=...` before `ps` observes it.
            Command::new("sh")
                .args([
                    "-c",
                    "read _",
                    "chrome-test-marker",
                    &format!("--user-data-dir={}", profile.display()),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        }

        #[tokio::test]
        async fn stale_process_owning_the_profile_is_killed() {
            let dir = tempfile::tempdir().unwrap();
            let profile = dir.path().join("x-default");
            std::fs::create_dir_all(&profile).unwrap();
            let mut child = spawn_marker(&profile);
            let marker = format!("--user-data-dir={}", profile.display());
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            while tokio::time::Instant::now() < deadline {
                if process_command_line(child.id())
                    .map(|cmd| cmd.contains(&marker))
                    .unwrap_or(false)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            write_record(&StaleRecord { pid: child.id(), port: 1, profile_dir: profile.display().to_string() }).unwrap();

            assert_eq!(reclaim_stale_chrome(&profile).await, ReclaimOutcome::Killed);
            let status = tokio::task::spawn_blocking(move || child.wait()).await.unwrap().unwrap();
            assert!(!status.success(), "process was terminated by a signal");
            assert!(read_record(&profile).is_none());
        }

        #[tokio::test]
        async fn unrelated_process_is_left_alone() {
            let dir = tempfile::tempdir().unwrap();
            let profile = dir.path().join("x-default");
            let mut other = Command::new("sleep").arg("30").spawn().unwrap();
            write_record(&StaleRecord { pid: other.id(), port: 1, profile_dir: profile.display().to_string() }).unwrap();

            assert_eq!(reclaim_stale_chrome(&profile).await, ReclaimOutcome::NothingToReclaim);
            assert!(other.try_wait().unwrap().is_none(), "unrelated process must survive");
            assert!(read_record(&profile).is_none(), "stale record is cleared");
            other.kill().unwrap();
            let _ = other.wait();
        }

        #[tokio::test]
        async fn no_record_means_nothing_to_do() {
            let dir = tempfile::tempdir().unwrap();
            assert_eq!(reclaim_stale_chrome(&dir.path().join("p")).await, ReclaimOutcome::NothingToReclaim);
        }
    }

    #[cfg(target_os = "macos")]
    mod macos_tests {
        use super::*;

        #[test]
        fn responsibility_disclaim_succeeds_on_macos() {
            let mut raw: libc::posix_spawnattr_t = std::ptr::null_mut();
            assert_eq!(unsafe { libc::posix_spawnattr_init(&mut raw) }, 0);
            let res = macos::disclaim_responsibility(&mut raw);
            unsafe { libc::posix_spawnattr_destroy(&mut raw) };
            assert!(res.is_ok(), "expected macOS disclaim symbol to succeed: {res:?}");
        }

        #[test]
        fn disclaimed_spawn_passes_args_and_handles_lifecycle() {
            let dir = tempfile::tempdir().unwrap();
            let marker_file = dir.path().join("args.txt");
            let script = format!("printf '%s|%s' \"$1\" \"$2\" > \"{}\"", marker_file.display());
            let mut child = macos::spawn_disclaimed(
                Path::new("/bin/sh"),
                &["-c".into(), script, "arg0".into(), "first".into(), "second".into()],
            )
            .unwrap();
            assert!(child.id() > 0);
            let status = child.wait().unwrap();
            assert!(status.success(), "expected clean exit: {status}");
            assert_eq!(child.try_wait().unwrap(), Some(status), "cached exit status must be idempotent");
            assert_eq!(child.wait().unwrap(), status);
            child.kill().unwrap();
            assert_eq!(std::fs::read_to_string(&marker_file).unwrap(), "first|second");
        }

        #[test]
        fn disclaimed_spawn_try_wait_and_kill_running_process() {
            let mut child = macos::spawn_disclaimed(Path::new("/bin/sleep"), &["30".into()]).unwrap();
            assert!(child.try_wait().unwrap().is_none(), "running process should not have exited yet");
            child.kill().unwrap();
            let status = child.wait().unwrap();
            assert!(!status.success(), "killed process must not report success");
            assert_eq!(child.try_wait().unwrap(), Some(status));
        }

        #[test]
        fn disclaimed_spawn_missing_binary_errors() {
            let err = macos::spawn_disclaimed(Path::new("/definitely/missing/chrome-bin"), &[])
                .err()
                .expect("missing binary should fail");
            assert_eq!(err.kind(), io::ErrorKind::NotFound);
        }
    }
}
