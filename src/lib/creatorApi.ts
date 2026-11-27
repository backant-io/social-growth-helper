import { inTauri } from "./api";
import { createMockCreator } from "./creatorMock";
import type {
  BatchSummary,
  CreatorSettings,
  Detection,
  FormatBlueprint,
  FormatCandidate,
  FormatMatch,
  FormatRecord,
  FormatSort,
  Generated,
  ManualPost,
  OgcDraft,
  Processed,
  ScanSummary,
  ViralThresholds,
  WatchEntry,
} from "./types";

export interface CreatorApi {
  formats(sort: FormatSort, includeArchived: boolean): Promise<FormatRecord[]>;
  createFormat(blueprint: FormatBlueprint): Promise<FormatRecord>;
  updateFormat(id: string, blueprint: FormatBlueprint): Promise<FormatRecord>;
  setFormatStatus(id: string, status: "active" | "archived"): Promise<void>;
  candidates(status?: string): Promise<FormatCandidate[]>;
  dismissCandidate(id: string): Promise<void>;
  extract(candidateId: string): Promise<Processed>;
  extractPending(): Promise<BatchSummary>;
  addPost(post: ManualPost): Promise<Detection>;
  thresholds(): Promise<ViralThresholds>;
  setThresholds(t: ViralThresholds): Promise<ViralThresholds>;
  watchlist(): Promise<WatchEntry[]>;
  watchAdd(handle: string, followers: number | null): Promise<WatchEntry>;
  watchRemove(id: string): Promise<void>;
  watchScan(id: string): Promise<ScanSummary>;
  settings(): Promise<CreatorSettings>;
  saveSettings(s: CreatorSettings): Promise<CreatorSettings>;
  matchFormats(topic: string): Promise<FormatMatch[]>;
  generate(formatId: string, topic: string): Promise<Generated>;
  variation(candidateId: string): Promise<Generated>;
  createBlank(formatId: string | null, text: string): Promise<OgcDraft>;
  drafts(): Promise<OgcDraft[]>;
  edit(id: string, text: string): Promise<OgcDraft>;
  setMedia(id: string, paths: string[]): Promise<OgcDraft>;
  discard(id: string): Promise<void>;
  reopen(id: string): Promise<OgcDraft>;
  publish(id: string, expectedText: string): Promise<OgcDraft>;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
  }
}

const tauriCreator: CreatorApi = {
  formats: (sort, includeArchived) => call("formats_list", { sort, includeArchived }),
  createFormat: (blueprint) => call("formats_create", { blueprint }),
  updateFormat: (id, blueprint) => call("formats_update", { id, blueprint }),
  setFormatStatus: (id, status) => call("formats_set_status", { id, status }),
  candidates: (status = "pending") => call("formats_candidates", { status }),
  dismissCandidate: (id) => call("formats_dismiss_candidate", { id }),
  extract: (candidateId) => call("formats_extract", { candidateId }),
  extractPending: () => call("formats_extract_pending"),
  addPost: (post) => call("formats_add_post", { post }),
  thresholds: () => call("formats_get_thresholds"),
  setThresholds: (thresholds) => call("formats_set_thresholds", { thresholds }),
  watchlist: () => call("formats_watchlist"),
  watchAdd: (handle, followers) => call("formats_watch_add", { handle, followers }),
  watchRemove: (id) => call("formats_watch_remove", { id }),
  watchScan: (id) => call("formats_watch_scan", { id }),
  settings: () => call("creator_get_settings"),
  saveSettings: (settings) => call("creator_save_settings", { settings }),
  matchFormats: (topic) => call("creator_match_formats", { topic }),
  generate: (formatId, topic) => call("creator_generate", { formatId, topic }),
  variation: (candidateId) => call("creator_variation", { candidateId }),
  createBlank: (formatId, text) => call("creator_create_blank", { formatId, text }),
  drafts: () => call("creator_drafts"),
  edit: (id, text) => call("creator_edit", { id, text }),
  setMedia: (id, paths) => call("creator_set_media", { id, paths }),
  discard: (id) => call("creator_discard", { id }),
  reopen: (id) => call("creator_reopen", { id }),
  publish: (id, expectedText) => call("creator_publish", { id, expectedText }),
};

let mockInstance: CreatorApi | null = null;

/** In a plain browser the backend is an in-memory stand-in, so the page stays explorable and testable. */
export function creatorApi(): CreatorApi {
  if (inTauri()) return tauriCreator;
  mockInstance ??= createMockCreator();
  return mockInstance;
}
