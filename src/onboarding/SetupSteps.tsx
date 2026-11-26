import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { useOnlineStatus } from "../lib/errors";
import type { BrowserInfo, ProviderAccountView, StyleGuide, TaskModelRoute } from "../lib/types";
import { BrowserPanel, ProvidersPanel, RoutingPanel } from "../pages/SettingsPage";
import { browserHint, providerHint, routingHint, SETUP_STEP_COPY, type SetupHint } from "./tourSteps";

type SetupKind = keyof typeof SETUP_STEP_COPY;

interface Props {
  kind: SetupKind;
  rulesAcknowledged: boolean;
  onBack: () => void;
  onNext: () => void;
  finish?: boolean;
}

function Frame({ kind, hint, onBack, onNext, finish, children }: { kind: SetupKind; hint: SetupHint | null; onBack: () => void; onNext: () => void; finish?: boolean; children: ReactNode }) {
  const copy = SETUP_STEP_COPY[kind];
  return (
    <div className="flex h-full flex-col">
      <h2 className="mb-2 text-xl font-semibold">{copy.title}</h2>
      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto">
        <p className="text-sm text-zinc-600 dark:text-zinc-400">{copy.intro}</p>
        {hint && (
          <p role="status" data-testid="setup-hint" className={`text-sm ${hint.done ? "text-emerald-600 dark:text-emerald-400" : "text-amber-600 dark:text-amber-400"}`}>
            {hint.text}
          </p>
        )}
        {children}
      </div>
      <div className="mt-4 flex justify-between border-t border-zinc-200 pt-4 dark:border-zinc-800">
        <button type="button" onClick={onBack} className="rounded-lg px-4 py-2 text-sm text-zinc-600 dark:text-zinc-400">
          Back
        </button>
        <button type="button" onClick={onNext} className="rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white dark:bg-zinc-100 dark:text-zinc-900">
          {finish ? "Finish setup" : hint?.done ? "Next" : "Skip for now"}
        </button>
      </div>
    </div>
  );
}

function StyleGuideForm({ onSaved }: { onSaved: (g: StyleGuide) => void }) {
  const [guide, setGuide] = useState<StyleGuide | null>(null);
  const [prose, setProse] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.styleGetActiveGuide().then((g) => {
      setGuide(g);
      setProse(g.free_text_guide);
    });
  }, []);

  const save = async () => {
    if (!guide) return;
    setBusy(true);
    setError(null);
    try {
      const saved = await api.styleSaveGuideVersion({
        free_text_guide: prose,
        learned_rules: guide.learned_rules,
        banned_patterns: guide.banned_patterns,
        max_reply_chars: guide.max_reply_chars,
        emoji_policy: guide.emoji_policy,
        hashtag_policy: guide.hashtag_policy,
        note: "Saved during first-run setup",
      });
      setGuide(saved);
      onSaved(saved);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-2">
      <label className="block text-sm font-medium">
        Style guide
        <textarea
          aria-label="Style guide"
          rows={6}
          className="mt-1 w-full rounded-lg border border-zinc-300 bg-white px-3 py-2 text-sm dark:border-zinc-700 dark:bg-zinc-900"
          value={prose}
          onChange={(e) => setProse(e.target.value)}
        />
      </label>
      {error && <p role="alert" className="text-sm text-red-600 dark:text-red-400">{error}</p>}
      <button
        type="button"
        onClick={save}
        disabled={busy || !guide || !prose.trim()}
        className="rounded-lg border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800"
      >
        Save style guide
      </button>
    </div>
  );
}

export function SetupStep({ kind, rulesAcknowledged, onBack, onNext, finish }: Props) {
  const offline = !useOnlineStatus();
  const [providers, setProviders] = useState<ProviderAccountView[]>([]);
  const [routes, setRoutes] = useState<TaskModelRoute[]>([]);
  const [info, setInfo] = useState<BrowserInfo | null>(null);
  const [styleSaved, setStyleSaved] = useState<StyleGuide | null>(null);

  const loadLlm = useCallback(async () => {
    const [p, r] = await Promise.all([api.llmListProviders(), api.llmListTaskRoutes()]);
    setProviders(p);
    setRoutes(r);
  }, []);

  useEffect(() => {
    if (kind === "providers" || kind === "routing") void loadLlm();
    if (kind !== "browser") return;
    const poll = () => void api.browserInfo().then(setInfo);
    poll();
    const timer = setInterval(poll, 2000);
    return () => clearInterval(timer);
  }, [kind, loadLlm]);

  if (kind === "browser") {
    return (
      <Frame kind={kind} hint={browserHint(info, rulesAcknowledged)} onBack={onBack} onNext={onNext} finish={finish}>
        <BrowserPanel rulesAcknowledged={rulesAcknowledged} />
      </Frame>
    );
  }
  if (kind === "providers") {
    return (
      <Frame kind={kind} hint={providerHint(providers)} onBack={onBack} onNext={onNext} finish={finish}>
        <ProvidersPanel providers={providers} offline={offline} onChanged={loadLlm} />
      </Frame>
    );
  }
  if (kind === "style") {
    const hint = styleSaved ? { done: true, text: `Saved style guide v${styleSaved.version}.` } : null;
    return (
      <Frame kind={kind} hint={hint} onBack={onBack} onNext={onNext} finish={finish}>
        <StyleGuideForm onSaved={setStyleSaved} />
      </Frame>
    );
  }
  return (
    <Frame kind={kind} hint={routingHint(routes)} onBack={onBack} onNext={onNext} finish={finish}>
      <RoutingPanel providers={providers} routes={routes} onChanged={loadLlm} />
    </Frame>
  );
}
