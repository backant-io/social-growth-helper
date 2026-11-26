import { inTauri } from "../lib/api";
import { learningMock } from "./mock";
import type {
  ChangelogEntry,
  ExampleInfo,
  ExportSummary,
  ImportSummary,
  Insight,
  LearningOverview,
  Manifest,
  Patch,
  ReflectionMode,
  ReflectionSummary,
  VersionInfo,
  WipeReport,
} from "./types";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
  }
}

const MOCK_EXPORT_PATH = "~/Downloads/social-growth-helper-export-preview.zip";

/** Outside the desktop app (vite dev, screenshots) a localStorage-backed preview stands in for the backend. */
export const learningApi = {
  async overview(): Promise<LearningOverview> {
    return inTauri() ? call("learning_overview") : learningMock.overview();
  },
  async proposedCount(): Promise<number> {
    return inTauri() ? call("learning_proposed_count") : learningMock.overview().proposed.length;
  },
  async reflect(mode: ReflectionMode): Promise<ReflectionSummary> {
    return inTauri() ? call("learning_reflect", { mode }) : learningMock.reflect(mode);
  },
  async reflectIfDue(): Promise<ReflectionSummary | null> {
    return inTauri() ? call("learning_reflect_if_due") : null;
  },
  async accept(insightId: string, editedPatch: Patch | null): Promise<Insight> {
    return inTauri() ? call("learning_accept", { insightId, editedPatch }) : learningMock.accept(insightId, editedPatch);
  },
  async reject(insightId: string): Promise<void> {
    return inTauri() ? call("learning_reject", { insightId }) : learningMock.reject(insightId);
  },
  async rubricVersions(): Promise<VersionInfo[]> {
    return inTauri() ? call("learning_versions", { kind: "rubric" }) : learningMock.versions();
  },
  async rollbackRubric(version: number): Promise<void> {
    return inTauri() ? call("learning_rollback", { kind: "rubric", version }) : learningMock.rollback(version);
  },
  async changelog(): Promise<ChangelogEntry[]> {
    return inTauri() ? call("learning_changelog") : learningMock.changelog();
  },
  async examples(): Promise<ExampleInfo[]> {
    return inTauri() ? call("learning_examples") : learningMock.examples();
  },
  async removeExample(id: string): Promise<void> {
    return inTauri() ? call("learning_remove_example", { id }) : learningMock.removeExample(id);
  },
  async setMinSample(value: number): Promise<number> {
    return inTauri() ? call("learning_set_min_sample", { value }) : learningMock.setMinSample(value);
  },
  async defaultExportPath(): Promise<string> {
    return inTauri() ? call("data_default_export_path") : MOCK_EXPORT_PATH;
  },
  async exportData(path: string | null): Promise<ExportSummary> {
    if (inTauri()) return call("data_export", { path });
    return { path: path || MOCK_EXPORT_PATH, bytes: 184_320, tables: { reply_drafts: 18, observed_posts: 64, style_guides: 3 }, excluded: [] };
  },
  async inspectExport(path: string): Promise<Manifest> {
    if (inTauri()) return call("data_inspect_export", { path });
    if (!path.trim().endsWith(".zip")) throw new Error("this file is not a zip archive");
    return { format: "social-growth-helper-export", format_version: 1, app_version: "0.1.0", schema_version: 6, created_at: "2026-11-20T10:00:00Z", tables: { reply_drafts: 18, observed_posts: 64 }, excluded: [] };
  },
  async importData(path: string): Promise<ImportSummary> {
    if (inTauri()) return call("data_import", { path });
    return { tables: { reply_drafts: 18, observed_posts: 64 }, rows: 82, accounts_needing_keys: 1, exported_at: "2026-11-20T10:00:00Z", source_app_version: "0.1.0", safety_backup: null, warnings: ["1 provider account(s) need their API key entered again on this machine"] };
  },
  async wipe(confirmation: string, ignoreKeychainErrors: boolean): Promise<WipeReport> {
    if (inTauri()) return call("data_wipe", { confirmation, ignoreKeychainErrors });
    if (confirmation.trim() !== "DELETE") throw new Error("type DELETE to confirm");
    learningMock.reset();
    localStorage.removeItem("sgh-mock-state");
    return { rows_deleted: 120, keychain_removed: 1, files_removed: 3, warnings: [] };
  },
};
