import { inTauri, listen } from "../lib/api";
import { modeOfBundle, normalizeBundle } from "./logic";
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
  FormatFetchSummary,
  QueueItem,
  ReplyVoice,
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
const SAMPLE_SVG =
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><rect width="640" height="360" fill="#18181b"/><path d="M40 300 L170 250 L300 210 L440 120 L590 60" stroke="#10b981" stroke-width="5" fill="none"/><text x="40" y="54" fill="#e4e4e7" font-family="sans-serif" font-size="22" font-weight="600">Revenue per engineer ($M)</text></svg>',
  );

const AVATAR = (c: string, ch: string) =>
  "data:image/svg+xml;utf8," +
  encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="${c}"/><text x="32" y="43" text-anchor="middle" fill="#fff" font-family="sans-serif" font-size="30" font-weight="700">${ch}</text></svg>`);

const now = () => new Date().toISOString();
const memVoice: ReplyVoice = { prompt: "", formats: [] };

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
      post: {
        observed_id: "o1",
        platform_post_id: "111",
        handle: "naval",
        author_name: "Naval",
        avatar_url: AVATAR("#0f766e", "N"),
        text: "The best way to build wealth is to own equity in something that scales without you.",
        has_media: true,
        url: "https://x.com/naval/status/111",
        media: [{ kind: "image", url: SAMPLE_SVG, poster_url: null, alt: "Revenue per engineer chart", duration: null }],
      },
      score: 91,
      outlier_score: 86,
      breakout_flags: ["high-saves", "3.4x-reach", "reply-headroom"],
      author_followers: 18400,
      author_verified: true,
      angle: "Add the counterpoint: equity needs a team first",
      reasoning: "Fresh post (12 min) with fast likes and few replies, squarely in your startups topic.",
      model_used: "preview",
      status: "queued",
      metrics: { replies: 14, reposts: 40, likes: 612, bookmarks: 145, views: 22000 },
      posted_at: now(),
      draft_id: null,
      draft_text: null,
      draft_status: null,
      draft_error: null,
      draft_published_url: null,
      created_at: now(),
    },
    {
      evaluation_id: "e2",
      session_id: "s1",
      post: {
        observed_id: "o2",
        platform_post_id: "222",
        handle: "paulg",
        author_name: "Paul Graham",
        avatar_url: AVATAR("#9a3412", "P"),
        text: "Startups are mostly about not dying until the market comes to you.",
        has_media: true,
        url: "https://x.com/paulg/status/222",
        media: [{ kind: "video", url: null, poster_url: SAMPLE_SVG, alt: "Founder clip", duration: "0:42" }],
      },
      score: 78,
      outlier_score: 64,
      breakout_flags: ["high-efficiency"],
      author_followers: 92000,
      author_verified: true,
      angle: "Share a concrete survival tactic",
      reasoning: "Strong engagement velocity, low reply count.",
      model_used: "preview",
      status: "queued",
      metrics: { replies: 30, reposts: 90, likes: 1400, bookmarks: 120, views: 61000 },
      posted_at: now(),
      draft_id: null,
      draft_text: null,
      draft_status: null,
      draft_error: null,
      draft_published_url: null,
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
    outlier_score: item.outlier_score,
    breakout_flags: item.breakout_flags,
    author_followers: item.author_followers,
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
  async scoutStop(): Promise<boolean> {
    if (inTauri()) return call("scout_stop");
    const wasRunning = mem.running;
    mem.running = false;
    return wasRunning;
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
    const bundle = normalizeBundle(actions);
    return put({ ...d, actions: bundle, kind: modeOfBundle(bundle) });
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
    const url = "https://x.com/i/status/9001";
    put({
      ...d,
      edited_text: confirmedText,
      status: "published",
      published_url: url,
      steps: d.actions.map((a) => ({ action: a, status: "done", detail: null })),
    });
    const q = mem.queue.find((item) => item.evaluation_id === d.evaluation_id);
    if (q) {
      q.draft_id = d.id;
      q.draft_text = confirmedText;
      q.draft_status = "published";
      q.draft_published_url = url;
    }
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
    window.dispatchEvent(new CustomEvent("sgh:kill", { detail: engaged }));
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
  async replyVoiceGet(): Promise<ReplyVoice> {
    return inTauri() ? call("reply_voice_get") : { ...memVoice, formats: [...memVoice.formats] };
  },
  async replyVoiceSetPrompt(prompt: string): Promise<ReplyVoice> {
    if (inTauri()) return call("reply_voice_set_prompt", { prompt });
    memVoice.prompt = prompt.trim();
    return { ...memVoice, formats: [...memVoice.formats] };
  },
  async replyFormatSetEnabled(id: string, enabled: boolean): Promise<ReplyVoice> {
    if (inTauri()) return call("reply_format_set_enabled", { id, enabled });
    memVoice.formats = memVoice.formats.map((f) => (f.id === id ? { ...f, enabled } : f));
    return { ...memVoice, formats: [...memVoice.formats] };
  },
  async replyFormatDelete(id: string): Promise<ReplyVoice> {
    if (inTauri()) return call("reply_format_delete", { id });
    memVoice.formats = memVoice.formats.filter((f) => f.id !== id);
    return { ...memVoice, formats: [...memVoice.formats] };
  },
  async replyFormatsFetch(): Promise<FormatFetchSummary> {
    if (inTauri()) return call("reply_formats_fetch");
    const sample = [
      { name: "Counter then proof", pattern: "Open with a short counterpoint to [claim], then back it with [one concrete example].", when_to_use: "Confident opinions", why_it_works: "Adds a real second view." },
      { name: "Pointed question", pattern: "Ask the one [follow-up question] the post leaves open, in under fifteen words.", when_to_use: "Claims without detail", why_it_works: "Invites the author to answer." },
    ].filter((s) => !memVoice.formats.some((f) => f.name === s.name));
    const added = sample.map((s, i) => ({ ...s, id: `rf-${Date.now()}-${i}`, source_replies: 9, avg_likes: 24.5, enabled: true, created_at: now() }));
    memVoice.formats = [...added, ...memVoice.formats];
    return { posts_read: 6, replies_used: 9, added, duplicates: 2 - added.length };
  },
};

export { listen };
