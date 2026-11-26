import { useCallback, useEffect, useRef, useState } from "react";
import { inTauri } from "../../lib/api";
import { creatorApi } from "../../lib/creatorApi";
import type { CreatorSettings, FormatRecord, OgcDraft } from "../../lib/types";
import { DraftCard } from "./DraftCard";
import { btn, danger, field, muted, panel, primary } from "./ui";

interface Props {
  locked: boolean;
  xRunning: boolean;
}

export function WritePanel({ locked, xRunning }: Props) {
  const api = creatorApi();
  const [formats, setFormats] = useState<FormatRecord[]>([]);
  const [drafts, setDrafts] = useState<OgcDraft[]>([]);
  const [settings, setSettings] = useState<CreatorSettings>({ similarity_threshold: 0.5, max_chars: 280 });
  const [topic, setTopic] = useState("");
  const [formatId, setFormatId] = useState("");
  const [picked, setPicked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const draftsRef = useRef(drafts);
  draftsRef.current = drafts;

  const load = useCallback(async () => {
    const [f, d, s] = await Promise.all([api.formats("working", false), api.drafts(), api.settings()]);
    setFormats(f);
    setDrafts(d);
    setSettings(s);
    setFormatId((cur) => cur || f[0]?.id || "");
  }, [api]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (picked || !topic.trim() || formats.length === 0) return;
    const t = setTimeout(() => {
      api.matchFormats(topic).then((m) => m[0] && setFormatId(m[0].format_id)).catch(() => undefined);
    }, 400);
    return () => clearTimeout(t);
  }, [topic, picked, formats.length, api]);

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
          const updated = await api.setMedia(target.id, [...target.media_paths, ...event.payload.paths]);
          setDrafts((cur) => cur.map((d) => (d.id === updated.id ? updated : d)));
        } catch (e) {
          setError(e instanceof Error ? e.message : String(e));
        }
      });
      if (gone) un();
      else off = un;
    });
    return () => {
      gone = true;
      off?.();
    };
  }, [api]);

  async function generate() {
    setBusy(true);
    setError(null);
    try {
      const g = await api.generate(formatId, topic);
      setDrafts((cur) => [...g.drafts, ...cur]);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function blank() {
    setError(null);
    try {
      const d = await api.createBlank(formatId || null, topic.trim() || "");
      setDrafts((cur) => [d, ...cur]);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  const selected = formats.find((f) => f.id === formatId);
  const byId = (id: string | null) => formats.find((f) => f.id === id);

  return (
    <div className="space-y-4">
      <section className={panel} aria-labelledby="w-h">
        <h2 id="w-h" className="font-medium">New original post</h2>
        <p className={`mt-1 text-sm ${muted}`}>Write a rough thought. The app fits it into a format that has worked for others and drafts three versions in your voice. Nothing is posted until you press Publish.</p>
        <label className="mt-3 block text-sm">
          <span className="mb-1 block font-medium">Your rough thought</span>
          <textarea className={`${field} min-h-20`} value={topic} onChange={(e) => setTopic(e.target.value)} placeholder="e.g. shipping a small thing every week beats planning a big launch" />
        </label>
        <label className="mt-3 block text-sm">
          <span className="mb-1 block font-medium">Format</span>
          <select className={field} value={formatId} onChange={(e) => { setFormatId(e.target.value); setPicked(true); }} disabled={formats.length === 0}>
            {formats.length === 0 && <option value="">No formats yet. Add one in the Formats tab.</option>}
            {formats.map((f) => (
              <option key={f.id} value={f.id}>{f.name}</option>
            ))}
          </select>
        </label>
        {selected && (
          <p className={`mt-1 text-xs ${muted}`}>
            {selected.hook_formula}
            {selected.media_hint ? ` Works well with: ${selected.media_hint}.` : ""}
          </p>
        )}
        <div className="mt-3 flex flex-wrap gap-2">
          <button type="button" className={primary} disabled={busy || locked || !formatId || !topic.trim()} onClick={generate}>
            {busy ? "Writing..." : "Write 3 versions"}
          </button>
          <button type="button" className={btn} disabled={busy} onClick={blank}>Start from a blank draft</button>
        </div>
        {!xRunning && <p className={`mt-2 text-xs ${muted}`}>The X window is closed. You can write and edit now; open it in Settings before publishing.</p>}
        {error && <p role="alert" className={`mt-2 text-sm ${danger}`}>{error}</p>}
      </section>

      {drafts.length === 0 ? (
        <p className={`text-sm ${muted}`}>Your drafts will appear here.</p>
      ) : (
        <div className="space-y-3" aria-label="Drafts">
          {drafts.map((d) => (
            <DraftCard
              key={d.id}
              draft={d}
              format={byId(d.viral_format_id)}
              threshold={settings.similarity_threshold}
              maxChars={settings.max_chars}
              canPublish={xRunning && !locked}
              onChanged={(u) => setDrafts((cur) => cur.map((x) => (x.id === u.id ? u : x)))}
              onRemoved={(id) => setDrafts((cur) => cur.filter((x) => x.id !== id))}
            />
          ))}
        </div>
      )}
    </div>
  );
}
