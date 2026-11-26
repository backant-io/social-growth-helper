import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { joinList, parseList, stripHandle } from "../lib/lists";

interface Props {
  onSaved: () => void;
  onBack: () => void;
}

const input =
  "w-full rounded-lg border border-zinc-300 bg-white px-3 py-2 text-sm dark:border-zinc-700 dark:bg-zinc-900";

export function PreferencesStep({ onSaved, onBack }: Props) {
  const [topics, setTopics] = useState("");
  const [prefer, setPrefer] = useState("");
  const [avoid, setAvoid] = useState("");
  const [languages, setLanguages] = useState("en");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.getPreferences().then((p) => {
      setTopics(joinList(p.niche_topics));
      setPrefer(joinList(p.prefer_accounts));
      setAvoid(joinList(p.avoid_accounts));
      setLanguages(joinList(p.languages.length ? p.languages : ["en"]));
    });
  }, []);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.savePreferences({
        niche_topics: parseList(topics),
        prefer_accounts: parseList(prefer).map(stripHandle),
        avoid_accounts: parseList(avoid).map(stripHandle),
        languages: parseList(languages),
      });
      onSaved();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex-1 space-y-4 overflow-y-auto">
        <p className="text-sm text-zinc-600 dark:text-zinc-400">
          These seed Scout&apos;s ranking. You can change them any time in Settings. Nothing here contacts X.
        </p>
        <label className="block text-sm font-medium">
          Niche topics
          <input aria-label="Niche topics" className={`${input} mt-1`} value={topics} onChange={(e) => setTopics(e.target.value)} placeholder="local-first software, rust, indie hacking" />
        </label>
        <label className="block text-sm font-medium">
          Accounts to prefer
          <input aria-label="Accounts to prefer" className={`${input} mt-1`} value={prefer} onChange={(e) => setPrefer(e.target.value)} placeholder="@handle, @another" />
        </label>
        <label className="block text-sm font-medium">
          Accounts to avoid
          <input aria-label="Accounts to avoid" className={`${input} mt-1`} value={avoid} onChange={(e) => setAvoid(e.target.value)} placeholder="@handle" />
        </label>
        <label className="block text-sm font-medium">
          Languages
          <input aria-label="Languages" className={`${input} mt-1`} value={languages} onChange={(e) => setLanguages(e.target.value)} placeholder="en, de" />
        </label>
        {error && <p role="alert" className="text-sm text-red-600 dark:text-red-400">{error}</p>}
      </div>
      <div className="mt-4 flex justify-between border-t border-zinc-200 pt-4 dark:border-zinc-800">
        <button type="button" onClick={onBack} className="rounded-lg px-4 py-2 text-sm text-zinc-600 dark:text-zinc-400">
          Back
        </button>
        <button type="button" onClick={save} disabled={busy} className="rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900">
          Save and continue
        </button>
      </div>
    </div>
  );
}
