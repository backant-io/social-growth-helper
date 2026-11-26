//! Clean Chrome launch (BRW-01), loopback port allocation (SEC-03) and stale-process reclaim (BRW-16).

use super::{BrowserError, ChromeLaunchConfig};
use serde::{Deserialize, Serialize};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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

/// Spawns genuine Chrome with zero automation flags.
pub fn spawn_clean_chrome(cfg: &ChromeLaunchConfig, debug_port: u16) -> Result<Child, BrowserError> {
    std::fs::create_dir_all(&cfg.profile_dir)?;
    Ok(Command::new(&cfg.binary_path)
        .args(build_chrome_args(cfg, debug_port))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?)
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
        }
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
}
