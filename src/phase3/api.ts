import { inTauri, listen } from "../lib/api";
import { normalizeBundle } from "./logic";
import type {
  ActionName,
  ActivityEntry,
  ChecklistReport,
  CompletionStatus,
  DraftView,
  JobResult,
  NotificationSettings,
  PacingConfig,
  PacingUsage,
  PostActionState,
  ModelOption,
  QueueItem,
  ScoutStatus,
  SessionParams,
} from "./types";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
  }
}

/** Browser preview: an in-memory backend so the screens can be explored and screenshotted without Tauri. */
const now = () => new Date().toISOString();
const mem = {
  kill: false,
  threshold: 72,
  running: false,
  watchlist: ["naval", "paulg"],
  retention: 30,
  pacing: { enabled: false, like: 120, reply: 60, repost: 30, quote: 20, bookmark: 120, follow: 40, min_gap_sec: 90 } as PacingConfig,
  notifications: { enabled: true, scout_done: true, publish_done: true, publish_failed: true, attention: true } as NotificationSettings,
  queue: [
    {
      evaluation_id: "e1",
      session_id: "s1",
      post: { observed_id: "o1", platform_post_id: "111", handle: "naval", text: "The best way to build wealth is to own equity in something that scales without you.", has_media: false, url: "https://x.com/naval/status/111" },
      score: 91,
      angle: "Add the counterpoint: equity needs a team first",
      reasoning: "Fresh post (12 min) with fast likes and few replies, squarely in your startups topic.",
      model_used: "preview",
      status: "queued",
      metrics: { replies: 14, reposts: 40, likes: 612, bookmarks: 55, views: 22000 },
      posted_at: now(),
      draft_id: null,
      created_at: now(),
    },
    {
      evaluation_id: "e2",
      session_id: "s1",
      post: { observed_id: "o2", platform_post_id: "222", handle: "paulg", text: "Startups are mostly about not dying until the market comes to you.", has_media: false, url: "https://x.com/paulg/status/222" },
      score: 78,
      angle: "Share a concrete survival tactic",
      reasoning: "Strong engagement velocity, low reply count.",
      model_used: "preview",
      status: "queued",
      metrics: { replies: 30, reposts: 90, likes: 1400, bookmarks: 120, views: 61000 },
      posted_at: now(),
      draft_id: null,
      created_at: now(),
    },
  ] as QueueItem[],
  drafts: new Map<string, DraftView>(),
  activity: [
    { id: "a1", kind: "scout", title: "Scout finished", detail: "Scanned 50 posts, queued 4", ok: true, at: now(), screenshot: null },
    { id: "a2", kind: "challenge", title: "X needed attention", detail: "A challenge page was shown; the run stopped", ok: false, at: now(), screenshot: null },
  ] as ActivityEntry[],
  reports: [] as ChecklistReport[],
};

function mockDraft(item: QueueItem): DraftView {
  const id = `d-${item.evaluation_id}`;
  const existing = mem.drafts.get(id);
  if (existing) return existing;
  const v = [
    { id: `${id}-1`, text: "Equity only scales if the team does. Hiring the first three well is the whole game.", tone: "direct", model_used: "preview" },
    { id: `${id}-2`, text: "Own equity, sure. But the part nobody tweets about is who builds the thing while you sleep.", tone: "witty", model_used: "preview" },
  ];
  const d: DraftView = {
    id,
    evaluation_id: item.evaluation_id,
    post: item.post,
    angle: item.angle,
    reasoning: item.reasoning,
    score: item.score,
    kind: "reply",
    initial_text: v[0].text,
    edited_text: v[0].text,
    edit_distance: 0,
    tone_tag: "direct",
    model_used: "preview",
    actions: ["like", "reply"],
    status: "draft",
    error_message: null,
    published_url: null,
    variants: v,
    steps: [],
    created_at: now(),
  };
  mem.drafts.set(id, d);
  return d;
}

function put(d: DraftView): DraftView {
  mem.drafts.set(d.id, d);
  return { ...d };
}

