import { useCallback, useEffect, useState } from "react";
import { p3 } from "./api";
import type { ActivityEntry } from "./types";
import { EmptyBlock, Page, PageHeader } from "../components/ui";
import { muted, Notice } from "./ui";

const KIND_LABEL: Record<ActivityEntry["kind"], string> = { scout: "Scout", publish: "Publish", action: "Action", challenge: "Needs you", error: "Error" };

export function ActivityPage() {
  const [entries, setEntries] = useState<ActivityEntry[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    p3.activity(200).then(
      (e) => {
        setEntries(e);
        setLoaded(true);
      },
      (e) => setError(String(e.message ?? e)),
    );
  }, []);
  useEffect(() => {
    load();
    const t = window.setInterval(load, 10000);
    return () => window.clearInterval(t);
  }, [load]);

  return (
    <Page>
      <PageHeader title="Activity" description="What the app did on X." />
      {error && <Notice tone="error">{error}</Notice>}
      {loaded && entries.length === 0 ? (
        <EmptyBlock title="Nothing yet">Scouting, publishing and errors show up here.</EmptyBlock>
      ) : (
        <ul className="divide-y divide-zinc-200 border-y border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
          {entries.map((e) => (
            <li key={e.id} className="flex items-start justify-between gap-4 py-3.5 text-sm">
              <div className="min-w-0">
                <p className="flex items-center gap-2">
                  <span aria-hidden="true" className={`h-1.5 w-1.5 shrink-0 rounded-full ${e.ok === false || e.kind === "error" ? "bg-red-600" : e.kind === "challenge" ? "bg-amber-500" : "bg-zinc-400"}`} />
                  <span className="font-medium">{e.title}</span>
                  <span className="eyebrow">{KIND_LABEL[e.kind]}</span>
                </p>
                {e.detail && <p className={`mt-1 ${muted}`}>{e.detail}</p>}
                {e.screenshot && <p className="mt-1 text-xs break-all underline">{e.screenshot}</p>}
              </div>
              <time className={`shrink-0 font-mono text-xs ${muted}`} dateTime={e.at}>
                {new Date(e.at).toLocaleString()}
              </time>
            </li>
          ))}
        </ul>
      )}
    </Page>
  );
}
