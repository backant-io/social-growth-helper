export interface Preferences {
  niche_topics: string[];
  prefer_accounts: string[];
  avoid_accounts: string[];
  languages: string[];
}

export interface RulesNotice {
  version: number;
  text: string;
}

export interface RulesStatus {
  current_version: number;
  acknowledged_version: number | null;
  acknowledged_at: string | null;
  acknowledged: boolean;
}

export interface OnboardingStatus {
  rules: RulesStatus;
  tour_completed: boolean;
  preferences_saved: boolean;
}

export interface DistroSteps {
  name: string;
  steps: string[];
}

export interface SetupGuide {
  title: string;
  summary: string;
  no_file_fallback: boolean;
  options: DistroSteps[];
  verify: string;
}

export interface SecretsStatus {
  platform: "macos" | "linux" | "other";
  available: boolean;
  backend: string;
  issue: string | null;
  guide: SetupGuide | null;
}

export interface BrowserInstall {
  path: string;
  kind: "chrome" | "chromium" | "brave" | "custom";
  from_override: boolean;
}

export interface BrowserInfo {
  install: BrowserInstall | null;
  problem: string | null;
  override_path: string | null;
  running: boolean;
}

export interface VisibleTweet {
  id: string | null;
  handle: string | null;
  author: string;
  text: string;
  promoted: boolean;
  time: string | null;
}

export interface AppInfo {
  version: string;
  data_dir: string;
  db_path: string;
  schema_version: number;
  platform: string;
  arch: string;
}

export type SectionId = "scout" | "reply" | "creator" | "analytics" | "learning" | "style" | "settings";

export type QuitGuardPayload = { kind: "needs_confirmation"; jobs: string[] } | { kind: "proceed" };

// ---------------------------------------------------------------------------
// Phase 2: LLM Router, Providers, Prompts, Usage & Style Studio
// ---------------------------------------------------------------------------

export type TaskType =
  | "browser_navigation"
  | "post_scoring"
  | "reply_drafting"
  | "ogc_generation"
  | "format_extraction"
  | "learning_analysis"
  | "dashboard_builder";

export interface TaskMeta {
  id: TaskType;
  label: string;
  description: string;
  requiresVision: boolean;
}

export const TASK_METAS: TaskMeta[] = [
  {
    id: "browser_navigation",
    label: "Browser Navigation",
    description: "Locates X UI elements from screenshots when DOM selectors miss (vision required).",
    requiresVision: true,
  },
  {
    id: "post_scoring",
    label: "Post Scoring",
    description: "Scores Scout feed posts for reply potential and relevance.",
    requiresVision: false,
  },
  {
    id: "reply_drafting",
    label: "Reply Drafting",
    description: "Generates voice-matched reply variants and runs Style Test Bench.",
    requiresVision: false,
  },
  {
    id: "ogc_generation",
    label: "OGC Generation",
    description: "Drafts original posts from topic seeds and viral formats.",
    requiresVision: false,
  },
  {
    id: "format_extraction",
    label: "Format Extraction",
    description: "Decomposes winning posts into reusable structural blueprints.",
    requiresVision: false,
  },
  {
    id: "learning_analysis",
    label: "Learning Analysis",
    description: "Synthesizes engagement & edit diffs into style/rubric proposals.",
    requiresVision: false,
  },
  {
    id: "dashboard_builder",
    label: "Dashboard Builder",
    description: "Translates natural-language analytics questions into read-only widget specs.",
    requiresVision: false,
  },
];

export type ProviderType = "claude_cli" | "xai_oauth" | "xai_api" | "anthropic_api" | "grok_web";
export type BillingLane = "subscription" | "api";
export type ProviderHealth = "connected" | "needs_login" | "expired" | "offline" | "error" | "disconnected";

export interface ProviderCapabilities {
  vision: boolean;
  json_schema: boolean;
  streaming: boolean;
  thinking: boolean;
}

export interface ModelInfo {
  id: string;
  display_name: string;
  provider_type: ProviderType;
  capabilities: ProviderCapabilities;
  is_custom: boolean;
}

export interface ProviderAccountConfig {
  custom_models: string[];
  endpoint_override: string | null;
  cli_binary_path: string | null;
}

export interface ProviderAccountView {
  id: string;
  provider_type: ProviderType;
  label: string;
  billing_lane: BillingLane;
  status: ProviderHealth;
  last_verified_at: string | null;
  last_error: string | null;
  has_secret: boolean;
  config: ProviderAccountConfig;
  models: ModelInfo[];
  personal_use_notice: string | null;
}

export interface ClaudeCliDetection {
  found: boolean;
  binary_path: string | null;
  version: string | null;
  error: string | null;
}

export interface ClaudeCliDetectionResponse {
  detection: ClaudeCliDetection;
  personal_use_notice: string;
}

