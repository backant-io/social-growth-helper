# Linux: Secret Service setup

Social Growth Helper stores API keys and OAuth tokens only in the OS keychain: macOS Keychain, or the freedesktop Secret Service (libsecret) on Linux. There is **no encrypted-file fallback**. If no Secret Service is available, provider connections stay disabled and Settings > Security shows the steps below.

The in-app guide is generated from `src-tauri/src/secrets/guide.rs`; keep both in sync.

## How detection works

1. The app checks that a D-Bus session bus is present (`DBUS_SESSION_BUS_ADDRESS` or `$XDG_RUNTIME_DIR/bus`).
2. It writes, reads back and deletes a throwaway item through the Secret Service.
3. If either step fails, status is "not available" and storing or loading a secret is refused.

Click **Re-check** in Settings > Security after changing anything.

## GNOME Keyring on Ubuntu LTS

```bash
sudo apt install gnome-keyring libsecret-1-0 seahorse
```

Log out and back in so the daemon starts with your session (or `eval $(gnome-keyring-daemon --start --components=secrets)`). Open Passwords and Keys and make sure a default "Login" keyring exists and is unlocked.

## GNOME Keyring on Fedora

```bash
sudo dnf install gnome-keyring libsecret seahorse
```

Log out and back in, or run `eval $(gnome-keyring-daemon --start --components=secrets)`. Create and unlock a default "Login" keyring.

## KeePassXC (any distro)

Install KeePassXC, open Settings > Secret Service Integration and enable it. Open the database you want to expose and tick "Expose entries under this group". Keep KeePassXC running.

## KDE Wallet

Install `kwalletmanager`, make sure a wallet exists (Plasma 6 provides the Secret Service from the wallet daemon) and keep it unlocked.

## Troubleshooting

- Headless or SSH sessions usually have no session bus; run the app from a desktop session.
- If the status says the keyring is locked, unlock it in Passwords and Keys (or KeePassXC / KDE Wallet) and re-check.
- Package builds: the `.deb` depends on `libsecret-1-0` and a Secret Service provider; the `.rpm` depends on `libsecret` and `gnome-keyring`.
