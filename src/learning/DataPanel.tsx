import { useEffect, useState } from "react";
import { learningApi } from "./api";
import { canConfirmWipe, formatBytes, WIPE_CONFIRMATION } from "./model";
import type { ExportSummary, ImportSummary, Manifest } from "./types";
import { btn, btnDanger, input, panel } from "./ui";

interface Props {
  /** Called after an import or wipe so the app re-reads onboarding status and per-section data. */
  onDataChanged: () => void;
}

export function DataPanel({ onDataChanged }: Props) {
  const [exportPath, setExportPath] = useState("");
  const [exported, setExported] = useState<ExportSummary | null>(null);
  const [importPath, setImportPath] = useState("");
  const [manifest, setManifest] = useState<Manifest | null>(null);
  const [imported, setImported] = useState<ImportSummary | null>(null);
  const [wipeOpen, setWipeOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const [ignoreKeychain, setIgnoreKeychain] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    learningApi.defaultExportPath().then(setExportPath).catch(() => undefined);
  }, []);

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await fn();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className={panel} aria-labelledby="data-h">
      <h2 id="data-h" className="font-medium">Your data</h2>
      <p className="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
        Everything lives on this computer. Export a copy to move to another machine, or delete it all. Exports never include API keys, cookies or your X login.
      </p>

      <div className="mt-4 space-y-2">
        <h3 className="text-sm font-medium">Export</h3>
        <div className="flex gap-2">
          <input aria-label="Export file path" className={`${input} min-w-0 flex-1`} value={exportPath} onChange={(e) => setExportPath(e.target.value)} />
          <button
            type="button"
            className={btn}
            disabled={busy}
            onClick={() =>
              run(async () => {
                setExported(await learningApi.exportData(exportPath.trim() || null));
              })
            }
          >
            Export
          </button>
        </div>
        {exported && (
          <p role="status" className="text-sm text-emerald-700 dark:text-emerald-400">
            Saved {formatBytes(exported.bytes)} to <code className="break-all text-xs">{exported.path}</code>
          </p>
        )}
      </div>

      <div className="mt-5 space-y-2">
        <h3 className="text-sm font-medium">Import</h3>
        <p className="text-xs text-zinc-500">Replaces the data on this computer with an export. A backup is taken first. You will enter your provider API keys again.</p>
        <div className="flex gap-2">
          <input
            aria-label="Import file path"
            className={`${input} min-w-0 flex-1`}
            placeholder="Path to an export .zip"
            value={importPath}
            onChange={(e) => {
              setImportPath(e.target.value);
              setManifest(null);
              setImported(null);
            }}
          />
          <button
            type="button"
            className={btn}
            disabled={busy || !importPath.trim()}
            onClick={() =>
              run(async () => {
                setManifest(await learningApi.inspectExport(importPath.trim()));
              })
            }
          >
            Check file
          </button>
        </div>
        {manifest && (
          <div className="rounded-lg border border-zinc-200 p-3 text-sm dark:border-zinc-800">
            <p>
              Export from {manifest.created_at.slice(0, 10)}, app {manifest.app_version}. Contains{" "}
              {Object.values(manifest.tables).reduce((a, b) => a + b, 0)} records.
            </p>
            <button
              type="button"
              className={`${btn} mt-2`}
              disabled={busy}
              onClick={() =>
                run(async () => {
                  if (!window.confirm("Replace all data on this computer with this export?")) return;
                  setImported(await learningApi.importData(importPath.trim()));
                  setManifest(null);
                  onDataChanged();
                })
              }
            >
              Replace my data with this export
            </button>
          </div>
        )}
        {imported && (
          <div role="status" className="text-sm text-emerald-700 dark:text-emerald-400">
            <p>Imported {imported.rows} records.</p>
            {imported.warnings.map((w) => (
              <p key={w} className="text-amber-700 dark:text-amber-400">{w}</p>
            ))}
          </div>
        )}
      </div>

      <div className="mt-5 space-y-2 border-t border-zinc-200 pt-4 dark:border-zinc-800">
        <h3 className="text-sm font-medium text-red-700 dark:text-red-400">Delete all data</h3>
        <p className="text-xs text-zinc-500">
          Removes your drafts, history, learned style, saved API keys and the X browser profile from this computer. Your posts on X are not touched. This cannot be undone.
        </p>
        {!wipeOpen ? (
          <button type="button" className={btn} onClick={() => setWipeOpen(true)}>Delete everything</button>
        ) : (
          <div className="space-y-2 rounded-lg border border-red-300 p-3 dark:border-red-900">
            <label className="block text-sm">
              Type <strong>{WIPE_CONFIRMATION}</strong> to confirm
              <input aria-label="Type DELETE to confirm" className={`${input} mt-1 w-full`} value={typed} onChange={(e) => setTyped(e.target.value)} autoComplete="off" />
            </label>
            <label className="flex items-center gap-2 text-xs text-zinc-600 dark:text-zinc-400">
              <input type="checkbox" checked={ignoreKeychain} onChange={(e) => setIgnoreKeychain(e.target.checked)} />
              Continue even if saved API keys cannot be removed from the system keychain
            </label>
            <div className="flex gap-2">
              <button
                type="button"
                className={btnDanger}
                disabled={busy || !canConfirmWipe(typed)}
                onClick={() =>
                  run(async () => {
                    const r = await learningApi.wipe(typed, ignoreKeychain);
                    setWipeOpen(false);
                    setTyped("");
                    setNotice(`Deleted ${r.rows_deleted} records and ${r.files_removed} files.${r.warnings.length ? ` ${r.warnings.join(" ")}` : ""}`);
                    onDataChanged();
                  })
                }
              >
                Delete all data
              </button>
              <button type="button" className={btn} onClick={() => { setWipeOpen(false); setTyped(""); }}>Cancel</button>
            </div>
          </div>
        )}
      </div>

      {error && <p role="alert" className="mt-3 text-sm text-red-600 dark:text-red-400">{error}</p>}
      {notice && <p role="status" className="mt-3 text-sm text-emerald-700 dark:text-emerald-400">{notice}</p>}
    </section>
  );
}
