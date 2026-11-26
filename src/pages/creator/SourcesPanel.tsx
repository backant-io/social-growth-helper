import { useCallback, useEffect, useState } from "react";
import { creatorApi } from "../../lib/creatorApi";
import type { CreatorSettings, ViralThresholds, WatchEntry } from "../../lib/types";
import { btn, danger, field, muted, panel, primary } from "./ui";

interface Props {
  locked: boolean;
  xRunning: boolean;
}

export function SourcesPanel({ locked, xRunning }: Props) {
  const api = creatorApi();
  const [watch, setWatch] = useState<WatchEntry[]>([]);
  const [handle, setHandle] = useState("");
  const [followers, setFollowers] = useState("");
  const [thresholds, setThresholds] = useState<ViralThresholds | null>(null);
  const [settings, setSettings] = useState<CreatorSettings | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    const [w, t, s] = await Promise.all([api.watchlist(), api.thresholds(), api.settings()]);
    setWatch(w);
    setThresholds(t);
    setSettings(s);
  }, [api]);

  useEffect(() => {
    void load();
  }, [load]);

  async function run(label: string, fn: () => Promise<void>) {
    setBusy(label);
    setError(null);
    setNotice(null);
    try {
      await fn();
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  const numField = (label: string, value: number, apply: (n: number) => void, step = 1) => (
    <label className="block text-sm">
      <span className="mb-1 block text-zinc-500">{label}</span>
      <input type="number" step={step} className={field} value={value} onChange={(e) => apply(Number(e.target.value))} />
    </label>
  );

  return (
    <div className="space-y-4">
      {error && <p role="alert" className={`text-sm ${danger}`}>{error}</p>}
      {notice && <p role="status" className="text-sm text-emerald-600 dark:text-emerald-400">{notice}</p>}

      <section className={panel} aria-labelledby="wl-h">
        <h2 id="wl-h" className="font-medium">Creator watchlist</h2>
        <p className={`mt-1 text-sm ${muted}`}>Creators whose posts you want to learn from. A scan opens one creator's profile in your X window, reads what is on screen and flags posts that over-perform. It only runs when you press Scan.</p>
        <form
          className="mt-3 flex flex-wrap gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void run("add", async () => {
              await api.watchAdd(handle, followers.trim() ? Number(followers.replace(/[,\s]/g, "")) : null);
              setHandle("");
              setFollowers("");
            });
          }}
        >
          <input aria-label="Creator handle" className={`${field} w-48`} value={handle} onChange={(e) => setHandle(e.target.value)} placeholder="@handle" />
          <input aria-label="Follower count (optional)" className={`${field} w-48`} inputMode="numeric" value={followers} onChange={(e) => setFollowers(e.target.value)} placeholder="Followers (optional)" />
          <button type="submit" className={primary} disabled={busy !== null || !handle.trim()}>Add creator</button>
        </form>
        {watch.length === 0 ? (
          <p className={`mt-3 text-sm ${muted}`}>No creators yet.</p>
        ) : (
          <ul className="mt-3 divide-y divide-zinc-200 text-sm dark:divide-zinc-800">
            {watch.map((w) => (
              <li key={w.id} className="flex items-center justify-between gap-2 py-2">
                <span>
                  @{w.handle}
                  <span className={`ml-2 text-xs ${muted}`}>
                    {w.followers !== null ? `${w.followers.toLocaleString()} followers · ` : "follower count unknown · "}
                    {w.last_scanned_at ? `scanned ${new Date(w.last_scanned_at).toLocaleDateString()}` : "never scanned"}
                  </span>
                </span>
                <span className="flex gap-1.5">
                  <button
                    type="button"
                    className={btn}
                    disabled={busy !== null || locked || !xRunning}
                    title={xRunning ? undefined : "Open the X window first (Settings)"}
                    onClick={() =>
                      run(`scan-${w.id}`, async () => {
                        const s = await api.watchScan(w.id);
                        setNotice(`Read ${s.posts_read} posts from @${s.handle}: ${s.flagged} over-performing, ${s.new_candidates} new.`);
                      })
                    }
                  >
                    {busy === `scan-${w.id}` ? "Scanning..." : "Scan"}
                  </button>
                  <button type="button" className={btn} disabled={busy !== null} onClick={() => run("rm", () => api.watchRemove(w.id))}>Remove</button>
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {thresholds && (
        <section className={panel} aria-labelledby="th-h">
          <h2 id="th-h" className="font-medium">What counts as viral</h2>
          <p className={`mt-1 text-sm ${muted}`}>A post is flagged when it clears the minimum engagement and either beats the creator's follower count by the reach multiple or is saved unusually often. The defaults are a starting point; tune them against what you see.</p>
          <div className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-3">
            {numField("Minimum engagement", thresholds.min_engagement, (n) => setThresholds({ ...thresholds, min_engagement: n }))}
            {numField("Reach multiple", thresholds.reach_multiple, (n) => setThresholds({ ...thresholds, reach_multiple: n }), 0.05)}
            {numField("Bookmark ratio", thresholds.bookmark_ratio, (n) => setThresholds({ ...thresholds, bookmark_ratio: n }), 0.01)}
            {numField("Minimum bookmarks", thresholds.min_bookmarks, (n) => setThresholds({ ...thresholds, min_bookmarks: n }))}
            {numField("Assumed followers", thresholds.assumed_followers, (n) => setThresholds({ ...thresholds, assumed_followers: n }))}
          </div>
          <button type="button" className={`${btn} mt-3`} disabled={busy !== null} onClick={() => run("th", async () => { setThresholds(await api.setThresholds(thresholds)); setNotice("Saved."); })}>
            Save thresholds
          </button>
        </section>
      )}

      {settings && (
        <section className={panel} aria-labelledby="cs-h">
          <h2 id="cs-h" className="font-medium">Writing</h2>
          <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
            <label className="block text-sm">
              <span className="mb-1 block text-zinc-500">Originality warning at {Math.round(settings.similarity_threshold * 100)}% overlap</span>
              <input
                type="range"
                aria-label="Originality warning threshold"
                className="w-full"
                min={20}
                max={95}
                step={5}
                value={Math.round(settings.similarity_threshold * 100)}
                onChange={(e) => setSettings({ ...settings, similarity_threshold: Number(e.target.value) / 100 })}
              />
              <span className={`text-xs ${muted}`}>Lower is stricter. Drafts at or above this are flagged and need a rewrite before you post.</span>
            </label>
            {numField("Character limit", settings.max_chars, (n) => setSettings({ ...settings, max_chars: n }))}
          </div>
          <button type="button" className={`${btn} mt-3`} disabled={busy !== null} onClick={() => run("cs", async () => { setSettings(await api.saveSettings(settings)); setNotice("Saved."); })}>
            Save writing settings
          </button>
          <p className={`mt-2 text-xs ${muted}`}>Standard X accounts allow 280 characters. Raise the limit only if your account has longer posts.</p>
        </section>
      )}
    </div>
  );
}
