# Phase 1 verification

Run on an Apple Silicon Mac (macOS, arm64) with Chrome 158.

## Verified

| Check | Result |
| --- | --- |
| `cargo check` | clean, no warnings |
| `cargo test` (unit) | 62 passed |
| `pnpm build` (tsc + vite) | ok |
| `pnpm test` (vitest) | 9 passed |
| Live Chrome (`--ignored`): launch `--app`, isolated profile, run script in isolated world, screenshot, `Browser.close`, no leftover process | passed. `navigator.webdriver` is false and `Runtime.enable` never appears in the protocol log |
| Live macOS Keychain round trip (`--ignored`) | passed |
| `pnpm tauri build --target aarch64-apple-darwin` | `.app` (ad-hoc signed, arm64) and `.dmg` built |
| Packaged `.app` smoke run with a throwaway `HOME` | starts, creates `app.db`, `user_version` 2, WAL, 7 unset model routes |

Notes:

- DMG: the default `create-dmg` path asks Finder to style the window over AppleScript and can time out (`AppleEvent timed out (-1712)`) when Finder automation is not allowed. `CI=true pnpm tauri build ...` skips the styling and produces the DMG reliably. GitHub Actions sets `CI=true` already.
- The `.app` is ad-hoc signed and not notarized. First launch: right-click, Open.

## Not verified yet

- Anything that needs a live X login: login persistence across an app restart, reading the real home feed, the six post actions, and confirming the SPK-06 selectors (`READ_VISIBLE_TWEETS_JS`) against live X markup. BRW-06 (logged-in state detection) is not implemented in this phase.
- Linux: the Secret Service detector is unit-tested with an injected platform, and the setup guide is documented, but nothing was run on a Linux machine. The Linux CI job and the AppImage, deb and rpm bundles are stubs that have never run.
- Window layout of the X window next to the control plane (SHL-28 quit guard UI is wired but only exercised in unit tests; no real jobs exist yet).
- Tour and preference UI were built and type-checked but not exercised in an automated browser test.

## Phase 1 gap rows (BRW-14, BRW-15, SHL-14, SHL-23)

Implemented with unit tests only; run on a Linux cloud VM without a display, so none of it was exercised against a real window or Chrome UI.

- BRW-15: `browser/compat.rs` reads `<browser> --version` and warns below `MIN_TESTED_CHROME_MAJOR` (140; Phase 0/1 runs used 157/158). Shown in Settings.
- BRW-14: `browser/display.rs` detects Wayland/X11/macOS. On Wayland (and undetectable sessions) window placement is skipped and a hint is shown; nothing errors.
- SHL-23: `layout.rs` plans a side-by-side layout, saves/restores it in `app_settings`, and falls back to the default if the saved rectangles are off-screen. Needs a live check on macOS and X11 (CDP `Browser.setWindowBounds` vs Tauri logical coordinates, multi-monitor, HiDPI).
- SHL-14: `logging.rs` writes rotating logs (1 MiB x 5) under `<data dir>/logs` with redaction; Settings has "Copy diagnostics".
