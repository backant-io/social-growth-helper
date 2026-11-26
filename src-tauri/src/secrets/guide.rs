use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DistroSteps {
    pub name: String,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupGuide {
    pub title: String,
    pub summary: String,
    pub no_file_fallback: bool,
    pub options: Vec<DistroSteps>,
    pub verify: String,
}

/// Install-and-start steps for the three supported Secret Service providers (SEC-11).
/// Mirrored in `docs/linux-secret-service.md`.
pub fn linux_setup_guide() -> SetupGuide {
    let o = |name: &str, steps: &[&str]| DistroSteps {
        name: name.to_string(),
        steps: steps.iter().map(|s| s.to_string()).collect(),
    };
    SetupGuide {
        title: "A Secret Service is required to store tokens on Linux".into(),
        summary: "Social Growth Helper keeps API keys and OAuth tokens only in your desktop's Secret Service (libsecret). \
                  There is no encrypted-file fallback, so provider connections stay disabled until one is running."
            .into(),
        no_file_fallback: true,
        options: vec![
            o(
                "GNOME Keyring on Ubuntu LTS",
                &[
                    "sudo apt install gnome-keyring libsecret-1-0 seahorse",
                    "Log out and back in so the keyring daemon starts with your session (or run: eval $(gnome-keyring-daemon --start --components=secrets))",
                    "Open Passwords and Keys (seahorse) and make sure a default \"Login\" keyring exists and is unlocked",
                ],
            ),
            o(
                "GNOME Keyring on Fedora",
                &[
                    "sudo dnf install gnome-keyring libsecret seahorse",
                    "Log out and back in, or run: eval $(gnome-keyring-daemon --start --components=secrets)",
                    "Create and unlock a default \"Login\" keyring in Passwords and Keys",
                ],
            ),
            o(
                "KeePassXC (any distro)",
                &[
                    "Install KeePassXC (apt install keepassxc, or dnf install keepassxc)",
                    "In KeePassXC open Settings > Secret Service Integration and enable it",
                    "Open the database you want to expose and tick \"Expose entries under this group\"; keep KeePassXC running",
                ],
            ),
            o(
                "KDE Wallet",
                &[
                    "Install kwalletmanager (apt install kwalletmanager, or dnf install kwalletmanager)",
                    "Open KDE Wallet Manager and make sure a wallet exists; KDE Plasma 6 provides the Secret Service from the wallet daemon",
                    "Keep the wallet unlocked for your session",
                ],
            ),
        ],
        verify: "Click Re-check in Settings > Security. The status turns green once a test item can be written, read back and deleted.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_covers_all_required_providers() {
        let g = linux_setup_guide();
        let names: Vec<_> = g.options.iter().map(|o| o.name.to_lowercase()).collect();
        assert!(names.iter().any(|n| n.contains("gnome") && n.contains("ubuntu")));
        assert!(names.iter().any(|n| n.contains("gnome") && n.contains("fedora")));
        assert!(names.iter().any(|n| n.contains("keepassxc")));
        assert!(names.iter().any(|n| n.contains("kde wallet")));
        assert!(g.options.iter().all(|o| !o.steps.is_empty()));
    }
}
