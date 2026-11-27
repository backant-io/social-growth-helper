import { useEffect, useState } from "react";
import { p3 } from "./api";
import type { ChecklistReport } from "./types";
import { btnPrimary, input, muted, Notice } from "./ui";

export function Checklist() {
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
    <section aria-label="Live action check">
      <p className={`text-sm ${muted}`}>Run on a post you own or a throwaway test post. Likes, bookmarks, reposts and follows are done and then undone.</p>
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
