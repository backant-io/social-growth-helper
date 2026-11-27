# Design system

Monochrome, quiet, typographic. Tokens live in `src/index.css`; components and page patterns in `src/components/ui.tsx`.

## Principles (read before adding UI)

1. **One job per page.** Every page has one primary action and one list or form. If you cannot name the job in four words, split the page or cut features.
2. **Only essential actions.** A control earns its place if most people use it most days. Everything else is removed, not hidden behind "More".
3. **No chrome that does not help the next decision.** Do not add: filter bars, search boxes, sort pickers, min-score or count sliders, session-length pickers, model pickers inside feature pages, status badges, counters, refresh buttons, "last synced" lines, explanatory paragraphs, or banners that restate state. Data should refresh by itself.
4. **Never gate or pause the user.** No pause-everything switches, no "the app only acts when you press a button" notices, no confirmation panels for safe actions. The backend stays safe; the UI stays quiet. Confirm only destructive or irreversible actions.
5. **Advanced is a place, not a layer.** Settings has one Advanced tab. Inside a feature page, rarely needed controls go in a `Disclosure` (single level, closed by default). Never nest disclosures.
6. **Show problems, not health.** Banners and notices appear only when something needs attention (offline, sync failing, login required) and disappear on their own. A healthy app shows nothing.
7. **Defaults over choices.** Pick the sensible default (models, durations, thresholds) in code. Ask the user only when no default can be right.
8. **Words:** titles are one or two words; descriptions are at most one short sentence and are optional; button labels are verbs.
9. **Empty and loading states** use `EmptyBlock` / `LoadingBlock`: a short title, one line, at most one action.
10. **No cards unless they wrap an interaction.** Use whitespace and 1px rules.

## Tokens

- **Type**: Bricolage Grotesque (display: h1-h3, brand), Instrument Sans (body), JetBrains Mono (eyebrows, code). Bundled with `@fontsource-variable/*`, so the CSP needs no network.
- **Colour**: `--bg`, `--bg-subtle`, `--fg`, `--fg-muted`, `--fg-faint`, `--line`, `--line-strong`. Light and dark follow the OS. The Tailwind `zinc`, `sky`, `violet`, `blue`, `indigo` scales and the status colours are redefined in `@theme`, so every utility resolves to the neutral palette. Use colour only for status.
- **Space and shape**: 4px grid (`--space-*`), small radii (`--radius-*`), 1px borders.
- **Atmosphere**: `.atmosphere` (top glow and fading dot grid), `.sidebar-surface` (soft gradient). Avoid flat empty areas.

## Shared page patterns

Build every screen from these. Do not hand-roll replacements.

| Pattern | Use |
| --- | --- |
| `Page` (`wide` for two columns) | Outer wrapper: centred column, `space-y-8`. |
| `PageHeader` | Title, optional one-line description, optional right-aligned actions (one primary at most). |
| `Tabs` | Underline tabs for 2 to 4 sibling views of the same subject. |
| `Segmented` | A single either/or choice (for example For You / Following). |
| `Disclosure` | One closed section for rare controls. |
| `Notice` | Left-bar message for a problem or a result. Only when needed. |
| `EmptyBlock` / `LoadingBlock` | Empty and loading states. |
| `panel` | A section with a top rule, not a box. |
| `btnPrimary`, `btn`, `btnGhost`, `btnDanger`, `input`, `field`, `chip`, `Modal` | Controls. One `btnPrimary` per view. |

A typical page:

```tsx
<Page>
  <PageHeader title="Scout" actions={<button className={btnPrimary}>Start scouting</button>} />
  {/* the one list or form */}
  <Disclosure summary="Limits">{/* rare controls */}</Disclosure>
</Page>
```

## What the app shows today

- **Scout**: Start / Stop, For You / Following, queue. Selected post: angle, Draft reply, View on X, Dismiss.
- **Reply Studio**: the draft, Copy / Open on X, Regenerate.
- **Creator**: Write and Sources; no filters.
- **Learning**: insights with Accept / Dismiss.
- **Analytics**: the numbers; sync banner only when sync fails.
- **Style**: Voice, Examples, Try it; limits and version history in disclosures.
- **Activity**: a plain list that refreshes itself.
- **Settings**: General (Scout preferences, X window), AI models, Advanced.

## Intro, skip and window

- **Intro**: `Spotlight` (`src/onboarding/Spotlight.tsx`) dims the app and rings the control named by `data-tour`. Click the ring, Enter or arrows advance.
- **Skip setup**: Skip is on the welcome screen and every step. It unlocks the screens (`isLocked` in `setupSkip.ts`); Settings shows "Finish setup" until the rules are acknowledged. The backend still refuses every X action until then.
- **Window menu**: the default in-window menu is removed on Linux and Windows; macOS keeps its system menu bar.