export const p3 = {
  async scoutStatus(): Promise<ScoutStatus> {
    if (inTauri()) return call("scout_status");
    return { running: mem.running, session_id: mem.running ? "s-live" : null, threshold: mem.threshold, last_params: null, recent: [] };
  },
  async scoutStart(params: SessionParams): Promise<string> {
    if (inTauri()) return call("scout_start", { params });
    mem.running = true;
    return "s-live";
  },
  async scoutStop(): Promise<void> {
    if (inTauri()) return call("scout_stop");
    mem.running = false;
  },
  async scoutPause(paused: boolean): Promise<boolean> {
    if (inTauri()) return call("scout_pause", { paused });
    return mem.running;
  },
  async draftModelOptions(): Promise<ModelOption[]> {
    if (inTauri()) return call("draft_model_options");
    return [
      { value: "claude::sonnet", label: "Claude (subscription) - Sonnet" },
      { value: "grok::grok-4", label: "Grok - Grok 4" },
    ];
  },
  async scoutQueue(): Promise<QueueItem[]> {
    if (inTauri()) return call("scout_queue");
    return mem.queue.filter((q) => q.score >= mem.threshold);
  },
  async scoutDismiss(evaluationId: string, reason: string): Promise<void> {
    if (inTauri()) return call("scout_dismiss", { evaluationId, reason });
    mem.queue = mem.queue.filter((q) => q.evaluation_id !== evaluationId);
  },
  async setThreshold(threshold: number): Promise<number> {
    if (inTauri()) return call("scout_set_threshold", { threshold });
    mem.threshold = threshold;
    return threshold;
  },
  async watchlist(): Promise<string[]> {
    return inTauri() ? call("watchlist_get") : [...mem.watchlist];
  },
  async setWatchlist(handles: string[]): Promise<string[]> {
    if (inTauri()) return call("watchlist_set", { handles });
    mem.watchlist = handles;
    return handles;
  },

  async draftOpen(evaluationId: string): Promise<DraftView> {
    if (inTauri()) return call("draft_open", { evaluationId });
    const item = mem.queue.find((q) => q.evaluation_id === evaluationId);
    if (!item) throw new Error("that post is no longer in the queue");
    return mockDraft(item);
  },
  async draftGet(draftId: string): Promise<DraftView> {
    if (inTauri()) return call("draft_get", { draftId });
    return { ...mem.drafts.get(draftId)! };
  },
  async draftSaveEdit(draftId: string, text: string): Promise<DraftView> {
    if (inTauri()) return call("draft_save_edit", { draftId, text });
    const d = mem.drafts.get(draftId)!;
    return put({ ...d, edited_text: text });
  },
  async draftSelectVariant(draftId: string, variantId: string): Promise<DraftView> {
    if (inTauri()) return call("draft_select_variant", { draftId, variantId });
    const d = mem.drafts.get(draftId)!;
    const v = d.variants.find((x) => x.id === variantId)!;
    return put({ ...d, edited_text: v.text, tone_tag: v.tone });
  },
  async draftSetActions(draftId: string, actions: ActionName[]): Promise<DraftView> {
    if (inTauri()) return call("draft_set_actions", { draftId, actions });
    const d = mem.drafts.get(draftId)!;
    return put({ ...d, actions: normalizeBundle(actions) });
  },
  async draftDiscard(draftId: string, reason: string | null): Promise<void> {
    if (inTauri()) return call("draft_discard", { draftId, reason });
    mem.drafts.delete(draftId);
  },
  async draftCheck(draftId: string): Promise<string[]> {
    if (inTauri()) return call("draft_check", { draftId });
    const d = mem.drafts.get(draftId)!;
    return [...d.edited_text].length > 280 ? ["The text is longer than 280 characters."] : [];
  },
  async draftGenerate(draftId: string, instruction: string | null, model: string | null): Promise<DraftView> {
    if (inTauri()) return call("draft_generate", { draftId, instruction, model });
    const d = mem.drafts.get(draftId)!;
    const n = d.variants.length + 1;
    const text = instruction === "shorter" ? d.edited_text.split(".")[0] + "." : `${d.edited_text} (${instruction ?? "fresh"} ${n})`;
    const variant = { id: `${d.id}-${n}`, text, tone: instruction ?? "fresh", model_used: model ?? "preview" };
    return put({ ...d, variants: [...d.variants, variant], edited_text: text });
  },

  async publishDraft(draftId: string, confirmedText: string): Promise<void> {
    if (inTauri()) return call("publish_draft", { draftId, confirmedText });
    const d = mem.drafts.get(draftId)!;
    put({ ...d, status: "published", published_url: "https://x.com/i/status/9001", steps: d.actions.map((a) => ({ action: a, status: "done", detail: null })) });
  },
  async engage(observedPostId: string, action: ActionName, undo: boolean): Promise<JobResult> {
    if (inTauri()) return call("engage_action", { observedPostId, action, undo });
    return { ok: true, message: null, published_post_id: null };
  },
  async actionState(observedPostId: string, refresh: boolean): Promise<PostActionState> {
    if (inTauri()) return call("get_action_state", { observedPostId, refresh });
    return { known: { like: false, bookmark: false, repost: false, follow: false, replied: false, quoted: false }, live: null };
  },

  async killSwitch(): Promise<boolean> {
    return inTauri() ? call("get_kill_switch") : mem.kill;
  },
  async setKillSwitch(engaged: boolean): Promise<boolean> {
    if (inTauri()) return call("set_kill_switch", { engaged });
    mem.kill = engaged;
    return engaged;
  },
  async pacing(): Promise<PacingConfig> {
    return inTauri() ? call("get_pacing") : { ...mem.pacing };
  },
  async setPacing(config: PacingConfig): Promise<PacingConfig> {
    if (inTauri()) return call("set_pacing", { config });
    mem.pacing = config;
    return config;
  },
  async pacingUsage(): Promise<PacingUsage[]> {
    return inTauri() ? call("pacing_usage") : [];
  },
  async notifications(): Promise<NotificationSettings> {
    return inTauri() ? call("get_notifications") : { ...mem.notifications };
  },
  async setNotifications(settings: NotificationSettings): Promise<NotificationSettings> {
    if (inTauri()) return call("set_notifications", { settings });
    mem.notifications = settings;
    return settings;
  },
  async completionStatus(): Promise<CompletionStatus> {
    return inTauri() ? call("completion_status") : { scoring: false, drafting: false, browser_navigation: false };
  },
  async retentionDays(): Promise<number> {
    return inTauri() ? call("retention_get") : mem.retention;
  },
  async setRetentionDays(days: number): Promise<number> {
    if (inTauri()) return call("retention_set", { days });
    mem.retention = days;
    return days;
  },
  async activity(limit: number): Promise<ActivityEntry[]> {
    return inTauri() ? call("activity_list", { limit }) : mem.activity.slice(0, limit);
  },
  async runChecklist(postUrl: string): Promise<ChecklistReport> {
    if (inTauri()) return call("checklist_run", { postUrl });
    const r: ChecklistReport = {
      id: `c${mem.reports.length + 1}`,
      registry_version: 1,
      created_at: now(),
      post_url: postUrl,
      items: [
        { action: "like", status: "pass", detail: "liked and unliked" },
        { action: "bookmark", status: "pass", detail: "added and removed" },
        { action: "repost", status: "pass", detail: "reposted and undone" },
        { action: "follow", status: "fail", detail: "no follow control found" },
        { action: "reply", status: "manual", detail: "confirm by hand on a test post" },
        { action: "quote", status: "manual", detail: "confirm by hand on a test post" },
      ],
    };
    mem.reports.unshift(r);
    return r;
  },
  async checklistReports(): Promise<ChecklistReport[]> {
    return inTauri() ? call("checklist_reports") : [...mem.reports];
  },
};

export { listen };
