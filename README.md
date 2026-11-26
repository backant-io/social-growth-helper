# Social Growth Helper

A local desktop app that helps you grow on X without giving up your voice. It is a guideline and a tool: you read, you decide and you click. It does not spam and it does not post AI slop for you.

Phase 1 (this branch) is the foundation: app shell, local database, OS keychain, a clean real-Chrome browser driver and the first-run tour. Scout, Reply Studio, Creator, Analytics and Learning are empty states for now.

## Stack

- Tauri v2 (Rust) with a React 19, TypeScript and Tailwind v4 control plane. X is never loaded in that webview.
- SQLite (WAL, forward-only migrations, backup before upgrade) in `~/Library/Application Support/social-growth-helper/app.db` (Linux: `~/.local/share/social-growth-helper`).
- Secrets in macOS Keychain, or Linux Secret Service. See [docs/linux-secret-service.md](docs/linux-secret-service.md).
- A real Google Chrome window (`--app=https://x.com/home`, separate profile) driven over loopback CDP with no automation flags and no `Runtime.enable`. Chrome is closed when the app quits.

## Develop

```bash
pnpm install
pnpm tauri dev          # desktop app
pnpm test               # frontend tests
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build              # typecheck + production frontend build
```

Live checks that open real Chrome or touch the real Keychain are ignored by default:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test live_chrome -- --ignored --nocapture --test-threads=1
```

## Package (macOS arm64)

```bash
pnpm tauri build --target aarch64-apple-darwin
```

Produces an ad-hoc signed `.app` and `.dmg` under `src-tauri/target/aarch64-apple-darwin/release/bundle/`. It is not notarized; on first launch use right-click, Open. Linux AppImage, deb and rpm builds are configured in `.github/workflows/ci.yml` as stubs and have not been run yet.

## Verification status

See [docs/phase-1-verification.md](docs/phase-1-verification.md).

Learning loop, export and wipe: see [docs/phase-6-learning.md](docs/phase-6-learning.md).
