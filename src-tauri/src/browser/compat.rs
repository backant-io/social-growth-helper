//! Chrome version compatibility check (BRW-15). The CDP surface and X's bot heuristics move with Chrome, so a
//! build older than the minimum tested one gets a visible warning instead of silent flakiness.

use serde::Serialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Oldest Chromium major the app has been exercised against. Phase 0 and Phase 1 runs used Chrome 157 and 158.
pub const MIN_TESTED_CHROME_MAJOR: u32 = 140;

/// Inclusive major-version ranges known to break CDP input or the clean launch. Empty until one is found.
pub const KNOWN_INCOMPATIBLE: &[(u32, u32, &str)] = &[];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeVersion {
    pub major: u32,
    pub full: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatStatus {
    Ok,
    TooOld,
    Incompatible,
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VersionCheck {
    pub version: Option<String>,
    pub major: Option<u32>,
    pub min_tested_major: u32,
    pub status: CompatStatus,
    pub warning: Option<String>,
}

/// Extracts the dotted version from `--version` output ("Google Chrome 157.0.8103.50", "Chromium 126.0.6478.126
/// built on Ubuntu") or a CDP product string ("Chrome/157.0.8103.50").
pub fn parse_version(output: &str) -> Option<ChromeVersion> {
    output
        .split(|c: char| c.is_whitespace() || c == '/')
        .find(|tok| {
            let parts: Vec<&str> = tok.split('.').collect();
            parts.len() >= 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        })
        .and_then(|tok| Some(ChromeVersion { major: tok.split('.').next()?.parse().ok()?, full: tok.to_string() }))
}

pub fn assess(version: Option<ChromeVersion>) -> VersionCheck {
    let min = MIN_TESTED_CHROME_MAJOR;
    let Some(v) = version else {
        return VersionCheck {
            version: None,
            major: None,
            min_tested_major: min,
            status: CompatStatus::Unknown,
            warning: Some(format!(
                "Could not read the browser version. The app is tested with Chrome {min} or newer; if the X window misbehaves, update Chrome."
            )),
        };
    };
    if let Some((_, _, why)) = KNOWN_INCOMPATIBLE.iter().find(|(lo, hi, _)| (*lo..=*hi).contains(&v.major)) {
        return VersionCheck {
            warning: Some(format!("Chrome {} is known to be incompatible: {why}. Update or switch browser.", v.full)),
            version: Some(v.full),
            major: Some(v.major),
            min_tested_major: min,
            status: CompatStatus::Incompatible,
        };
    }
    if v.major < min {
        return VersionCheck {
            warning: Some(format!(
                "Chrome {} is older than the minimum tested version ({min}). Update Chrome before relying on the X window.",
                v.full
            )),
            version: Some(v.full),
            major: Some(v.major),
            min_tested_major: min,
            status: CompatStatus::TooOld,
        };
    }
    VersionCheck { version: Some(v.full), major: Some(v.major), min_tested_major: min, status: CompatStatus::Ok, warning: None }
}

/// Runs `<binary> --version` with a short timeout; any failure yields `None` (reported as `Unknown`).
pub fn detect(binary: &Path) -> Option<ChromeVersion> {
    let mut child = Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        match child.try_wait().ok()? {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    parse_version(&out)
}

pub fn check_binary(binary: &Path) -> VersionCheck {
    assess(detect(binary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_version_strings() {
        assert_eq!(parse_version("Google Chrome 157.0.8103.50 \n").unwrap().major, 157);
        let v = parse_version("Chromium 126.0.6478.126 built on Ubuntu , running on Ubuntu 24.04").unwrap();
        assert_eq!((v.major, v.full.as_str()), (126, "126.0.6478.126"));
        assert_eq!(parse_version("Brave Browser 142.1.84.130").unwrap().major, 142);
        assert_eq!(parse_version("Chrome/158.0.8200.1").unwrap().full, "158.0.8200.1");
    }

    #[test]
    fn rejects_text_without_a_version() {
        assert!(parse_version("").is_none());
        assert!(parse_version("Google Chrome").is_none());
        assert!(parse_version("build 12.3").is_none());
    }

    #[test]
    fn old_chrome_warns_below_the_minimum() {
        let c = assess(parse_version("Google Chrome 120.0.6099.109"));
        assert_eq!(c.status, CompatStatus::TooOld);
        let w = c.warning.unwrap();
        assert!(w.contains("120.0.6099.109") && w.contains(&MIN_TESTED_CHROME_MAJOR.to_string()));
    }

    #[test]
    fn minimum_and_newer_are_fine() {
        let at_min = assess(parse_version(&format!("Google Chrome {MIN_TESTED_CHROME_MAJOR}.0.1.2")));
        assert_eq!(at_min.status, CompatStatus::Ok);
        assert!(at_min.warning.is_none());
        assert_eq!(assess(parse_version("Google Chrome 158.0.8200.1")).status, CompatStatus::Ok);
    }

    #[test]
    fn unreadable_version_is_unknown_with_a_hint() {
        let c = assess(None);
        assert_eq!(c.status, CompatStatus::Unknown);
        assert!(c.warning.is_some() && c.version.is_none());
    }

    #[test]
    fn missing_binary_is_unknown() {
        assert_eq!(check_binary(Path::new("/definitely/not/chrome")).status, CompatStatus::Unknown);
    }

    #[cfg(unix)]
    #[test]
    fn detects_through_a_stub_binary() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("fake-chrome");
        std::fs::write(&bin, "#!/bin/sh\necho 'Google Chrome 99.0.1.2'\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let c = check_binary(&bin);
        assert_eq!(c.version.as_deref(), Some("99.0.1.2"));
        assert_eq!(c.status, CompatStatus::TooOld);
    }
}
