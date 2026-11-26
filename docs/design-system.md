# Design system

Monochrome, quiet, typographic. Tokens live in `src/index.css`; components in `src/components/ui.tsx`.

- **Type**: Bricolage Grotesque (display: h1-h3, brand, page titles), Instrument Sans (body), JetBrains Mono (eyebrows, code). Fonts are bundled through `@fontsource-variable/*`, so the CSP needs no network (`font-src 'self' data:`).
- **Colour**: `--bg`, `--bg-subtle`, `--fg`, `--fg-muted`, `--fg-faint`, `--line`, `--line-strong`. Light and dark follow the OS. The Tailwind `zinc` scale and the status colours (red, amber, emerald, sky) are redefined in `@theme`, so every existing utility resolves to the same neutral palette.
- **Space and shape**: 4px grid (`--space-*`), small radii (`--radius-*`, 4 to 10px), 1px borders.
- **Components**: `btn` (outline), `btnPrimary` (solid), `btnGhost` (text), `btnDanger`, `input`/`field`, `chip`, `Tabs` (underline), `PageHeader`, `Notice`, `EmptyBlock`, `Modal`.
- **No cards by default**: `panel` is a section with a top rule. Boxes are kept only around real interaction (inputs, notices, the coach mark).
- **Atmosphere**: `.atmosphere` adds a faint top glow and a dot grid that fades out; `.sidebar-surface` a soft gradient. Avoid flat empty areas.
- **Intro**: `Spotlight` (src/onboarding/Spotlight.tsx) dims the app and rings the real control named by `data-tour` (`nav-*`, `main`). Clicking the ring, Enter or the arrow keys advance.
- **Skip setup**: Skip is on the welcome screen and every setup step. It unlocks the screens (`isLocked` in `setupSkip.ts`) and shows a banner with "Finish setup". The backend still refuses every X action until the rules are acknowledged.
- **Window menu**: the default in-window menu is removed on Linux and Windows; macOS keeps its system menu bar.
