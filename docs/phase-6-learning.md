# Phase 6: learning loop, export and wipe

Covers LRN-01 to LRN-14, SHL-12 (export/import) and SHL-13 (wipe). STY-07 style versioning is provided by Phase 2 and is not duplicated here.

## What it does

- Compares what the app drafted with what you posted (edits), what you skipped (dismissals) and how replies performed (impressions normalized by author reach and your own median).
- Turns repeated patterns into insight cards. Each card carries a structured patch (scout rubric, style guide or example promotion), evidence and a confidence.
- Nothing changes until you press Apply. You can edit the text first. Every rubric apply creates an immutable new version, and Roll back restores an older one. Style changes create a new `style_guides` version that is managed on the Style page.
- Below the minimum sample (default 30 published replies, adjustable) cards are marked low confidence.
- Learning never writes to X. Passes run on app open (at most every 12 hours) or when you press Quick or Deep reflect. There is no background daemon.
- Deep reflect also asks an analysis model: `RouterLlm` sends the prompt through `LlmRouter::complete_json_for_task("learning_analysis")`. Its proposals are parsed, validated, capped at 0.8 confidence and limited to rubric and style patches. If the route is unset or the call fails, the pass still lands the rule-based suggestions and says why in the run note. Quick reflect never calls a model.

## Data

Migration `0006_learning.sql` (0003 is Phase 2, 0004 is Phase 3, 0005 is Phase 4 analytics views). It extends `learning_insights` and adds `learning_runs` and `learning_changelog`. A trigger blocks edits to existing rubric versions.

## Export, import, wipe

- Export writes a zip with `manifest.json`, `README.txt`, `style.json` and a sanitized `data.db`. API key references, cookies, the browser profile, app settings and acknowledgements are never included. Imported accounts are marked `needs_key`.
- Import checks the manifest and schema version, takes a safety backup, remaps the profile id and replaces the data in one transaction, rolling back on any integrity violation.
- Wipe requires typing `DELETE`, removes keychain entries first (aborts if that fails unless overridden), clears all rows, vacuums, deletes backups and the browser profile, and returns the app to onboarding.

## UI

- Learning section: Suggestions, What I noticed, History (versions with rollback, kept examples, change log).
- Settings, Your data: export, import (with a file check first) and delete everything.
- Outside the desktop app the UI runs against a localStorage preview backend (`src/learning/mock.ts`).

## STY-07 dedupe with Phase 2

- Phase 2 owns style version history (`style_guides` with `version`, `is_active`, and `StylePage` rollback). Learning does not keep its own style versions.
- A style patch from Learning inserts a new `style_guides` row (max version plus one, `is_active` moved to it, `note` set to `Applied from Learning`). The current guide is read through `style::repo::get_active_style_guide`, and the new row copies its prose, `max_reply_chars`, `emoji_policy` and `hashtag_policy`, changing only the learned rules and banned patterns.
- If no guide has been saved yet, the base is Phase 2's starter guide (what the app already uses). A baseline v1 with the starter's values is stored first, with the note `Starting point before learning changes`, so the change can be undone from the Style page.
- Learning's History tab lists scout rubric versions only, with a pointer to the Style page. Style changes appear in the change log.
- `learning_versions` and `learning_rollback` reject `style` with a validation error that points to the Style page.
- Promoted examples are inserted into `style_examples` and its FTS5 index (`style_examples_fts`), so they are searchable through Phase 2's `search_style_examples`.
- Export, import and wipe skip FTS5 shadow tables (`*_data`, `*_idx`, `*_docsize`, `*_config`) and never copy them.
- Import does not trust the archive's search index. It skips `style_examples_fts`, then empties and rebuilds it from the imported `style_examples` inside the import transaction, with the profile id of this machine.

## Rebase notes

- The migration is 0006. Merge after Phases 3 and 4 are on `main`. The runner applies only versions above `user_version`, so a database already at 6 would skip any later 4 or 5.
- Expect conflicts in `lib.rs` (invoke handler), `App.tsx` and `SettingsPage.tsx`.
