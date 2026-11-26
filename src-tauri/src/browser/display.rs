//! Display-server detection (BRW-14). Wayland does not let a client position its own or another process's
//! top-level windows, so coordinate pinning is skipped there and the user gets a hint instead of an error.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayServer {
    Macos,
    X11,
    Wayland,
    Unknown,
}

impl DisplayServer {
    pub fn supports_window_placement(self) -> bool {
        matches!(self, DisplayServer::Macos | DisplayServer::X11)
    }

    pub fn placement_hint(self) -> Option<&'static str> {
        match self {
            DisplayServer::Wayland => Some(
                "You are on Wayland, which does not let apps position windows. Arrange the app and the X window side by side yourself, or switch between them with Alt+Tab. Nothing is broken.",
            ),
            DisplayServer::Unknown => Some(
                "The display server could not be detected, so automatic window placement is off. Arrange the app and the X window yourself.",
            ),
            DisplayServer::Macos | DisplayServer::X11 => None,
        }
    }
}

pub fn detect(is_macos: bool, env: &dyn Fn(&str) -> Option<String>) -> DisplayServer {
    if is_macos {
        return DisplayServer::Macos;
    }
    let set = |k: &str| env(k).map(|v| !v.trim().is_empty()).unwrap_or(false);
    let session = env("XDG_SESSION_TYPE").map(|v| v.trim().to_lowercase());
    match session.as_deref() {
        Some("wayland") => DisplayServer::Wayland,
        Some("x11") => DisplayServer::X11,
        _ if set("WAYLAND_DISPLAY") => DisplayServer::Wayland,
        _ if set("DISPLAY") => DisplayServer::X11,
        _ => DisplayServer::Unknown,
    }
}

pub fn detect_system() -> DisplayServer {
    detect(cfg!(target_os = "macos"), &|k| std::env::var(k).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn det(macos: bool, pairs: &[(&str, &str)]) -> DisplayServer {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        detect(macos, &move |k| m.get(k).cloned())
    }

    #[test]
    fn macos_always_supports_placement() {
        let d = det(true, &[("XDG_SESSION_TYPE", "wayland")]);
        assert_eq!(d, DisplayServer::Macos);
        assert!(d.supports_window_placement() && d.placement_hint().is_none());
    }

    #[test]
    fn wayland_session_skips_placement_with_a_hint() {
        for env in [
            vec![("XDG_SESSION_TYPE", "wayland"), ("DISPLAY", ":0")],
            vec![("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")],
        ] {
            let d = det(false, &env);
            assert_eq!(d, DisplayServer::Wayland);
            assert!(!d.supports_window_placement());
            assert!(d.placement_hint().unwrap().contains("Wayland"));
        }
    }

    #[test]
    fn x11_session_supports_placement() {
        assert_eq!(det(false, &[("XDG_SESSION_TYPE", "x11"), ("WAYLAND_DISPLAY", "wayland-0")]), DisplayServer::X11);
        assert_eq!(det(false, &[("DISPLAY", ":1")]), DisplayServer::X11);
        assert!(DisplayServer::X11.supports_window_placement());
    }

    #[test]
    fn nothing_set_is_unknown_and_conservative() {
        let d = det(false, &[("DISPLAY", "  ")]);
        assert_eq!(d, DisplayServer::Unknown);
        assert!(!d.supports_window_placement() && d.placement_hint().is_some());
    }
}
