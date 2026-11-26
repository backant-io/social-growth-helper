import { useState } from "react";
import { analyticsApi } from "./api";
import { WidgetChartRenderer } from "./charts/ChartPrimitives";
import type { BuilderRouteStatus, DashboardWidget, WidgetQueryResult, WidgetSpec } from "./types";

interface RepromptBuilderModalProps {
  existingWidget: DashboardWidget | null;
  windowDays: number;
  builderRoute: BuilderRouteStatus;
  onAccept: (spec: WidgetSpec, sourcePrompt: string) => Promise<void>;
  onClose: () => void;
}

const PROMPT_SUGGESTIONS = [
  "Weekly reply impressions stacked by model",
  "Scatter of edit distance vs impressions by model",
  "Reply timing × author tier heatmap",
  "Funnel of impressions by action bundle",
  "Top OGC viral formats table",
];

export function RepromptBuilderModal({
  existingWidget,
  windowDays,
  builderRoute,
  onAccept,
  onClose,
}: RepromptBuilderModalProps) {
  const [prompt, setPrompt] = useState(existingWidget?.source_prompt ?? "");
  const [draftSpec, setDraftSpec] = useState<WidgetSpec | null>(existingWidget?.spec ?? null);
  const [previewData, setPreviewData] = useState<WidgetQueryResult | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
  const [rawMode, setRawMode] = useState(false);
  const [rawJson, setRawJson] = useState(
    existingWidget ? JSON.stringify(existingWidget.spec, null, 2) : "",
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function runReprompt(inputPrompt?: string) {
    const text = (inputPrompt ?? prompt).trim();
    if (!text) return;
    setBusy(true);
    setError(null);
    try {
      const res = await analyticsApi.repromptWidget(text, draftSpec, windowDays);
      setDraftSpec(res.spec);
      setPreviewData(res.preview);
      setRawJson(JSON.stringify(res.spec, null, 2));
      setSummary(res.summary);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  async function runRawPreview() {
    setBusy(true);
    setError(null);
    try {
      const [spec, data] = await analyticsApi.previewSpecJson(rawJson, windowDays);
      setDraftSpec(spec);
      setPreviewData(data);
      setSummary(`Sandboxed spec validated (${spec.chart_type} on ${spec.view}).`);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  async function handleAccept() {
    if (!draftSpec) return;
    setBusy(true);
    setError(null);
    try {
      await onAccept(draftSpec, prompt.trim() || draftSpec.title);
      onClose();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div className="flex max-h-[90vh] w-full max-w-3xl flex-col overflow-hidden rounded-xl border border-zinc-200 dark:border-zinc-800 bg-[var(--bg)] shadow-2xl">
        <header className="flex items-center justify-between border-b border-zinc-200 dark:border-zinc-800 px-5 py-3.5">
          <div>
            <h2 className="text-base font-semibold text-zinc-900 dark:text-zinc-100">
              {existingWidget ? `Reprompt: ${existingWidget.title}` : "Reprompt a New Widget"}
            </h2>
            <p className="text-xs text-zinc-500">
              Describe what you want to see or tweak the current chart. All queries run inside the read-only analytics sandbox.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-2 py-1 text-xs text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
          >
            Close
          </button>
        </header>

        <div className="flex-1 overflow-y-auto p-5 space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-900 px-3 py-2 text-xs">
            <div className="flex items-center gap-2">
              <span
                className={`h-2 w-2 rounded-full ${
                  builderRoute.configured ? "bg-emerald-500" : "bg-amber-500"
                }`}
              />
              <span className="font-medium text-zinc-700 dark:text-zinc-200">
                dashboard_builder:{" "}
                {builderRoute.configured
                  ? `${builderRoute.provider_label ?? builderRoute.provider_type} · ${builderRoute.model_id}`
                  : "Unset (using built-in deterministic planner)"}
              </span>
            </div>
            <button
              type="button"
              onClick={() => setRawMode((v) => !v)}
              className="text-sky-600 dark:text-sky-400 hover:underline"
            >
              {rawMode ? "Hide Spec JSON" : "Edit Spec JSON"}
            </button>
          </div>

          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void runReprompt();
            }}
          >
            <input
              type="text"
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder={
                existingWidget
                  ? 'Follow-up edit (e.g. "switch to stacked bar", "filter to claude only", "last 7d")'
                  : 'Ask for a chart (e.g. "Weekly reply impressions stacked by model")'
              }
              className="flex-1 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-transparent px-3 py-2 text-sm text-zinc-900 dark:text-zinc-100 focus:border-sky-500 focus:outline-none"
            />
            <button
              type="submit"
              disabled={busy || !prompt.trim()}
              className="rounded-lg bg-zinc-900 dark:bg-zinc-100 px-4 py-2 text-sm font-medium text-white dark:text-zinc-900 hover:opacity-90 disabled:opacity-40"
            >
              {busy ? "Building…" : "Preview"}
            </button>
          </form>

          {!existingWidget && (
            <div className="flex flex-wrap gap-1.5">
              {PROMPT_SUGGESTIONS.map((s) => (
                <button
                  key={s}
                  type="button"
                  onClick={() => {
                    setPrompt(s);
                    void runReprompt(s);
                  }}
                  className="rounded-full border border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-900 px-2.5 py-1 text-[11px] text-zinc-600 dark:text-zinc-300 hover:border-sky-400"
                >
                  {s}
                </button>
              ))}
            </div>
          )}

          {rawMode && (
            <div className="space-y-2">
              <textarea
                rows={9}
                value={rawJson}
                onChange={(e) => setRawJson(e.target.value)}
                placeholder='{"title":"...","view":"v_analytics_by_model","chart_type":"bar",...}'
                className="w-full rounded-lg border border-zinc-300 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900 p-3 font-mono text-xs text-zinc-800 dark:text-zinc-200 focus:border-sky-500 focus:outline-none"
              />
              <div className="flex justify-end">
                <button
                  type="button"
                  disabled={busy || !rawJson.trim()}
                  onClick={() => void runRawPreview()}
                  className="rounded-md border border-zinc-300 dark:border-zinc-700 px-3 py-1.5 text-xs font-medium text-zinc-700 dark:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800 disabled:opacity-40"
                >
                  Validate & Preview JSON
                </button>
              </div>
            </div>
          )}

          {error && (
            <div className="rounded-lg border border-rose-200 dark:border-rose-900 bg-rose-50 dark:bg-rose-950/40 px-3.5 py-2.5 text-xs text-rose-700 dark:text-rose-300">
              {error}
            </div>
          )}

          {draftSpec && previewData && (
            <div className="rounded-xl border border-zinc-200 dark:border-zinc-800 bg-transparent p-4 space-y-2">
              <div className="flex items-center justify-between gap-2">
                <div>
                  <div className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">{draftSpec.title}</div>
                  {summary && <div className="text-[11px] text-zinc-500">{summary}</div>}
                </div>
                <span className="rounded bg-zinc-100 dark:bg-zinc-800 px-2 py-0.5 text-[11px] text-zinc-600 dark:text-zinc-300">
                  {draftSpec.chart_type} · {previewData.row_count} rows
                </span>
              </div>
              <WidgetChartRenderer spec={draftSpec} data={previewData} />
            </div>
          )}
        </div>

        <footer className="flex items-center justify-between border-t border-zinc-200 dark:border-zinc-800 px-5 py-3">
          <span className="text-[11px] text-zinc-500">
            Read-only views only · max 500 rows · no network or write access
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg border border-zinc-300 dark:border-zinc-700 px-3.5 py-1.5 text-xs font-medium text-zinc-700 dark:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800"
            >
              Discard
            </button>
            <button
              type="button"
              disabled={busy || !draftSpec}
              onClick={() => void handleAccept()}
              className="rounded-lg bg-sky-600 px-4 py-1.5 text-xs font-medium text-white hover:bg-sky-700 disabled:opacity-40"
            >
              {existingWidget ? "Update Widget" : "Add to Dashboard"}
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}
