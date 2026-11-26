//! Chrome discovery and manual override (BRW-05). Playwright's bundled Chromium is rejected on purpose:
//! its TLS fingerprint and missing proprietary codecs are what X flags.

use super::BrowserError;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserKind {
    Chrome,
    Chromium,
    Brave,
    Custom,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BrowserInstall {
    pub path: PathBuf,
    pub kind: BrowserKind,
    pub from_override: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Macos,
    Linux,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }
}

fn kind_for(path: &Path) -> BrowserKind {
    let s = path.to_string_lossy().to_lowercase();
    if s.contains("brave") {
        BrowserKind::Brave
    } else if s.contains("chromium") {
        BrowserKind::Chromium
    } else if s.contains("chrome") {
        BrowserKind::Chrome
    } else {
        BrowserKind::Custom
    }
}

pub fn is_playwright_build(path: &Path) -> bool {
    path.to_string_lossy().to_lowercase().contains("playwright")
}

/// Absolute candidates in priority order (stable Chrome first, then Chromium-class fallbacks).
pub fn absolute_candidates(os: Os, home: Option<&Path>) -> Vec<PathBuf> {
    match os {
        Os::Macos => {
            let mut v = vec![PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")];
            if let Some(h) = home {
                v.push(h.join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
            }
            v.push(PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"));
            v.push(PathBuf::from("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"));
            v
        }
        Os::Linux => vec![],
    }
}

pub const LINUX_BINARY_NAMES: &[&str] = &[
    "google-chrome-stable",
    "google-chrome",
    "chromium",
    "chromium-browser",
    "brave-browser",
];

pub fn discover(
    os: Os,
    home: Option<&Path>,
    override_path: Option<&Path>,
    is_file: &dyn Fn(&Path) -> bool,
    which: &dyn Fn(&str) -> Option<PathBuf>,
) -> Result<BrowserInstall, BrowserError> {
    if let Some(p) = override_path {
        if is_playwright_build(p) {
            return Err(BrowserError::BinaryNotFound(
                "The configured path is a Playwright browser build, which is not supported. Point to a normal Chrome, Chromium or Brave install.".into(),
            ));
        }
        if !is_file(p) {
            return Err(BrowserError::BinaryNotFound(format!("The configured browser path does not exist: {}.", p.display())));
        }
        return Ok(BrowserInstall { path: p.to_path_buf(), kind: kind_for(p), from_override: true });
    }

    let mut candidates = absolute_candidates(os, home);
    if os == Os::Linux {
        candidates.extend(LINUX_BINARY_NAMES.iter().filter_map(|n| which(n)));
    }
    for c in candidates {
        if !is_playwright_build(&c) && is_file(&c) {
            return Ok(BrowserInstall { kind: kind_for(&c), path: c, from_override: false });
        }
    }

    let hint = match os {
        Os::Macos => "Install Google Chrome from https://www.google.com/chrome/.",
        Os::Linux => "Install google-chrome-stable, chromium or brave-browser (for example: sudo apt install chromium, or sudo dnf install chromium).",
    };
    Err(BrowserError::BinaryNotFound(hint.into()))
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|d| d.join(name)).find(|p| p.is_file())
}

pub fn discover_system(override_path: Option<&Path>) -> Result<BrowserInstall, BrowserError> {
    let home = dirs::home_dir();
    discover(Os::current(), home.as_deref(), override_path, &|p| p.is_file(), &find_in_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn run(os: Os, existing: &[&str], over: Option<&str>) -> Result<BrowserInstall, BrowserError> {
        let set: HashSet<PathBuf> = existing.iter().map(PathBuf::from).collect();
        let set2 = set.clone();
        discover(
            os,
            Some(Path::new("/Users/p")),
            over.map(Path::new),
            &move |p| set.contains(p),
            &move |n| {
                let c = PathBuf::from(format!("/usr/bin/{n}"));
                set2.contains(&c).then_some(c)
            },
        )
    }

    #[test]
    fn mac_prefers_stable_chrome_over_brave() {
        let r = run(
            Os::Macos,
            &[
                "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ],
            None,
        )
        .unwrap();
        assert_eq!(r.kind, BrowserKind::Chrome);
        assert!(!r.from_override);
    }

    #[test]
    fn mac_falls_back_to_brave() {
        let r = run(Os::Macos, &["/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"], None).unwrap();
        assert_eq!(r.kind, BrowserKind::Brave);
    }

    #[test]
    fn linux_follows_documented_order() {
        let r = run(Os::Linux, &["/usr/bin/chromium", "/usr/bin/google-chrome"], None).unwrap();
        assert_eq!(r.path, PathBuf::from("/usr/bin/google-chrome"));
        let r = run(Os::Linux, &["/usr/bin/brave-browser", "/usr/bin/chromium-browser"], None).unwrap();
        assert_eq!(r.path, PathBuf::from("/usr/bin/chromium-browser"));
    }

    #[test]
    fn override_wins_and_is_validated() {
        let r = run(Os::Linux, &["/opt/mychrome/chrome", "/usr/bin/google-chrome"], Some("/opt/mychrome/chrome")).unwrap();
        assert!(r.from_override);
        assert!(run(Os::Linux, &["/usr/bin/google-chrome"], Some("/nope/chrome")).is_err());
    }

    #[test]
    fn playwright_chromium_is_rejected() {
        let pw = "/Users/p/Library/Caches/ms-playwright/chromium-1100/chrome-mac/Chromium.app/Contents/MacOS/Chromium";
        assert!(run(Os::Macos, &[pw], Some(pw)).is_err());
    }

    #[test]
    fn nothing_installed_gives_guided_message() {
        let e = run(Os::Macos, &[], None).unwrap_err().to_string();
        assert!(e.contains("Install Google Chrome"));
        let e = run(Os::Linux, &[], None).unwrap_err().to_string();
        assert!(e.contains("apt install") && e.contains("dnf install"));
    }
}
