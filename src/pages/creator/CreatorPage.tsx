import { useCallback, useEffect, useRef, useState } from "react";
import { InlineModelGuard } from "../../components/InlineModelGuard";
import { btn, btnGhost, btnPrimary, EmptyBlock, LoadingBlock, muted, Notice, Page, PageHeader } from "../../components/ui";
import { XPostCard } from "../../components/XPostCard";
import { api, inTauri } from "../../lib/api";
import { candidateInsights, percent, xPostUrl } from "../../lib/creator";
import { creatorApi } from "../../lib/creatorApi";
import type { CreatorSettings, FormatCandidate, FormatRecord, OgcDraft } from "../../lib/types";
import { DraftCard } from "./DraftCard";
import { SourcesPanel } from "./SourcesPanel";

const VISIBLE_POSTS = 4;
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

function TemplateCard({ format, selected, onToggle }: { format: FormatRecord; selected: boolean; onToggle: () => void }) {
  const win = format.personal_win_rate ?? format.observed_win_rate;
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={selected}
      onClick={onToggle}
      className={`flex h-full flex-col rounded-2xl border p-4 text-left transition-colors ${selected ? "border-zinc-950 bg-zinc-100 dark:border-zinc-100 dark:bg-zinc-900" : "border-zinc-200 hover:border-zinc-400 dark:border-zinc-800 dark:hover:border-zinc-600"}`}
    >
      <span className="flex items-start justify-between gap-2">
        <span className="font-display text-sm leading-snug font-semibold">{format.name}</span>
        <span
          aria-hidden="true"
          className={`mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded border text-[10px] ${selected ? "border-zinc-950 bg-zinc-950 text-white dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-950" : "border-zinc-300 dark:border-zinc-600"}`}
        >
          {selected ? "✓" : ""}
        </span>
      </span>
      <span className={`mt-1 text-xs leading-relaxed ${muted}`}>{format.hook_formula}</span>
      <span className="mt-3 line-clamp-4 rounded-lg bg-zinc-100 px-2.5 py-2 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-zinc-700 dark:bg-zinc-900 dark:text-zinc-300">{format.skeleton}</span>
      <span className={`mt-auto pt-3 font-mono text-[11px] ${muted}`}>
        {win != null ? `${percent(win)} worked` : "New"} · seen {format.observation_count}x
      </span>
    </button>
  );
}

function Insights({ c }: { c: FormatCandidate }) {
  const items = candidateInsights(c);
  return (
    <ul className="flex flex-wrap gap-1.5" aria-label="Why it worked">
      {items.map((i) => (
        <li key={i.label} className="rounded-full border border-zinc-200 px-2.5 py-0.5 text-xs dark:border-zinc-700">
          <span className={muted}>{i.label} </span>
          <span className="font-mono font-medium">{i.value}</span>
        </li>
      ))}
    </ul>
  );
}

