import { useCallback, useEffect, useState } from "react";
import { p3 } from "./api";
import type { ActivityEntry, ChecklistReport } from "./types";
import { EmptyBlock, PageHeader } from "../components/ui";
import { btn, btnPrimary, input, muted, Notice, panel } from "./ui";

const KIND_LABEL: Record<ActivityEntry["kind"], string> = { scout: "Scout", publish: "Publish", action: "Action", challenge: "Needs you", error: "Error" };
const FILTERS: Array<"all" | ActivityEntry["kind"]> = ["all", "scout", "publish", "action", "challenge", "error"];

function Checklist() {
  const [url, setUrl] = useState("");
  const [reports, setReports] = useState<ChecklistReport[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void p3.checklistReports().then(setReports);
  }, []);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await p3.runChecklist(url.trim());
      setReports((rs) => [r, ...rs]);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className={panel} aria-labelledby="chk-h">
      <h2 id="chk-h" className="font-medium">Live action check</h2>
      <p className={`mt-1 text-sm ${muted}`}>
        Run on a post you own or a throwaway test post. Like, bookmark, repost and follow are done and then undone; reply and quote are checked by hand. The result is saved with the selector version so X changes show up quickly.
      </p>
      <div className="mt-3 flex flex-wrap gap-2">
        <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://x.com/you/status/123..." aria-label="Test post URL" className={`${input} min-w-64 flex-1`} />
        <button type="button" className={btnPrimary} disabled={busy || !url.trim()} onClick={run}>
          {busy ? "Running..." : "Run check"}
        </button>
      </div>
      {error && <div className="mt-3"><Notice tone="error">{error}</Notice></div>}
      {reports.map((r) => (
        <div key={r.id} className="mt-4 text-sm">
          <p className={`text-xs ${muted}`}>
            {new Date(r.created_at).toLocaleString()}, selectors v{r.registry_version}, {r.post_url}
          </p>
          <ul className="mt-1 grid gap-1 sm:grid-cols-2">
            {r.items.map((i) => (
              <li key={i.action} className="flex items-baseline justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-1.5 dark:border-zinc-800">
                <span className="capitalize">{i.action}</span>
                <span className={i.status === "pass" ? "text-emerald-600" : i.status === "fail" ? "text-red-600" : "text-amber-600"} title={i.detail}>
                  {i.status}
                </span>
              </li>
            ))}
          </ul>
        </div>
      ))}
    </section>
  );
}

export function ActivityPage() {
  const [entries, setEntries] = useState<ActivityEntry[]>([]);
  const [filter, setFilter] = useState<(typeof FILTERS)[number]>("all");
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    p3.activity(200).then(setEntries, (e) => setError(String(e.message ?? e)));
  }, []);
  useEffect(load, [load]);

  const shown = entries.filter((e) => filter === "all" || e.kind === filter);

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <PageHeader
        title="Activity"
        description="Everything the app did on X: scout runs, publishes, single actions, challenges and errors."
        actions={<button type="button" className={btn} onClick={load}>Refresh</button>}
      />
      {error && <Notice tone="error">{error}</Notice>}
      <div className="flex flex-wrap gap-2" role="group" aria-label="Filter">
        {FILTERS.map((f) => (
          <button key={f} type="button" aria-pressed={filter === f} onClick={() => setFilter(f)} className={`rounded-full border px-3 py-1 text-sm capitalize ${filter === f ? "border-zinc-900 bg-zinc-900 text-white dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-900" : "border-zinc-300 dark:border-zinc-700"}`}>
            {f === "all" ? "All" : KIND_LABEL[f]}
          </button>
        ))}
      </div>
      {shown.length === 0 ? (
        <EmptyBlock title="Nothing here yet">{filter === "all" ? "Scout runs, publishes and errors will be listed here." : "No entries of this kind yet."}</EmptyBlock>
      ) : (
        <ul className="divide-y divide-zinc-200 rounded-xl border border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
          {shown.map((e) => (
            <li key={e.id} className="flex items-start justify-between gap-3 px-4 py-3 text-sm">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="rounded bg-zinc-100 px-1.5 py-0.5 text-xs dark:bg-zinc-800">{KIND_LABEL[e.kind]}</span>
                  <span className="font-medium">{e.title}</span>
                  {e.ok === false && <span className="text-xs text-red-600">failed</span>}
                  {e.ok === true && <span className="text-xs text-emerald-600">ok</span>}
                </div>
                {e.detail && <p className={`mt-1 ${muted}`}>{e.detail}</p>}
                {e.screenshot && <p className="mt-1 break-all text-xs underline">{e.screenshot}</p>}
              </div>
              <time className={`shrink-0 text-xs ${muted}`} dateTime={e.at}>
                {new Date(e.at).toLocaleString()}
              </time>
            </li>
          ))}
        </ul>
      )}
      <Checklist />
    </div>
  );
}
