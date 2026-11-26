import { useCallback, useEffect, useState } from "react";
import { ModelPicker, type ModelSelection } from "../components/ModelPicker";
import { UnsetRouteGuard } from "../components/UnsetRouteGuard";
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

const panel = "rounded-xl border border-zinc-200 p-4 dark:border-zinc-800";
const btn =
  "rounded-lg border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800";
const primaryBtn =
  "rounded-lg bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200";
const inputCls =
  "min-w-0 rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900";

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
        <p className="text-xs text-amber-700 dark:text-amber-300">Fallback reason: {run.fallback_reason}</p>
      )}

      {run.variants.length === 0 ? (
        <p className="text-xs text-amber-700 dark:text-amber-300">
          No variants passed the banned-phrase post-filter.
        </p>
      ) : (
        <div className="space-y-2">
          {run.variants.map((v, idx) => (
            <div key={idx} className="rounded-lg border border-zinc-200 p-3 text-sm dark:border-zinc-800">
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
                  {v.check.passed ? " · All rules passed" : ` · ${v.check.violations.join("; ")}`}
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
            {run.rejected_variants.length} variant(s) blocked by banned-phrase post-filter
          </summary>
          <div className="mt-2 space-y-2">
            {run.rejected_variants.map((rv, i) => (
              <div key={i} className="rounded border border-red-200 p-2 dark:border-red-900">
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

export function StylePage({ locked, offline, onReviewRules, onOpenSettings }: Props) {
  const [tab, setTab] = useState<StyleTab>("guide");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Guide state
  const [guide, setGuide] = useState<StyleGuide | null>(null);
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
  const [kindFilter, setKindFilter] = useState<ExampleKind | "all">("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [exKind, setExKind] = useState<ExampleKind>("reply");
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
  const [sampleAuthor, setSampleAuthor] = useState("@founder");
  const [samplePost, setSamplePost] = useState(
    "We cut database query latency by 60% just by batching our dashboard metrics into one materialized view."
  );
  const [overrideSel, setOverrideSel] = useState<ModelSelection>({ providerAccountId: null, modelId: null });
  const [enableCompare, setEnableCompare] = useState(false);
  const [compareSel, setCompareSel] = useState<ModelSelection>({ providerAccountId: null, modelId: null });
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
    const k = kindFilter === "all" ? null : kindFilter;
    if (searchQuery.trim()) {
      const hits = await api.styleSearchExamples(k, searchQuery, 50);
      setExamples(hits.map((h) => h.example));
    } else {
      setExamples(await api.styleListExamples(k));
    }
  }, [kindFilter, searchQuery]);

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
      const found = await api.styleImportExamplesFromBrowser(xHandle.trim() || null, importKind);
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
        sample_author: sampleAuthor.trim() || null,
        sample_post_text: samplePost,
        override_route:
          overrideSel.providerAccountId && overrideSel.modelId
            ? {
                provider_account_id: overrideSel.providerAccountId,
                model_id: overrideSel.modelId,
                reasoning_effort: null,
                temperature: null,
              }
            : null,
        compare_route:
          enableCompare && compareSel.providerAccountId && compareSel.modelId
            ? {
                provider_account_id: compareSel.providerAccountId,
                model_id: compareSel.modelId,
                reasoning_effort: null,
                temperature: null,
              }
            : null,
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
  const hasReplyRoute = Boolean(replyRoute?.provider_account_id && replyRoute?.model_id);
  const hasOverride = Boolean(overrideSel.providerAccountId && overrideSel.modelId);

  return (
    <div className="mx-auto max-w-4xl space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Style Studio</h1>
          <p className="text-xs text-zinc-500">
            Versioned voice guide, FTS5 few-shot examples, banned-phrase enforcement, and Style Test Bench.
          </p>
        </div>
        <div role="tablist" aria-label="Style Studio sections" className="flex gap-1 rounded-xl border border-zinc-200 p-1 dark:border-zinc-800">
          {(
            [
              ["guide", "Voice & Rules"],
              ["examples", `Examples (${examples.length})`],
              ["bench", "Test Bench"],
            ] as Array<[StyleTab, string]>
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              onClick={() => setTab(id)}
              className={`rounded-lg px-3 py-1.5 text-xs font-medium ${
                tab === id
                  ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"
                  : "text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {locked && (
        <div className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-800 dark:bg-amber-950/40 dark:text-amber-200">
          <span>Acknowledge the rules of engagement before running live Style Test Bench or X imports.</span>
          <button type="button" onClick={onReviewRules} className={primaryBtn}>
            Review rules
          </button>
        </div>
      )}

      {error && (
        <div role="alert" className="rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-800 dark:bg-red-950/40 dark:text-red-300">
          {error}
        </div>
      )}
      {notice && (
        <div className="rounded-lg border border-emerald-300 bg-emerald-50 px-3 py-2 text-sm text-emerald-800 dark:border-emerald-800 dark:bg-emerald-950/40 dark:text-emerald-300">
          {notice}
        </div>
      )}

      {/* TAB 1: VOICE & RULES */}
      {tab === "guide" && (
        <div className="space-y-4">
          <section className={panel}>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <h2 className="font-medium">Style Guide</h2>
                <span className="rounded bg-zinc-100 px-2 py-0.5 text-xs dark:bg-zinc-800">
                  {guide && guide.version > 0 ? `v${guide.version} active` : "Unsaved starter"}
                </span>
              </div>
              <button type="button" className={primaryBtn} disabled={savingGuide} onClick={() => void handleSaveGuide()}>
                {savingGuide ? "Saving..." : "Save New Version"}
              </button>
            </div>

            <label className="mt-3 block text-xs font-medium text-zinc-600 dark:text-zinc-400">
              Voice prose
              <textarea
                aria-label="Voice prose"
                rows={5}
                className={`${inputCls} mt-1 w-full`}
                value={prose}
                onChange={(e) => setProse(e.target.value)}
                placeholder="Describe your tone, perspective, pacing, and what good replies sound like."
              />
            </label>

            <div className="mt-4 grid gap-3 sm:grid-cols-3">
              <label className="text-xs font-medium text-zinc-600 dark:text-zinc-400">
                Max reply length (chars)
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
                  onChange={(e) => setEmojiPolicy(e.target.value as EmojiPolicy)}
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
                  onChange={(e) => setHashtagPolicy(e.target.value as HashtagPolicy)}
                >
                  <option value="none">None (0 hashtags)</option>
                  <option value="max_1">Max 1 hashtag</option>
                  <option value="allowed">Allowed</option>
                </select>
              </label>
            </div>

            <div className="mt-4">
              <div className="mb-1 text-xs font-medium text-zinc-600 dark:text-zinc-400">
                Banned phrases &amp; anti-cliché patterns (enforced in prompts &amp; post-filter)
              </div>
              <TagChips
                items={bannedPatterns}
                ariaLabel="Add banned phrase"
                placeholder="e.g. game changer, delve, let's dive in"
                onAdd={(v) =>
                  setBannedPatterns((prev) =>
                    prev.some((x) => x.toLowerCase() === v.toLowerCase()) ? prev : [...prev, v]
                  )
                }
                onRemove={(v) => setBannedPatterns((prev) => prev.filter((x) => x !== v))}
              />
            </div>

            <div className="mt-4">
              <div className="mb-1 text-xs font-medium text-zinc-600 dark:text-zinc-400">Explicit style rules</div>
              <TagChips
                items={learnedRules}
                ariaLabel="Add style rule"
                placeholder="e.g. Lead with one concrete number or trade-off"
                onAdd={(v) =>
                  setLearnedRules((prev) =>
                    prev.some((x) => x.toLowerCase() === v.toLowerCase()) ? prev : [...prev, v]
                  )
                }
                onRemove={(v) => setLearnedRules((prev) => prev.filter((x) => x !== v))}
              />
            </div>

            <div className="mt-4">
              <input
                aria-label="Version note"
                className={`${inputCls} w-full`}
                placeholder="Optional note for this version (e.g. 'Tightened max length to 200')"
                value={versionNote}
                onChange={(e) => setVersionNote(e.target.value)}
              />
            </div>
          </section>

          <section className={panel}>
            <h3 className="font-medium">Version History</h3>
            {versions.length === 0 ? (
              <p className="mt-2 text-xs text-zinc-500">No saved versions yet. Save once to create v1.</p>
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
                      {v.note && <span className="ml-2 text-zinc-600 dark:text-zinc-400">— {v.note}</span>}
                      <div className="mt-0.5 text-zinc-500">
                        Max {v.max_reply_chars} chars · {v.banned_patterns.length} banned pattern(s) · {v.created_at}
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        type="button"
                        className={btn}
                        aria-expanded={diffOpen === v.version}
                        onClick={() => setDiffOpen(diffOpen === v.version ? null : v.version)}
                      >
                        {diffOpen === v.version ? "Hide changes" : "Show changes"}
                      </button>
                      {!v.is_active && (
                        <button type="button" className={btn} onClick={() => void handleActivateVersion(v.version)}>
                          Activate v{v.version}
                        </button>
                      )}
                    </div>
                    {diffOpen === v.version && (
                      <div className="w-full">
                        <p className="mb-1 text-zinc-500">
                          {previousVersion(versions, v.version) ? `Changes from v${previousVersion(versions, v.version)!.version} to v${v.version}` : `Everything in v${v.version} (first saved version)`}
                        </p>
                        <DiffView diff={styleVersionDiff(previousVersion(versions, v.version), v)} label={`Style guide v${v.version} changes`} />
                      </div>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      )}

      {/* TAB 2: FEW-SHOT EXAMPLES */}
      {tab === "examples" && (
        <div className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-2">
              <select
                aria-label="Filter example kind"
                className={inputCls}
                value={kindFilter}
                onChange={(e) => setKindFilter(e.target.value as ExampleKind | "all")}
              >
                <option value="all">All kinds</option>
                <option value="reply">Replies</option>
                <option value="ogc">Original posts (OGC)</option>
              </select>
              <input
                aria-label="Search examples"
                className={`${inputCls} w-64`}
                placeholder="Search examples (FTS5)..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
              />
            </div>
            <button type="button" className={btn} onClick={() => setShowImport((v) => !v)}>
              {showImport ? "Close Import" : "Import Examples"}
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
                  <button type="button" className={btn} onClick={() => void handleParsePaste()}>
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
                    <span>{selectedIdx.size} of {candidates.length} selected</span>
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
                      <li key={idx} className="flex items-start gap-2 rounded-lg border border-zinc-200 p-2 text-xs dark:border-zinc-800">
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
                          {c.author_handle && <span className="font-medium">@{c.author_handle}: </span>}
                          <span>{c.example_text}</span>
                        </div>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </section>
          )}

          <section className={panel}>
            <h2 className="font-medium">Add Few-Shot Example</h2>
            <div className="mt-3 grid gap-2 sm:grid-cols-2">
              <select
                aria-label="New example kind"
                className={inputCls}
                value={exKind}
                onChange={(e) => setExKind(e.target.value as ExampleKind)}
              >
                <option value="reply">Reply example</option>
                <option value="ogc">Original post (OGC) example</option>
              </select>
              <input
                className={inputCls}
                placeholder="Tags (comma-separated, e.g. postgres, latency)"
                value={exTags}
                onChange={(e) => setExTags(e.target.value)}
              />
            </div>
            {exKind === "reply" && (
              <input
                className={`${inputCls} mt-2 w-full`}
                placeholder="Optional: target post you were replying to"
                value={exContext}
                onChange={(e) => setExContext(e.target.value)}
              />
            )}
            <textarea
              rows={3}
              className={`${inputCls} mt-2 w-full`}
              placeholder="Your reply or post text..."
              value={exText}
              onChange={(e) => setExText(e.target.value)}
            />
            <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
              <input
                className={`${inputCls} flex-1`}
                placeholder="Optional: why this works (helps FTS5 & prompt)"
                value={exWhy}
                onChange={(e) => setExWhy(e.target.value)}
              />
              <label className="flex items-center gap-1.5 text-xs">
                <input type="checkbox" checked={exPinned} onChange={(e) => setExPinned(e.target.checked)} />
                Pin in every prompt
              </label>
              <button
                type="button"
                className={primaryBtn}
                disabled={savingExample || !exText.trim()}
                onClick={() => void handleSaveExample()}
              >
                Save Example
              </button>
            </div>
          </section>

          <section className="space-y-2">
            {examples.length === 0 ? (
              <p className="text-sm text-zinc-500">No saved examples match your filter yet.</p>
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
                        <span key={t} className="rounded border border-zinc-200 px-1.5 py-0.5 dark:border-zinc-700">
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
                    <p className="mt-2 text-xs text-zinc-500">Context: {ex.source_context_text}</p>
                  )}
                  <p className="mt-1 text-sm">{ex.example_text}</p>
                  {ex.why_it_works && <p className="mt-1 text-xs text-zinc-500">Why: {ex.why_it_works}</p>}
                </div>
              ))
            )}
          </section>
        </div>
      )}

      {/* TAB 3: STYLE TEST BENCH */}
      {tab === "bench" && (
        <div className="space-y-4">
          {!hasReplyRoute && !hasOverride && (
            <UnsetRouteGuard
              taskType="reply_drafting"
              providers={providers}
              route={replyRoute}
              onAssigned={loadAll}
              onGoToSettings={onOpenSettings}
            />
          )}

          <section className={panel}>
            <h2 className="font-medium">Test Input</h2>
            <div className="mt-3 space-y-3">
              <div className="flex flex-wrap gap-2">
                <input
                  aria-label="Sample author handle"
                  className={`${inputCls} w-44`}
                  placeholder="@author"
                  value={sampleAuthor}
                  onChange={(e) => setSampleAuthor(e.target.value)}
                />
                <input
                  aria-label="Sample post text"
                  className={`${inputCls} flex-1`}
                  placeholder="Paste a sample X post to test reply voice..."
                  value={samplePost}
                  onChange={(e) => setSamplePost(e.target.value)}
                />
              </div>

              <div className="flex flex-wrap items-start justify-between gap-3 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
                <div>
                  <div className="mb-1 text-xs font-medium text-zinc-600 dark:text-zinc-400">
                    Model override for this run (default: {replyRoute?.model_id ?? "Unset"})
                  </div>
                  <ModelPicker
                    providers={providers}
                    value={overrideSel}
                    onChange={setOverrideSel}
                    compact
                    unsetLabel={replyRoute?.model_id ? `Default (${replyRoute.model_id})` : "Use default route"}
                    ariaLabel="Test bench model override"
                    onProvidersUpdated={loadAll}
                  />
                </div>
                <div>
                  <label className="mb-1 flex items-center gap-1.5 text-xs font-medium text-zinc-600 dark:text-zinc-400">
                    <input
                      type="checkbox"
                      checked={enableCompare}
                      onChange={(e) => setEnableCompare(e.target.checked)}
                    />
                    Compare side-by-side with 2nd model
                  </label>
                  {enableCompare && (
                    <ModelPicker
                      providers={providers}
                      value={compareSel}
                      onChange={setCompareSel}
                      compact
                      unsetLabel="Select 2nd model..."
                      ariaLabel="Comparison model"
                    />
                  )}
                </div>
              </div>

              <div className="flex items-center gap-3">
                <button
                  type="button"
                  className={primaryBtn}
                  disabled={runningBench || offline || locked || (!hasReplyRoute && !hasOverride)}
                  onClick={() => void handleRunBench()}
                >
                  {runningBench ? "Generating..." : "Run Style Test Bench"}
                </button>
                {offline && <span className="text-xs text-amber-600 dark:text-amber-400">Disabled while offline</span>}
              </div>
            </div>
          </section>

          {benchResult && (
            <div className="space-y-4">
              <div className={benchResult.comparison ? "grid gap-4 md:grid-cols-2" : "space-y-4"}>
                <ModelRunColumn title="Primary" run={benchResult.primary} />
                {benchResult.comparison && <ModelRunColumn title="Comparison" run={benchResult.comparison} />}
              </div>

              <details className={panel}>
                <summary className="cursor-pointer text-sm font-medium">
                  Prompt &amp; Retrieval Inspector (Style v{benchResult.style_guide_version} · Prompt v
                  {benchResult.prompt_version} · {benchResult.retrieved_examples.length} examples injected)
                </summary>
                <div className="mt-3 space-y-3 text-xs">
                  <div>
                    <div className="font-medium text-zinc-500">Retrieved Few-Shot Examples</div>
                    {benchResult.retrieved_examples.length === 0 ? (
                      <p className="text-zinc-500">None saved yet.</p>
                    ) : (
                      <ul className="mt-1 space-y-1">
                        {benchResult.retrieved_examples.map((item) => (
                          <li key={item.example.id} className="rounded border border-zinc-200 p-2 dark:border-zinc-800">
                            <span className="font-medium">{item.reason}:</span> {item.example.example_text}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                  <div>
                    <div className="font-medium text-zinc-500">Rendered User Prompt</div>
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
    </div>
  );
}