export function CreatorPage({ locked, onGoToSettings }: { locked: boolean; onGoToSettings?: () => void }) {
  const capi = creatorApi();
  const [formats, setFormats] = useState<FormatRecord[]>([]);
  const [posts, setPosts] = useState<FormatCandidate[]>([]);
  const [drafts, setDrafts] = useState<OgcDraft[]>([]);
  const [settings, setSettings] = useState<CreatorSettings>({ similarity_threshold: 0.5, max_chars: 280 });
  const [picked, setPicked] = useState<string[]>([]);
  const [topic, setTopic] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [sourcesOpen, setSourcesOpen] = useState(false);
  const [xRunning, setXRunning] = useState(false);
  const draftsRef = useRef(drafts);
  draftsRef.current = drafts;
  const examples = useRef<HTMLElement>(null);

  const load = useCallback(async () => {
    const [f, pending, done, d, s] = await Promise.all([capi.formats("working", false), capi.candidates("pending"), capi.candidates("extracted"), capi.drafts(), capi.settings()]);
    setFormats(f);
    setPosts([...pending, ...done].sort((a, b) => b.virality_score - a.virality_score));
    setDrafts(d);
    setSettings(s);
    setPicked((cur) => cur.filter((id) => f.some((x) => x.id === id)));
  }, [capi]);

  useEffect(() => {
    load()
      .catch((e) => setError(message(e)))
      .finally(() => setLoaded(true));
  }, [load]);

  useEffect(() => {
    let alive = true;
    const check = () => api.browserInfo().then((i) => alive && setXRunning(i.running)).catch(() => undefined);
    void check();
    const t = setInterval(check, 5000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  useEffect(() => {
    if (!inTauri()) return;
    let off: (() => void) | undefined;
    let gone = false;
    import("@tauri-apps/api/webview").then(async ({ getCurrentWebview }) => {
      const un = await getCurrentWebview().onDragDropEvent(async (event) => {
        if (event.payload.type !== "drop") return;
        const ratio = window.devicePixelRatio || 1;
        const el = document.elementFromPoint(event.payload.position.x / ratio, event.payload.position.y / ratio);
        const id = el?.closest("[data-draft-id]")?.getAttribute("data-draft-id");
        const target = draftsRef.current.find((d) => d.id === id);
        if (!target) return;
        try {
          const updated = await capi.setMedia(target.id, [...target.media_paths, ...event.payload.paths]);
          setDrafts((cur) => cur.map((d) => (d.id === updated.id ? updated : d)));
        } catch (e) {
          setError(message(e));
        }
      });
      if (gone) un();
      else off = un;
    });
    return () => {
      gone = true;
      off?.();
    };
  }, [capi]);

  async function run(label: string, fn: () => Promise<void>) {
    setBusy(label);
    setError(null);
    setNotice(null);
    try {
      await fn();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(null);
    }
  }

  const toggle = (id: string) => setPicked((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));
  const showExamples = () => setTimeout(() => examples.current?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);

  const generate = () =>
    run("generate", async () => {
      for (const id of picked) {
        const g = await capi.generate(id, topic);
        setDrafts((cur) => [...g.drafts, ...cur]);
      }
      showExamples();
    });

  const vary = (c: FormatCandidate) =>
    run(`vary-${c.id}`, async () => {
      const g = await capi.variation(c.id);
      setDrafts((cur) => [...g.drafts, ...cur]);
      await load();
      setNotice(`${g.drafts.length} variations of @${c.author_handle}'s post are ready under Examples.`);
      showExamples();
    });

  const createTemplates = () =>
    run("templates", async () => {
      const r = await capi.extractPending();
      await load();
      if (r.stopped_reason) setError(r.stopped_reason);
      setNotice(r.created + r.merged > 0 ? `${r.created} new ${r.created === 1 ? "template" : "templates"}${r.merged ? `, ${r.merged} matched ones you already have` : ""}.` : r.stopped_reason ? null : "Nothing new to learn from yet.");
    });

  const findViral = () =>
    run("find", async () => {
      const list = await capi.watchlist();
      if (list.length === 0) {
        setSourcesOpen(true);
        setNotice("Add a few creators to learn from below, then try again. Posts Scout finds also show up here.");
        return;
      }
      if (!xRunning) throw new Error("Open the X window first (Settings), then find more viral formats.");
      let fresh = 0;
      for (const w of list) fresh += (await capi.watchScan(w.id)).new_candidates;
      await load();
      setNotice(fresh > 0 ? `Found ${fresh} new viral ${fresh === 1 ? "post" : "posts"} from ${list.length} ${list.length === 1 ? "creator" : "creators"}. Press Create more templates to learn their formats.` : `Checked ${list.length} ${list.length === 1 ? "creator" : "creators"}. Nothing new went viral.`);
    });

  const dismiss = (c: FormatCandidate) =>
    run(`dismiss-${c.id}`, async () => {
      await capi.dismissCandidate(c.id);
      setPosts((cur) => cur.filter((x) => x.id !== c.id));
    });

  const pending = posts.filter((p) => p.status === "pending").length;
  const byId = (id: string | null) => formats.find((f) => f.id === id);
  const shown = showAll ? posts : posts.slice(0, VISIBLE_POSTS);
  const working = busy !== null;

  if (!loaded) {
    return (
      <Page>
        <PageHeader title="Creator" />
        <LoadingBlock label="Loading formats" />
      </Page>
    );
  }

  return (
    <Page>
      <PageHeader
        title="Creator"
        description="Pick formats that work, generate examples, edit, post."
        actions={
          <>
            <button type="button" className={btn} disabled={working} onClick={() => void findViral()}>
              {busy === "find" ? "Looking..." : "Find more viral formats"}
            </button>
            <button type="button" className={btn} disabled={working || pending === 0} onClick={() => void createTemplates()} title={pending === 0 ? "No viral posts are waiting to be learned from" : undefined}>
              {busy === "templates" ? "Learning..." : pending > 0 ? `Create more templates (${pending})` : "Create more templates"}
            </button>
          </>
        }
      />

      <InlineModelGuard taskType="ogc_generation" onGoToSettings={onGoToSettings} onAssigned={() => undefined} />
      {error && <Notice tone="error">{error}</Notice>}
      {notice && <Notice tone="ok">{notice}</Notice>}

      <section aria-labelledby="tpl-h" className="space-y-3">
        <h2 id="tpl-h" className="font-display text-base font-semibold tracking-tight">
          1. Templates that work
        </h2>
        {formats.length === 0 ? (
          <EmptyBlock title="No templates yet">Find viral formats from creators you follow, then press Create more templates to turn their posts into reusable templates.</EmptyBlock>
        ) : (
          <>
            <ul className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3" role="group" aria-label="Templates">
              {formats.map((f) => (
                <li key={f.id}>
                  <TemplateCard format={f} selected={picked.includes(f.id)} onToggle={() => toggle(f.id)} />
                </li>
              ))}
            </ul>
            <div className="flex flex-wrap items-center gap-3">
              <label className="min-w-56 flex-1">
                <span className="sr-only">About (optional)</span>
                <input
                  value={topic}
                  maxLength={300}
                  onChange={(e) => setTopic(e.target.value)}
                  placeholder="About... (optional, defaults to your topics)"
                  className="w-full rounded-lg border border-zinc-300 bg-transparent px-3 py-1.5 text-sm placeholder:text-zinc-400 focus:border-zinc-900 focus:outline-none dark:border-zinc-700 dark:focus:border-zinc-100"
                />
              </label>
              <button type="button" className={btnPrimary} disabled={working || locked || picked.length === 0} onClick={() => void generate()}>
                {busy === "generate" ? "Writing..." : picked.length > 0 ? `Generate examples (${picked.length})` : "Select templates to generate"}
              </button>
            </div>
          </>
        )}
      </section>

      {drafts.length > 0 && (
        <section ref={examples} aria-labelledby="ex-h" className="scroll-mt-4 space-y-3">
          <h2 id="ex-h" className="font-display text-base font-semibold tracking-tight">
            2. Examples
          </h2>
          <p className={`text-sm ${muted}`}>Edit any example, then post it. Nothing goes to X until you confirm the exact text.</p>
          <div className="space-y-3" aria-label="Drafts">
            {drafts.map((d) => (
              <DraftCard
                key={d.id}
                draft={d}
                format={byId(d.viral_format_id)}
                threshold={settings.similarity_threshold}
                maxChars={settings.max_chars}
                canPublish={xRunning && !locked}
                publishBlock={!xRunning ? "Open the X window first (Settings)" : undefined}
                onChanged={(u) => setDrafts((cur) => cur.map((x) => (x.id === u.id ? u : x)))}
                onRemoved={(id) => setDrafts((cur) => cur.filter((x) => x.id !== id))}
              />
            ))}
          </div>
        </section>
      )}

      {posts.length > 0 && (
        <section aria-labelledby="posts-h" className="space-y-3">
          <h2 id="posts-h" className="font-display text-base font-semibold tracking-tight">
            Viral posts behind them
          </h2>
          <ul className="divide-y divide-zinc-200 border-y border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
            {shown.map((c) => (
              <li key={c.id}>
                <XPostCard
                  handle={c.author_handle}
                  name={c.author_name || undefined}
                  avatarUrl={c.avatar_url}
                  postedAt={c.posted_at}
                  text={c.text}
                  media={c.media}
                  url={xPostUrl(c.author_handle, c.platform_post_id)}
                  metrics={c.metrics}
                  compact
                >
                  <div className="space-y-3">
                    <Insights c={c} />
                    {c.reasons.length > 0 && <p className={`text-xs ${muted}`}>{c.reasons.slice(0, 2).join(". ")}</p>}
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <span className={`text-xs ${muted}`}>{byId(c.format_id) ? `Template: ${byId(c.format_id)!.name}` : "Template not created yet"}</span>
                      <div className="flex items-center gap-1.5">
                        <button type="button" className={btnGhost} disabled={working} onClick={() => void dismiss(c)}>
                          Dismiss
                        </button>
                        <button type="button" className={btnPrimary} disabled={working || locked} onClick={() => void vary(c)}>
                          {busy === `vary-${c.id}` ? "Writing..." : "Create a variation"}
                        </button>
                      </div>
                    </div>
                  </div>
                </XPostCard>
              </li>
            ))}
          </ul>
          {posts.length > VISIBLE_POSTS && (
            <button type="button" className={btnGhost} onClick={() => setShowAll((v) => !v)}>
              {showAll ? "Show fewer" : `Show ${posts.length - VISIBLE_POSTS} more`}
            </button>
          )}
        </section>
      )}

      <details open={sourcesOpen} onToggle={(e) => setSourcesOpen((e.target as HTMLDetailsElement).open)} className="group border-t border-zinc-200 pt-4 dark:border-zinc-800">
        <summary className="eyebrow flex cursor-pointer list-none items-center justify-between hover:text-zinc-950 dark:hover:text-zinc-50">
          Creators and settings
          <span aria-hidden="true" className="transition-transform group-open:rotate-45">
            +
          </span>
        </summary>
        <div className="mt-4">
          <SourcesPanel locked={locked} xRunning={xRunning} />
        </div>
      </details>
    </Page>
  );
}
