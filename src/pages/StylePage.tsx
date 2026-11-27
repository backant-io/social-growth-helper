import { useCallback, useEffect, useState } from "react";
import { UnsetRouteGuard } from "../components/UnsetRouteGuard";
import {
  btn,
  btnGhost,
  btnPrimary as primaryBtn,
  Disclosure,
  input as inputCls,
  Notice,
  Page,
  PageHeader,
  panel,
  Tabs,
} from "../components/ui";
import { api } from "../lib/api";
import { DiffView } from "../learning/DiffView";
import { parseLlmError } from "../lib/errors";
import { previousVersion, styleVersionDiff } from "../lib/styleDiff";
import type {
  BenchModelRun,
  EmojiPolicy,
  ExampleKind,
  HashtagPolicy,
  ImportCandidate,
  ProviderAccountView,
  StyleBenchResult,
  StyleExample,
  StyleGuide,
  TaskModelRoute,
} from "../lib/types";

interface Props {
  locked: boolean;
  offline: boolean;
  onReviewRules: () => void;
  onOpenSettings: () => void;
}

type StyleTab = "guide" | "examples" | "bench";

function TagChips({
  items,
  onAdd,
  onRemove,
  placeholder,
  ariaLabel,
}: {
  items: string[];
  onAdd: (val: string) => void;
  onRemove: (val: string) => void;
  placeholder: string;
  ariaLabel: string;
}) {
  const [draft, setDraft] = useState("");
  const commit = () => {
    const clean = draft.trim();
    if (!clean) return;
    onAdd(clean);
    setDraft("");
  };
  return (
    <div className="space-y-2">
      <div className="flex gap-2">
        <input
          aria-label={ariaLabel}
          className={`${inputCls} flex-1`}
          placeholder={placeholder}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commit();
            }
          }}
        />
        <button type="button" className={btn} onClick={commit}>
          Add
        </button>
      </div>
      {items.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {items.map((item) => (
            <span
              key={item}
              className="inline-flex items-center gap-1 rounded-md border border-zinc-300 bg-zinc-50 px-2 py-0.5 text-xs dark:border-zinc-700 dark:bg-zinc-900"
            >
              {item}
              <button
                type="button"
                aria-label={`Remove ${item}`}
                className="text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100"
                onClick={() => onRemove(item)}
              >
                ×
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function ModelRunColumn({ title, run }: { title: string; run: BenchModelRun }) {
  return (
    <div className="space-y-3 rounded-xl border border-zinc-200 p-3 dark:border-zinc-800">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-zinc-200 pb-2 text-xs dark:border-zinc-800">
        <div>
          <span className="font-semibold">{title}</span>{" "}
          <span className="font-mono text-zinc-500">{run.model_id}</span>
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="rounded bg-zinc-100 px-1.5 py-0.5 dark:bg-zinc-800">
            {run.billing_lane === "subscription" ? "Subscription" : "API"}
          </span>
          {run.used_fallback && (
            <span className="rounded bg-amber-100 px-1.5 py-0.5 text-amber-800 dark:bg-amber-950/60 dark:text-amber-300">
              Fallback used
            </span>
          )}
          <span className="text-zinc-500">{run.latency_ms} ms</span>
        </div>
      </div>

      {run.fallback_reason && (
        <p className="text-xs text-amber-700 dark:text-amber-300">
          Fallback reason: {run.fallback_reason}
        </p>
      )}

      {run.variants.length === 0 ? (
        <p className="text-xs text-amber-700 dark:text-amber-300">
          No variants passed the banned-phrase post-filter.
        </p>
      ) : (
        <div className="space-y-2">
          {run.variants.map((v, idx) => (
            <div
              key={idx}
              className="rounded-lg border border-zinc-200 p-3 text-sm dark:border-zinc-800"
            >
              <div className="mb-1 flex flex-wrap items-center justify-between gap-2 text-xs">
                <span className="rounded bg-zinc-100 px-1.5 py-0.5 font-medium dark:bg-zinc-800">
                  {v.tone_tag}
                </span>
                <span
                  className={
                    v.check.passed
                      ? "text-emerald-600 dark:text-emerald-400"
                      : "text-amber-600 dark:text-amber-400"
                  }
                >
                  {v.check.char_count}/{v.check.max_reply_chars} chars
                  {v.check.passed
                    ? " · All rules passed"
                    : ` · ${v.check.violations.join("; ")}`}
                </span>
              </div>
              <p className="whitespace-pre-wrap">{v.text}</p>
            </div>
          ))}
        </div>
      )}

      {run.rejected_variants.length > 0 && (
        <details className="rounded-lg border border-red-200 bg-red-50/50 p-2.5 text-xs dark:border-red-900 dark:bg-red-950/30">
          <summary className="cursor-pointer font-medium text-red-700 dark:text-red-300">
            {run.rejected_variants.length} variant(s) blocked by banned-phrase
            post-filter
          </summary>
          <div className="mt-2 space-y-2">
            {run.rejected_variants.map((rv, i) => (
              <div
                key={i}
                className="rounded border border-red-200 p-2 dark:border-red-900"
              >
                <div className="font-medium text-red-700 dark:text-red-300">
                  Blocked: {rv.check.banned_hits.join(", ")}
                </div>
                <p className="mt-1 opacity-80">{rv.text}</p>
              </div>
            ))}
          </div>
        </details>
      )}
    </div>
  );
}

export function StylePage({
  locked,
  offline,
  onReviewRules,
  onOpenSettings,
}: Props) {
  const [tab, setTab] = useState<StyleTab>("guide");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Guide state
  const [, setGuide] = useState<StyleGuide | null>(null);
  const [versions, setVersions] = useState<StyleGuide[]>([]);
  const [prose, setProse] = useState("");
  const [learnedRules, setLearnedRules] = useState<string[]>([]);
  const [bannedPatterns, setBannedPatterns] = useState<string[]>([]);
  const [maxChars, setMaxChars] = useState(220);
  const [emojiPolicy, setEmojiPolicy] = useState<EmojiPolicy>("minimal");
  const [hashtagPolicy, setHashtagPolicy] = useState<HashtagPolicy>("none");
  const [versionNote, setVersionNote] = useState("");
  const [savingGuide, setSavingGuide] = useState(false);
  const [diffOpen, setDiffOpen] = useState<number | null>(null);

  // Examples state
  const [examples, setExamples] = useState<StyleExample[]>([]);
  const exKind: ExampleKind = "reply";
  const [exContext, setExContext] = useState("");
  const [exText, setExText] = useState("");
  const [exWhy, setExWhy] = useState("");
  const [exTags, setExTags] = useState("");
  const [exPinned, setExPinned] = useState(false);
  const [savingExample, setSavingExample] = useState(false);

  // Import state
  const [showImport, setShowImport] = useState(false);
  const [importMode, setImportMode] = useState<"paste" | "x">("paste");
  const [importKind, setImportKind] = useState<ExampleKind>("reply");
  const [pasteRaw, setPasteRaw] = useState("");
  const [xHandle, setXHandle] = useState("");
  const [candidates, setCandidates] = useState<ImportCandidate[]>([]);
  const [selectedIdx, setSelectedIdx] = useState<Set<number>>(new Set());
  const [importing, setImporting] = useState(false);

  // Bench state
  const [providers, setProviders] = useState<ProviderAccountView[]>([]);
  const [routes, setRoutes] = useState<TaskModelRoute[]>([]);
  const [samplePost, setSamplePost] = useState(
    "We cut database query latency by 60% just by batching our dashboard metrics into one materialized view.",
  );
  const [runningBench, setRunningBench] = useState(false);
  const [benchResult, setBenchResult] = useState<StyleBenchResult | null>(null);

  const hydrateGuide = (g: StyleGuide) => {
    setGuide(g);
    setProse(g.free_text_guide);
    setLearnedRules(g.learned_rules);
    setBannedPatterns(g.banned_patterns);
    setMaxChars(g.max_reply_chars);
    setEmojiPolicy(g.emoji_policy);
    setHashtagPolicy(g.hashtag_policy);
  };

  const loadAll = useCallback(async () => {
    const [g, vers, p, r] = await Promise.all([
      api.styleGetActiveGuide(),
      api.styleListGuideVersions(),
      api.llmListProviders(),
      api.llmListTaskRoutes(),
    ]);
    hydrateGuide(g);
    setVersions(vers);
    setProviders(p);
    setRoutes(r);
  }, []);

  const loadExamples = useCallback(async () => {
    setExamples(await api.styleListExamples(null));
  }, []);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  useEffect(() => {
    void loadExamples();
  }, [loadExamples]);

  const handleSaveGuide = async () => {
    setSavingGuide(true);
    setError(null);
    setNotice(null);
    try {
      const saved = await api.styleSaveGuideVersion({
        free_text_guide: prose,
        learned_rules: learnedRules,
        banned_patterns: bannedPatterns,
        max_reply_chars: maxChars,
        emoji_policy: emojiPolicy,
        hashtag_policy: hashtagPolicy,
        note: versionNote.trim() || null,
      });
      hydrateGuide(saved);
      setVersionNote("");
      setVersions(await api.styleListGuideVersions());
      setNotice(`Saved Style Guide v${saved.version}.`);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSavingGuide(false);
    }
  };

  const handleActivateVersion = async (ver: number) => {
    setError(null);
    try {
      const active = await api.styleActivateGuideVersion(ver);
      hydrateGuide(active);
      setVersions(await api.styleListGuideVersions());
      setNotice(`Activated Style Guide v${ver}.`);
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const handleSaveExample = async () => {
    setSavingExample(true);
    setError(null);
    try {
      await api.styleUpsertExample({
        id: null,
        kind: exKind,
        source_context_text: exContext.trim() || null,
        example_text: exText,
        why_it_works: exWhy.trim() || null,
        tags: exTags
          .split(",")
          .map((t) => t.trim())
          .filter(Boolean),
        pinned: exPinned,
        source: "manual",
        performance_score: null,
      });
      setExContext("");
      setExText("");
      setExWhy("");
      setExTags("");
      setExPinned(false);
      await loadExamples();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSavingExample(false);
    }
  };

  const handleParsePaste = async () => {
    setError(null);
    const found = await api.styleParsePastedExamples(pasteRaw, importKind);
    setCandidates(found);
    setSelectedIdx(new Set(found.map((_, i) => i)));
  };

  const handleBrowserImport = async () => {
    setImporting(true);
    setError(null);
    try {
      const found = await api.styleImportExamplesFromBrowser(
        xHandle.trim() || null,
        importKind,
      );
      setCandidates(found);
      setSelectedIdx(new Set(found.map((_, i) => i)));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setImporting(false);
    }
  };

  const handleCommitCandidates = async () => {
    setImporting(true);
    setError(null);
    try {
      const chosen = candidates.filter((_, i) => selectedIdx.has(i));
      for (const c of chosen) {
        await api.styleUpsertExample({
          id: null,
          kind: c.kind,
          source_context_text: c.source_context_text,
          example_text: c.example_text,
          why_it_works: null,
          tags: c.tags,
          pinned: false,
          source: c.source,
          performance_score: null,
        });
      }
      setCandidates([]);
      setPasteRaw("");
      setShowImport(false);
      await loadExamples();
      setNotice(`Imported ${chosen.length} example(s).`);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setImporting(false);
    }
  };

  const handleRunBench = async () => {
    setRunningBench(true);
    setError(null);
    try {
      const res = await api.styleRunBench({
        sample_author: null,
        sample_post_text: samplePost,
        override_route: null,
        compare_route: null,
        example_limit: 5,
      });
      setBenchResult(res);
    } catch (e) {
      setError(parseLlmError(e).message);
    } finally {
      setRunningBench(false);
    }
  };

  const replyRoute = routes.find((r) => r.task_type === "reply_drafting");
  const hasReplyRoute = Boolean(
    replyRoute?.provider_account_id && replyRoute?.model_id,
  );

  return (
    <Page>
      <PageHeader
        title="Style"
        description="Your voice, in a few rules and examples."
      />
      <Tabs
        label="Style sections"
        active={tab}
        onChange={setTab}
        tabs={[
          { id: "guide", label: "Voice and rules" },
          { id: "examples", label: `Examples (${examples.length})` },
          { id: "bench", label: "Try it" },
        ]}
      />

      {locked && (
        <Notice tone="info">
          Acknowledge the rules to use live features.{" "}
          <button type="button" onClick={onReviewRules} className="underline">
            Review rules
          </button>
        </Notice>
      )}
      {error && <Notice tone="error">{error}</Notice>}
      {notice && <Notice tone="ok">{notice}</Notice>}

      {/* TAB 1: VOICE & RULES */}
      {tab === "guide" && (
        <div className="space-y-8">
          <section className="space-y-5">
            <label className="block">
              <span className="eyebrow mb-1.5 block">Voice</span>
              <textarea
                aria-label="Voice prose"
                rows={5}
                className={`${inputCls} w-full`}
                value={prose}
                onChange={(e) => setProse(e.target.value)}
                placeholder="How you sound: tone, perspective, pacing."
              />
            </label>

            <div className="">
              <div className="eyebrow mb-1.5">Phrases to avoid</div>
              <TagChips
                items={bannedPatterns}
                ariaLabel="Add banned phrase"
                placeholder="e.g. game changer, delve, let's dive in"
                onAdd={(v) =>
                  setBannedPatterns((prev) =>
                    prev.some((x) => x.toLowerCase() === v.toLowerCase())
                      ? prev
                      : [...prev, v],
                  )
                }
                onRemove={(v) =>
                  setBannedPatterns((prev) => prev.filter((x) => x !== v))
                }
              />
            </div>

            <div className="">
              <div className="eyebrow mb-1.5">Rules</div>
              <TagChips
                items={learnedRules}
                ariaLabel="Add style rule"
                placeholder="e.g. Lead with one concrete number or trade-off"
                onAdd={(v) =>
                  setLearnedRules((prev) =>
                    prev.some((x) => x.toLowerCase() === v.toLowerCase())
                      ? prev
                      : [...prev, v],
                  )
                }
                onRemove={(v) =>
                  setLearnedRules((prev) => prev.filter((x) => x !== v))
                }
              />
            </div>

            <div className="flex items-center justify-end gap-2">
              <button
                type="button"
                className={primaryBtn}
                disabled={savingGuide}
                onClick={() => void handleSaveGuide()}
              >
                {savingGuide ? "Saving..." : "Save"}
              </button>
            </div>
          </section>

          <Disclosure summary="Limits">
            <div className="grid gap-3 sm:grid-cols-3">
              <label className="text-xs font-medium text-zinc-600 dark:text-zinc-400">
                Max length
                <input
                  type="number"
                  min={40}
                  max={2000}
                  aria-label="Max reply length"
                  className={`${inputCls} mt-1 w-full`}
                  value={maxChars}
                  onChange={(e) => setMaxChars(Number(e.target.value) || 220)}
                />
              </label>
              <label className="text-xs font-medium text-zinc-600 dark:text-zinc-400">
                Emoji policy
                <select
                  aria-label="Emoji policy"
                  className={`${inputCls} mt-1 w-full`}
                  value={emojiPolicy}
                  onChange={(e) =>
                    setEmojiPolicy(e.target.value as EmojiPolicy)
                  }
                >
                  <option value="none">None (0 emojis)</option>
                  <option value="minimal">Minimal (max 1)</option>
                  <option value="allowed">Allowed</option>
                </select>
              </label>
              <label className="text-xs font-medium text-zinc-600 dark:text-zinc-400">
                Hashtag policy
                <select
                  aria-label="Hashtag policy"
                  className={`${inputCls} mt-1 w-full`}
                  value={hashtagPolicy}
                  onChange={(e) =>
                    setHashtagPolicy(e.target.value as HashtagPolicy)
                  }
                >
                  <option value="none">None (0 hashtags)</option>
                  <option value="max_1">Max 1 hashtag</option>
                  <option value="allowed">Allowed</option>
                </select>
              </label>
            </div>
          </Disclosure>

          <Disclosure summary="Version history">
            <input
              aria-label="Version note"
              className={`${inputCls} w-full`}
              placeholder="Optional note for this version (e.g. 'Tightened max length to 200')"
              value={versionNote}
              onChange={(e) => setVersionNote(e.target.value)}
            />
            <section>
              {versions.length === 0 ? (
                <p className="mt-2 text-xs text-zinc-500">
                  No saved versions yet. Save once to create v1.
                </p>
              ) : (
                <ul className="mt-2 space-y-2">
                  {versions.map((v) => (
                    <li
                      key={v.id}
                      className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-xs dark:border-zinc-800"
                    >
                      <div>
                        <span className="font-semibold">v{v.version}</span>{" "}
                        {v.is_active && (
                          <span className="rounded bg-emerald-100 px-1.5 py-0.5 text-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-300">
                            Active
                          </span>
                        )}
                        {v.note && (
                          <span className="ml-2 text-zinc-600 dark:text-zinc-400">
                            — {v.note}
                          </span>
                        )}
                        <div className="mt-0.5 text-zinc-500">
                          Max {v.max_reply_chars} chars ·{" "}
                          {v.banned_patterns.length} banned pattern(s) ·{" "}
                          {v.created_at}
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <button
                          type="button"
                          className={btn}
                          aria-expanded={diffOpen === v.version}
                          onClick={() =>
                            setDiffOpen(
                              diffOpen === v.version ? null : v.version,
                            )
                          }
                        >
                          {diffOpen === v.version
                            ? "Hide changes"
                            : "Show changes"}
                        </button>
                        {!v.is_active && (
                          <button
                            type="button"
                            className={btn}
                            onClick={() =>
                              void handleActivateVersion(v.version)
                            }
                          >
                            Activate v{v.version}
                          </button>
                        )}
                      </div>
                      {diffOpen === v.version && (
                        <div className="w-full">
                          <p className="mb-1 text-zinc-500">
                            {previousVersion(versions, v.version)
                              ? `Changes from v${previousVersion(versions, v.version)!.version} to v${v.version}`
                              : `Everything in v${v.version} (first saved version)`}
                          </p>
                          <DiffView
                            diff={styleVersionDiff(
                              previousVersion(versions, v.version),
                              v,
                            )}
                            label={`Style guide v${v.version} changes`}
                          />
                        </div>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </Disclosure>
        </div>
      )}

      {/* TAB 2: FEW-SHOT EXAMPLES */}
      {tab === "examples" && (
        <div className="space-y-4">
          <div className="flex items-center justify-between gap-2">
            <p className="text-sm text-zinc-600 dark:text-zinc-400">
              Replies and posts that sound like you.
            </p>
            <button
              type="button"
              className={btnGhost}
              onClick={() => setShowImport((v) => !v)}
            >
              {showImport ? "Close import" : "Import from X"}
            </button>
          </div>

          {showImport && (
            <section className={panel}>
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex gap-2">
                  <button
                    type="button"
                    className={importMode === "paste" ? primaryBtn : btn}
                    onClick={() => setImportMode("paste")}
                  >
                    Paste Text
                  </button>
                  <button
                    type="button"
                    className={importMode === "x" ? primaryBtn : btn}
                    onClick={() => setImportMode("x")}
                  >
                    Import from X Window
                  </button>
                </div>
                <select
                  aria-label="Import kind"
                  className={inputCls}
                  value={importKind}
                  onChange={(e) => setImportKind(e.target.value as ExampleKind)}
                >
                  <option value="reply">As Reply examples</option>
                  <option value="ogc">As OGC examples</option>
                </select>
              </div>

              {importMode === "paste" ? (
                <div className="mt-3 space-y-2">
                  <textarea
                    rows={4}
                    className={`${inputCls} w-full`}
                    placeholder="Paste examples separated by a blank line or ---"
                    value={pasteRaw}
                    onChange={(e) => setPasteRaw(e.target.value)}
                  />
                  <button
                    type="button"
                    className={btn}
                    onClick={() => void handleParsePaste()}
                  >
                    Extract Candidates
                  </button>
                </div>
              ) : (
                <div className="mt-3 flex flex-wrap gap-2">
                  <input
                    className={`${inputCls} flex-1`}
                    placeholder="@yourhandle (leave empty to read currently visible tweets in X window)"
                    value={xHandle}
                    onChange={(e) => setXHandle(e.target.value)}
                  />
                  <button
                    type="button"
                    className={primaryBtn}
                    disabled={importing || offline || locked}
                    onClick={() => void handleBrowserImport()}
                  >
                    {importing ? "Reading..." : "Fetch from X"}
                  </button>
                </div>
              )}

              {candidates.length > 0 && (
                <div className="mt-3 space-y-2">
                  <div className="flex items-center justify-between text-xs">
                    <span>
                      {selectedIdx.size} of {candidates.length} selected
                    </span>
                    <button
                      type="button"
                      className={primaryBtn}
                      disabled={importing || selectedIdx.size === 0}
                      onClick={() => void handleCommitCandidates()}
                    >
                      Save Selected ({selectedIdx.size})
                    </button>
                  </div>
                  <ul className="max-h-56 space-y-1.5 overflow-y-auto">
                    {candidates.map((c, idx) => (
                      <li
                        key={idx}
                        className="flex items-start gap-2 rounded-lg border border-zinc-200 p-2 text-xs dark:border-zinc-800"
                      >
                        <input
                          type="checkbox"
                          checked={selectedIdx.has(idx)}
                          onChange={(e) => {
                            const next = new Set(selectedIdx);
                            if (e.target.checked) next.add(idx);
                            else next.delete(idx);
                            setSelectedIdx(next);
                          }}
                        />
                        <div className="min-w-0 flex-1">
                          {c.author_handle && (
                            <span className="font-medium">
                              @{c.author_handle}:{" "}
                            </span>
                          )}
                          <span>{c.example_text}</span>
                        </div>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </section>
          )}

          <section className="space-y-3" aria-label="Add example">
            <textarea
              rows={3}
              aria-label="Example text"
              className={`${inputCls} w-full`}
              placeholder="Paste a reply or post you are proud of"
              value={exText}
              onChange={(e) => setExText(e.target.value)}
            />
            <div className="flex items-center justify-end gap-2">
              <button
                type="button"
                className={primaryBtn}
                disabled={savingExample || !exText.trim()}
                onClick={() => void handleSaveExample()}
              >
                Save example
              </button>
            </div>
          </section>

          <section className="space-y-2">
            {examples.length === 0 ? (
              <p className="text-sm text-zinc-500">No examples yet.</p>
            ) : (
              examples.map((ex) => (
                <div key={ex.id} className={panel}>
                  <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <span className="rounded bg-zinc-100 px-1.5 py-0.5 font-medium uppercase dark:bg-zinc-800">
                        {ex.kind}
                      </span>
                      {ex.pinned && (
                        <span className="rounded bg-emerald-100 px-1.5 py-0.5 text-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-300">
                          Pinned
                        </span>
                      )}
                      {ex.tags.map((t) => (
                        <span
                          key={t}
                          className="rounded border border-zinc-200 px-1.5 py-0.5 dark:border-zinc-700"
                        >
                          {t}
                        </span>
                      ))}
                    </div>
                    <div className="flex gap-2">
                      <button
                        type="button"
                        className={btn}
                        onClick={async () => {
                          await api.styleUpsertExample({
                            id: ex.id,
                            kind: ex.kind,
                            source_context_text: ex.source_context_text,
                            example_text: ex.example_text,
                            why_it_works: ex.why_it_works,
                            tags: ex.tags,
                            pinned: !ex.pinned,
                            source: ex.source,
                            performance_score: ex.performance_score,
                          });
                          await loadExamples();
                        }}
                      >
                        {ex.pinned ? "Unpin" : "Pin"}
                      </button>
                      <button
                        type="button"
                        className={btn}
                        onClick={async () => {
                          await api.styleDeleteExample(ex.id);
                          await loadExamples();
                        }}
                      >
                        Delete
                      </button>
                    </div>
                  </div>
                  {ex.source_context_text && (
                    <p className="mt-2 text-xs text-zinc-500">
                      Context: {ex.source_context_text}
                    </p>
                  )}
                  <p className="mt-1 text-sm">{ex.example_text}</p>
                  {ex.why_it_works && (
                    <p className="mt-1 text-xs text-zinc-500">
                      Why: {ex.why_it_works}
                    </p>
                  )}
                </div>
              ))
            )}
          </section>
        </div>
      )}

      {/* TAB 3: STYLE TEST BENCH */}
      {tab === "bench" && (
        <div className="space-y-4">
          {!hasReplyRoute && (
            <UnsetRouteGuard
              taskType="reply_drafting"
              providers={providers}
              route={replyRoute}
              onAssigned={loadAll}
              onGoToSettings={onOpenSettings}
            />
          )}

          <section className="space-y-3" aria-label="Try your voice">
            <textarea
              aria-label="Sample post text"
              rows={3}
              className={`${inputCls} w-full`}
              placeholder="Paste a post to see how you would reply"
              value={samplePost}
              onChange={(e) => setSamplePost(e.target.value)}
            />
            <div className="flex items-center justify-end gap-3">
              {offline && (
                <span className="text-xs text-zinc-500">Offline</span>
              )}
              <button
                type="button"
                className={primaryBtn}
                disabled={
                  runningBench ||
                  offline ||
                  locked ||
                  !hasReplyRoute ||
                  !samplePost.trim()
                }
                onClick={() => void handleRunBench()}
              >
                {runningBench ? "Writing..." : "Try it"}
              </button>
            </div>
          </section>

          {benchResult && (
            <div className="space-y-4">
              <div
                className={
                  benchResult.comparison
                    ? "grid gap-4 md:grid-cols-2"
                    : "space-y-4"
                }
              >
                <ModelRunColumn title="Primary" run={benchResult.primary} />
                {benchResult.comparison && (
                  <ModelRunColumn
                    title="Comparison"
                    run={benchResult.comparison}
                  />
                )}
              </div>

              <details className={panel}>
                <summary className="cursor-pointer text-sm font-medium">
                  Prompt &amp; Retrieval Inspector (Style v
                  {benchResult.style_guide_version} · Prompt v
                  {benchResult.prompt_version} ·{" "}
                  {benchResult.retrieved_examples.length} examples injected)
                </summary>
                <div className="mt-3 space-y-3 text-xs">
                  <div>
                    <div className="font-medium text-zinc-500">
                      Retrieved Few-Shot Examples
                    </div>
                    {benchResult.retrieved_examples.length === 0 ? (
                      <p className="text-zinc-500">None saved yet.</p>
                    ) : (
                      <ul className="mt-1 space-y-1">
                        {benchResult.retrieved_examples.map((item) => (
                          <li
                            key={item.example.id}
                            className="rounded border border-zinc-200 p-2 dark:border-zinc-800"
                          >
                            <span className="font-medium">{item.reason}:</span>{" "}
                            {item.example.example_text}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                  <div>
                    <div className="font-medium text-zinc-500">
                      Rendered User Prompt
                    </div>
                    <pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap rounded-lg bg-zinc-100 p-3 font-mono text-[11px] dark:bg-zinc-900">
                      {benchResult.rendered_user_prompt}
                    </pre>
                  </div>
                </div>
              </details>
            </div>
          )}
        </div>
      )}
    </Page>
  );
}
