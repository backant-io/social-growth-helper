# Phase 5 verification: viral formats and the OGC creator

Covers matrix rows OGC-01 to OGC-09, OGC-13, OGC-14 and BRW-17. OGC-10 (personal win rate) is stored and sortable but stays empty until Phase 4 analytics feeds it. OGC-11 and OGC-12 are P2 and not built.

## What was built

- `src-tauri/src/formats/`: viral detection (engagement against follower count plus bookmark ratio, thresholds editable), blueprint extraction behind the `StructuredLlm` seam with strict validation (placeholders match slots, no topic words, no copied runs), clustering and dedup, the format library, creator watchlist with a user-started profile scan, manual add. `formats::router_llm::RouterLlm` implements `StructuredLlm` on top of `LlmRouter::complete_json_for_task`, and `AppState::llm` holds it.
- `src-tauri/src/creator/`: three-variant composer in the user's voice (style guide, banned phrases, pinned examples), originality check, drafts with edit distance, media validation. Publishing goes through Phase 3's approval gate and serial publish worker; there is no second publish path.
- `src-tauri/src/publish/gate.rs`: `Scope::TopLevel` and `approve_top_level`, an additive scope for a standalone OGC post. `Approval` is still constructible only inside the gate. Phase 3 reply and engagement scopes are unchanged.
- `src-tauri/src/publish/executor.rs`: `execute_top_level`, which composes and posts a standalone post using Phase 3's executor helpers and records one `publish_audit_log` row (`target_kind = 'ogc'`) per attempt. `AppRunner::run` dispatches top-level approvals here.
- `BrowserDriver::set_file_input_files` (DOM.setFileInputFiles) for media attach.
- Migration `0007_viral_formats_creator.sql` (additive: new columns, `format_candidates`, `creator_watchlist`). It follows Phase 2 (0003), Phase 3 (0004), Phase 4 (0005) and Phase 6 (0006).
- UI: Creator page with Write, Formats and Sources and settings tabs.

## Guarantees and how they are enforced

| Rule | Where |
| --- | --- |
| Nothing is posted without a click | `gate::approve_top_level` is the only way to build a top-level `Approval`. It requires the draft to be open, the user to have acknowledged, the kill switch to be off, and the confirmed text to equal the stored text. Tested: stale text, empty, over limit, double claim, kill switch, unacknowledged |
| One publish at a time | The OGC click submits to Phase 3's serial worker (`publish.submit_and_wait`), the same queue as reply and engagement publishes |
| No duplicate posts | The claim is one atomic `UPDATE ... WHERE status IN (...)`. A click on Post whose result was not seen becomes `unconfirmed` and is never retried until the user says it is not on X. Tested in `top_level_tests` |
| Text typed is the text approved | The composer is read back and compared before Post is pressed |
| Kill switch | Checked at approval time by the gate, and Phase 3's executor checks apply before Post |
| Scraped text is untrusted | Candidate text is fenced in the extraction prompt and the output is validated before storage |
| No unattended automation | Scans run one creator per click. There is no timer |
| No model, no guessing | If the `format_extraction` or `ogc_generation` route is unset, `RouterLlm` returns `NoModel`. Extraction leaves the candidate pending and says which setting is missing. Formats can still be written by hand |

## Verified here (Linux, headless)

| Check | Result |
| --- | --- |
| `cargo check --all-targets` | clean, no warnings |
| `cargo test --lib` | 385 passed |
| `pnpm typecheck`, `pnpm test` (47 tests), `pnpm build` | ok |
| UI in a plain browser against the in-memory stand-in (Playwright): write 3 versions, edit, attach media, Publish disabled with the X window closed, extraction message without a model, new format, watchlist add and validation | passed, no console errors (run before the publish rewiring; repeat before merge) |

## Known gaps

- The OGC path does not run `pre_publish_problems` (banned-phrase and recent-duplicate checks). That function rejects empty action bundles, and a standalone post has none. Banned phrases are enforced at generation time only.
- The OGC path does not run `pacing::check`.
- Anything against live X is unverified: the compose selectors (`tweetTextarea_0`, `fileInput`, `tweetButton`, `toast`) in `publish/executor.rs`, the profile scan selectors and metric labels in `formats/scan.rs`, and real media upload. Check them with a throwaway account before relying on Publish.
- Viral thresholds are defaults, not tuned on real feeds.
- Phase 3 Scout should call `formats::pipeline::consider_post(..., PostSource::Scout, ...)` for each evaluated post (OGC-06). The hook exists; the call site does not.