export interface XaiDeviceChallenge {
  device_code: string;
  user_code: string;
  verification_uri: string;
  verification_uri_complete: string | null;
  expires_in: number;
  interval: number;
  token_endpoint: string;
}

export type DevicePollOutcome =
  | { state: "pending"; slow_down: boolean }
  | { state: "authorized" }
  | { state: "expired" }
  | { state: "denied" };

export interface XaiOAuthPollResult {
  outcome: DevicePollOutcome;
  account: ProviderAccountView | null;
}

export interface TaskModelRoute {
  task_type: TaskType;
  provider_account_id: string | null;
  model_id: string | null;
  reasoning_effort: string | null;
  temperature: number | null;
  fallback_account_id: string | null;
  fallback_model_id: string | null;
  max_calls_per_hour: number | null;
  updated_at: string;
}

export interface TaskModelRoutePatch {
  task_type: TaskType;
  provider_account_id: string | null;
  model_id: string | null;
  reasoning_effort: string | null;
  temperature: number | null;
  fallback_account_id: string | null;
  fallback_model_id: string | null;
  max_calls_per_hour: number | null;
}

export interface UsageSummaryRow {
  task_type: TaskType;
  provider_type: ProviderType;
  model_id: string;
  total_calls: number;
  successful_calls: number;
  fallback_calls: number;
  input_tokens: number;
  output_tokens: number;
  avg_latency_ms: number;
}

export interface TaskHourlyCount {
  task_type: TaskType;
  calls_last_hour: number;
  max_calls_per_hour: number | null;
}

export interface UsageDashboard {
  window_days: number;
  total_calls: number;
  total_input_tokens: number;
  total_output_tokens: number;
  rows: UsageSummaryRow[];
  hourly_counts: TaskHourlyCount[];
}

export interface PromptTemplate {
  id: string;
  task_type: TaskType;
  version: number;
  system_prompt: string;
  user_template: string;
  is_override: boolean;
  is_active: boolean;
  created_at: string;
}

export type EmojiPolicy = "none" | "minimal" | "allowed";
export type HashtagPolicy = "none" | "max_1" | "allowed";
export type ExampleKind = "reply" | "ogc";

export interface StyleGuide {
  id: string;
  profile_id: string;
  version: number;
  free_text_guide: string;
  learned_rules: string[];
  banned_patterns: string[];
  max_reply_chars: number;
  emoji_policy: EmojiPolicy;
  hashtag_policy: HashtagPolicy;
  note: string | null;
  is_active: boolean;
  created_at: string;
}

export interface StyleGuideInput {
  free_text_guide: string;
  learned_rules: string[];
  banned_patterns: string[];
  max_reply_chars: number;
  emoji_policy: EmojiPolicy;
  hashtag_policy: HashtagPolicy;
  note: string | null;
}

export interface StyleExample {
  id: string;
  profile_id: string;
  kind: ExampleKind;
  source_context_text: string | null;
  example_text: string;
  why_it_works: string | null;
  tags: string[];
  pinned: boolean;
  source: string;
  performance_score: number | null;
  created_at: string;
}

export interface StyleExampleInput {
  id: string | null;
  kind: ExampleKind;
  source_context_text: string | null;
  example_text: string;
  why_it_works: string | null;
  tags: string[];
  pinned: boolean;
  source: string | null;
  performance_score: number | null;
}

export interface RetrievedExample {
  example: StyleExample;
  reason: string;
  score: number | null;
}

export interface ImportCandidate {
  kind: ExampleKind;
  source_context_text: string | null;
  example_text: string;
  tags: string[];
  source: string;
  author_handle: string | null;
  external_id: string | null;
}

export interface RouteOverrideInput {
  provider_account_id: string;
  model_id: string;
  reasoning_effort: string | null;
  temperature: number | null;
}

export interface LlmUsage {
  input_tokens: number;
  output_tokens: number;
}

export interface ConstraintCheck {
  passed: boolean;
  char_count: number;
  max_reply_chars: number;
  banned_hits: string[];
  emoji_count: number;
  hashtag_count: number;
  violations: string[];
}

export interface BenchVariant {
  tone_tag: string;
  text: string;
  check: ConstraintCheck;
}

export interface BenchModelRun {
  model_id: string;
  provider_type: ProviderType;
  provider_label: string;
  billing_lane: BillingLane;
  used_fallback: boolean;
  fallback_reason: string | null;
  latency_ms: number;
  usage: LlmUsage;
  variants: BenchVariant[];
  rejected_variants: BenchVariant[];
}

export interface StyleBenchInput {
  sample_author: string | null;
  sample_post_text: string;
  override_route: RouteOverrideInput | null;
  compare_route: RouteOverrideInput | null;
  example_limit: number | null;
}

export interface StyleBenchResult {
  style_guide_version: number;
  prompt_version: number;
  system_prompt: string;
  rendered_user_prompt: string;
  retrieved_examples: RetrievedExample[];
  primary: BenchModelRun;
  comparison: BenchModelRun | null;
}
