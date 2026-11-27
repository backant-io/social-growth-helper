export type ActionName = "like" | "reply" | "repost" | "quote" | "bookmark" | "follow";

export interface Metrics {
  replies: number;
  reposts: number;
  likes: number;
  bookmarks: number;
  views: number;
}

export interface MediaItem {
  kind: "image" | "video" | "gif";
  url: string | null;
  poster_url: string | null;
  alt: string | null;
  duration: string | null;
}

export interface PostView {
  observed_id: string;
  platform_post_id: string;
  handle: string;
  author_name: string;
  avatar_url: string | null;
  text: string;
  has_media: boolean;
  url: string;
  media: MediaItem[];
}

export interface QueueItem {
  evaluation_id: string;
  session_id: string | null;
  post: PostView;
  score: number;
  outlier_score: number | null;
  breakout_flags: string[];
  author_followers: number | null;
  author_verified: boolean;
  angle: string;
  reasoning: string;
  model_used: string;
  status: string;
  metrics: Metrics;
  posted_at: string | null;
  draft_id: string | null;
  draft_text: string | null;
  draft_status: string | null;
  draft_error: string | null;
  draft_published_url: string | null;
  created_at: string;
}

export interface FilterCounts {
  invalid: number;
  promoted: number;
  own: number;
  seen: number;
  stale: number;
  reply_restricted: number;
  avoided: number;
  language: number;
  sensitive?: number;
}

export interface SessionSummary {
  reason: string;
  scanned: number;
  scored: number;
  queued: number;
  filtered: FilterCounts;
  model: string;
  heuristic_scored: number;
}

export interface SessionView {
  id: string;
  started_at: string;
  ended_at: string | null;
  ended_reason: string | null;
  max_duration_sec: number;
  max_posts: number;
  stop_after_queued: number | null;
  threshold: number;
  posts_scanned: number;
  posts_queued: number;
  summary: SessionSummary | null;
}

export interface ScoutStatus {
  running: boolean;
  session_id: string | null;
  threshold: number;
  last_params: SessionParams | null;
  recent: SessionView[];
}

export interface SessionParams {
  feed_source: "for_you" | "following" | "watchlist" | "both" | "all";
  max_duration_sec: number;
  max_posts: number;
  stop_after_queued: number | null;
  threshold: number;
}

export interface ScoutProgress {
  session_id: string;
  scanned: number;
  queued: number;
  scored: number;
  elapsed_sec: number;
  target?: number | null;
}

export interface VariantView {
  id: string;
  text: string;
  tone: string | null;
  model_used: string;
}

export interface StepView {
  action: string;
  status: "pending" | "done" | "skipped" | "failed" | "uncertain";
  detail: string | null;
}

export interface DraftView {
  id: string;
  evaluation_id: string | null;
  post: PostView;
  angle: string | null;
  reasoning: string | null;
  score: number | null;
  outlier_score: number | null;
  breakout_flags: string[];
  author_followers: number | null;
  kind: string;
  initial_text: string;
  edited_text: string;
  edit_distance: number;
  tone_tag: string | null;
  model_used: string;
  actions: ActionName[];
  status: "draft" | "queued_for_publish" | "publishing" | "published" | "failed" | "discarded";
  error_message: string | null;
  published_url: string | null;
  variants: VariantView[];
  steps: StepView[];
  created_at: string;
}

export interface ActionState {
  like: boolean;
  bookmark: boolean;
  repost: boolean;
  follow: boolean;
  replied: boolean;
  quoted: boolean;
}

export interface LiveState {
  like: string;
  bookmark: string;
  repost: string;
  follow: string;
}

export interface PostActionState {
  known: ActionState;
  live: LiveState | null;
}

export interface JobResult {
  ok: boolean;
  message: string | null;
  published_post_id: string | null;
}

export interface PacingConfig {
  enabled: boolean;
  like: number;
  reply: number;
  repost: number;
  quote: number;
  bookmark: number;
  follow: number;
  min_gap_sec: number;
}

export interface PacingUsage {
  action: string;
  used_24h: number;
  cap: number;
}

export interface NotificationSettings {
  enabled: boolean;
  scout_done: boolean;
  publish_done: boolean;
  publish_failed: boolean;
  attention: boolean;
}

export interface CompletionStatus {
  scoring: boolean;
  drafting: boolean;
  browser_navigation: boolean;
}

export interface ActivityEntry {
  id: string;
  kind: "scout" | "publish" | "action" | "challenge" | "error";
  title: string;
  detail: string | null;
  ok: boolean | null;
  at: string;
  screenshot: string | null;
}

export interface ChecklistItem {
  action: string;
  status: "pass" | "fail" | "skipped" | "manual";
  detail: string;
}

export interface ChecklistReport {
  id: string;
  registry_version: number;
  created_at: string;
  post_url: string;
  items: ChecklistItem[];
}

export interface ModelOption {
  value: string;
  label: string;
}

export interface ReplyFormat {
  id: string;
  name: string;
  pattern: string;
  when_to_use: string;
  why_it_works: string;
  source_replies: number;
  avg_likes: number;
  enabled: boolean;
  created_at: string;
}

export interface ReplyVoice {
  /** The saved prompt; empty while the built-in opening is in use. */
  prompt: string;
  formats: ReplyFormat[];
}

export interface FormatFetchSummary {
  posts_read: number;
  replies_used: number;
  added: ReplyFormat[];
  duplicates: number;
}
