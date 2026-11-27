import { useCallback, useEffect, useState } from "react";
import { percent } from "../../lib/creator";
import { creatorApi } from "../../lib/creatorApi";
import type { FormatCandidate, FormatRecord, FormatSort, ManualPost } from "../../lib/types";
import { FormatEditor } from "./FormatEditor";
import { EmptyBlock, LoadingBlock, Notice } from "../../components/ui";
import { btn, field, muted, panel, primary } from "./ui";

const EMPTY_POST: ManualPost = { url: "", text: "", likes: 0, reposts: 0, replies: 0, bookmarks: 0, views: null, author_followers: null };

function num(v: string): number {
  const n = Number(v.replace(/[,\s]/g, ""));
  return Number.isFinite(n) && n >= 0 ? Math.floor(n) : 0;
}

function FormatCard({ f, onEdit, onToggle }: { f: FormatRecord; onEdit: () => void; onToggle: () => void }) {
  return (
    <article className={panel} aria-label={f.name}>
      <header className="flex items-start justify-between gap-2">
        <div>
          <h3 className="font-medium">{f.name}</h3>
          <p className={`text-xs ${muted}`}>
            {f.archetype.replace(/_/g, " ")} · {f.origin === "manual" ? "added by hand" : `seen ${f.observation_count}x`}
            {f.recent_sightings > 0 && ` · ${f.recent_sightings} in the last 2 weeks`}
            {f.status === "archived" && " · archived"}
          </p>
        </div>
        <div className="flex gap-1.5">
          <button type="button" className={btn} onClick={onEdit}>Edit</button>
          <button type="button" className={btn} onClick={onToggle}>{f.status === "active" ? "Archive" : "Restore"}</button>
        </div>
      </header>
      <p className="mt-2 text-sm">{f.hook_formula}</p>
      <pre className="mt-2 whitespace-pre-wrap rounded-lg bg-zinc-50 p-3 font-sans text-sm dark:bg-zinc-900">{f.skeleton}</pre>
      <dl className="mt-2 flex flex-wrap gap-x-6 gap-y-1 text-xs">
        <div><dt className="inline text-zinc-500">On X: </dt><dd className="inline">{percent(f.observed_win_rate)}</dd></div>
        <div><dt className="inline text-zinc-500">For you: </dt><dd className="inline">{f.personal_win_rate === null ? "not enough posts yet" : percent(f.personal_win_rate)}</dd></div>
      </dl>
      {f.sources.length > 0 && (
        <details className="mt-2 text-xs">
          <summary className="cursor-pointer text-zinc-500">Source posts ({f.sources.length})</summary>
          <ul className="mt-1 space-y-0.5">
            {f.sources.map((s) => (
              <li key={s.post_id}>
                <a className="underline" href={s.url} target="_blank" rel="noreferrer">@{s.author}</a> <span className={muted}>score {Math.round(s.score)}</span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </article>
  );
}

function CandidateCard({ c, busy, onStudy, onDismiss }: { c: FormatCandidate; busy: boolean; onStudy: () => void; onDismiss: () => void }) {
  return (
    <article className={panel} aria-label={`Post by ${c.author_handle}`}>
      <header className="flex items-start justify-between gap-2 text-xs">
        <span className={muted}>
          <a className="underline" href={`https://x.com/${c.author_handle}/status/${c.platform_post_id}`} target="_blank" rel="noreferrer">@{c.author_handle}</a>
          {" · "}{c.source === "manual" ? "added by hand" : c.source === "watchlist" ? "from your watchlist" : "from Scout"}
          {c.virality_score > 0 && ` · score ${Math.round(c.virality_score)}`}
        </span>
        <span className={muted}>{c.metrics.likes} likes · {c.metrics.reposts} reposts · {c.metrics.bookmarks} bookmarks</span>
      </header>
      <p className="mt-2 whitespace-pre-wrap text-sm">{c.text}</p>
      {c.reasons.length > 0 && <ul className={`mt-2 list-disc pl-5 text-xs ${muted}`}>{c.reasons.map((r) => <li key={r}>{r}</li>)}</ul>}
      <footer className="mt-3 flex justify-end gap-2">
        <button type="button" className={btn} disabled={busy} onClick={onDismiss}>Dismiss</button>
        <button type="button" className={primary} disabled={busy} onClick={onStudy}>Turn into a format</button>
      </footer>
    </article>
  );
}

export function FormatsPanel() {
  const api = creatorApi();
  const [formats, setFormats] = useState<FormatRecord[]>([]);
  const [candidates, setCandidates] = useState<FormatCandidate[]>([]);
  const [sort, setSort] = useState<FormatSort>("working");
  const [archived, setArchived] = useState(false);
  const [editing, setEditing] = useState<FormatRecord | "new" | null>(null);
  const [post, setPost] = useState<ManualPost>(EMPTY_POST);
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  const load = useCallback(async () => {
    const [f, c] = await Promise.all([api.formats(sort, archived), api.candidates("pending")]);
    setFormats(f);
    setCandidates(c);
  }, [api, sort, archived]);

  useEffect(() => {
    load()
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoaded(true));
  }, [load]);

  async function run(fn: () => Promise<void>) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await fn();
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  if (!loaded) return error ? <Notice tone="error">{error}</Notice> : <LoadingBlock label="Loading formats" />;

  return (
    <div className="space-y-6">
      {error && <Notice tone="error">{error}</Notice>}
      {notice && <Notice tone="ok">{notice}</Notice>}
      {loaded && formats.length === 0 && candidates.length === 0 && (
        <Notice tone="info">A format is a reusable shape for a post, such as a hook plus a structure, taken from posts that did well. Formats only guide drafts in the Write tab. They never post anything.</Notice>
      )}

      <section aria-labelledby="lib-h" className="space-y-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 id="lib-h" className="font-display text-lg font-semibold tracking-tight">Format library</h2>
          <div className="flex flex-wrap items-center gap-2 text-sm">
            <label className="flex items-center gap-1.5">
              <span className={muted}>Sort</span>
              <select aria-label="Sort formats" className={`${field} w-auto`} value={sort} onChange={(e) => setSort(e.target.value as FormatSort)}>
                <option value="working">Working for you</option>
                <option value="rising">Rising</option>
                <option value="recent">Newest</option>
              </select>
            </label>
            <label className="flex items-center gap-1.5">
              <input type="checkbox" checked={archived} onChange={(e) => setArchived(e.target.checked)} /> Show archived
            </label>
            <button type="button" className={primary} onClick={() => setEditing("new")}>New format</button>
          </div>
        </div>
        {formats.length === 0 ? (
          <EmptyBlock title="No formats yet" action={<button type="button" className={btn} onClick={() => setEditing("new")}>Write your own</button>}>Turn a post below into one, or write your own.</EmptyBlock>
        ) : (
          formats.map((f) => (
            <FormatCard
              key={f.id}
              f={f}
              onEdit={() => setEditing(f)}
              onToggle={() => run(() => api.setFormatStatus(f.id, f.status === "active" ? "archived" : "active"))}
            />
          ))
        )}
      </section>

      <section aria-labelledby="cand-h" className="space-y-3">
        <div className="flex items-center justify-between gap-2">
          <h2 id="cand-h" className="font-display text-lg font-semibold tracking-tight">Posts worth studying</h2>
          {candidates.length > 1 && (
            <button
              type="button"
              className={btn}
              disabled={busy}
              onClick={() =>
                run(async () => {
                  const s = await api.extractPending();
                  setNotice(`${s.created} new, ${s.merged} merged into existing formats, ${s.failed} failed.`);
                  if (s.stopped_reason) setError(s.stopped_reason);
                })
              }
            >
              Study all
            </button>
          )}
        </div>
        {candidates.length === 0 ? (
          <EmptyBlock title="Nothing waiting">Posts that over-perform show up here after a watchlist scan, or add one by hand below.</EmptyBlock>
        ) : (
          candidates.map((c) => (
            <CandidateCard
              key={c.id}
              c={c}
              busy={busy}
              onDismiss={() => run(() => api.dismissCandidate(c.id))}
              onStudy={() =>
                run(async () => {
                  const r = await api.extract(c.id);
                  setNotice(r.merged ? `Merged into "${r.format.name}".` : `New format: "${r.format.name}".`);
                })
              }
            />
          ))
        )}

        <details className={panel}>
          <summary className="cursor-pointer text-sm font-medium">Add a post by hand</summary>
          <form
            className="mt-3 space-y-3 text-sm"
            onSubmit={async (e) => {
              e.preventDefault();
              setAdding(true);
              await run(async () => {
                const d = await api.addPost(post);
                setNotice(d.already_known ? "That post is already in your list." : "Added. Turn it into a format when you are ready.");
                setPost(EMPTY_POST);
              });
              setAdding(false);
            }}
          >
            <label className="block">
              <span className="mb-1 block font-medium">Link to the post</span>
              <input className={field} value={post.url} onChange={(e) => setPost({ ...post, url: e.target.value })} placeholder="https://x.com/name/status/123456789" required />
            </label>
            <label className="block">
              <span className="mb-1 block font-medium">Post text</span>
              <textarea className={`${field} min-h-20`} value={post.text} onChange={(e) => setPost({ ...post, text: e.target.value })} required />
            </label>
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
              {(["likes", "reposts", "replies", "bookmarks"] as const).map((k) => (
                <label key={k} className="block">
                  <span className="mb-1 block capitalize text-zinc-500">{k}</span>
                  <input className={field} inputMode="numeric" value={post[k] || ""} onChange={(e) => setPost({ ...post, [k]: num(e.target.value) })} placeholder="0" />
                </label>
              ))}
            </div>
            <p className={`text-xs ${muted}`}>Text and numbers come from you, so nothing is read from X. Posts you add by hand skip the viral thresholds.</p>
            <button type="submit" className={primary} disabled={busy || adding || !post.url.trim() || !post.text.trim()}>Add post</button>
          </form>
        </details>
      </section>

      {editing && (
        <FormatEditor
          initial={editing === "new" ? null : editing}
          onCancel={() => setEditing(null)}
          onSave={async (bp) => {
            if (editing === "new") await api.createFormat(bp);
            else await api.updateFormat(editing.id, bp);
            setEditing(null);
            await load();
          }}
        />
      )}
    </div>
  );
}
