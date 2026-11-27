import { useEffect, useState } from "react";
import { p3 } from "./api";
import type { NotificationSettings, PacingConfig, PacingUsage } from "./types";
import { btn, input, muted, Notice, panel } from "./ui";

const CAP_FIELDS: Array<keyof Omit<PacingConfig, "enabled" | "min_gap_sec">> = ["like", "reply", "repost", "quote", "bookmark", "follow"];
const NOTIFY_FIELDS: Array<[keyof Omit<NotificationSettings, "enabled">, string]> = [
  ["scout_done", "Scout finished with new queue items"],
  ["publish_done", "A publish finished"],
  ["publish_failed", "A publish stopped"],
  ["attention", "X needs you (login or challenge)"],
];

export function PublishSettings() {
  const [pacing, setPacing] = useState<PacingConfig | null>(null);
  const [usage, setUsage] = useState<PacingUsage[]>([]);
  const [notify, setNotify] = useState<NotificationSettings | null>(null);
  const [retention, setRetention] = useState(30);
  const [watch, setWatch] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void p3.pacing().then(setPacing);
    void p3.pacingUsage().then(setUsage);
    void p3.notifications().then(setNotify);
    void p3.retentionDays().then(setRetention);
    void p3.watchlist().then((w) => setWatch(w.join(", ")));
  }, []);

  const guard = async (fn: () => Promise<unknown>, ok: string) => {
    setError(null);
    try {
      await fn();
      setMsg(ok);
    } catch (e) {
      setMsg(null);
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  if (!pacing || !notify) return null;
  const used = (a: string) => usage.find((u) => u.action === a)?.used_24h ?? 0;

  return (
    <div className="space-y-4">
      {msg && <Notice tone="ok">{msg}</Notice>}
      {error && <Notice tone="error">{error}</Notice>}

      <section className={panel} aria-labelledby="pace-h">
        <div className="flex items-center justify-between">
          <h2 id="pace-h" className="font-display text-lg font-semibold tracking-tight">Pacing limits</h2>
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={pacing.enabled} onChange={(e) => setPacing({ ...pacing, enabled: e.target.checked })} />
            Enforce daily limits
          </label>
        </div>
        <p className={`mt-1 text-sm ${muted}`}>Optional daily caps. 0 means no cap.</p>
        <div className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-3">
          {CAP_FIELDS.map((f) => (
            <label key={f} className="text-sm">
              <span className="block capitalize">{f} per day <span className={`text-xs ${muted}`}>({used(f)} used)</span></span>
              <input type="number" min={0} max={10000} value={pacing[f]} onChange={(e) => setPacing({ ...pacing, [f]: Number(e.target.value) })} className={`${input} mt-1 w-full`} />
            </label>
          ))}
          <label className="text-sm">
            <span className="block">Gap between text posts (sec)</span>
            <input type="number" min={0} max={3600} value={pacing.min_gap_sec} onChange={(e) => setPacing({ ...pacing, min_gap_sec: Number(e.target.value) })} className={`${input} mt-1 w-full`} />
          </label>
        </div>
        <button type="button" className={`${btn} mt-3`} onClick={() => void guard(async () => setPacing(await p3.setPacing(pacing)), "Pacing saved.")}>
          Save pacing
        </button>
      </section>

      <section className={panel} aria-labelledby="notify-h">
        <h2 id="notify-h" className="font-display text-lg font-semibold tracking-tight">Notifications</h2>
        <label className="mt-2 flex items-center gap-2 text-sm">
          <input type="checkbox" checked={notify.enabled} onChange={(e) => setNotify({ ...notify, enabled: e.target.checked })} />
          Show desktop notifications
        </label>
        <div className="mt-2 space-y-1 pl-6">
          {NOTIFY_FIELDS.map(([k, label]) => (
            <label key={k} className="flex items-center gap-2 text-sm">
              <input type="checkbox" disabled={!notify.enabled} checked={notify[k]} onChange={(e) => setNotify({ ...notify, [k]: e.target.checked })} />
              {label}
            </label>
          ))}
        </div>
        <button type="button" className={`${btn} mt-3`} onClick={() => void guard(async () => setNotify(await p3.setNotifications(notify)), "Notification settings saved.")}>
          Save notifications
        </button>
      </section>

      <section className={panel} aria-labelledby="watch-h">
        <h2 id="watch-h" className="font-display text-lg font-semibold tracking-tight">Watchlist</h2>
        <p className={`mt-1 text-sm ${muted}`}>Handles to scan, separated by commas.</p>
        <textarea value={watch} onChange={(e) => setWatch(e.target.value)} rows={2} aria-label="Watchlist handles" className={`${input} mt-2 w-full`} />
        <button
          type="button"
          className={`${btn} mt-2`}
          onClick={() =>
            void guard(async () => {
              const list = await p3.setWatchlist(watch.split(/[\s,;]+/).map((h) => h.replace(/^@/, "")).filter(Boolean));
              setWatch(list.join(", "));
            }, "Watchlist saved.")
          }
        >
          Save watchlist
        </button>
      </section>

      <section className={panel} aria-labelledby="ret-h">
        <h2 id="ret-h" className="font-display text-lg font-semibold tracking-tight">Screenshot retention</h2>
        <p className={`mt-1 text-sm ${muted}`}>Publish screenshots are deleted after this many days.</p>
        <div className="mt-2 flex items-center gap-2">
          <input type="number" min={1} max={365} value={retention} onChange={(e) => setRetention(Number(e.target.value))} aria-label="Retention days" className={`${input} w-24`} />
          <span className="text-sm">days</span>
          <button type="button" className={btn} onClick={() => void guard(async () => setRetention(await p3.setRetentionDays(retention)), "Retention saved. Older files are removed on the next start.")}>
            Save
          </button>
        </div>
      </section>
    </div>
  );
}
