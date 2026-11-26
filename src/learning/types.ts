export type PatchTarget = "rubric" | "style" | "examples";

export type PatchOp =
  | { op: "add_criterion"; text: string }
  | { op: "remove_criterion"; text: string }
  | { op: "set_weight"; key: string; value: number }
  | { op: "add_rule"; text: string }
  | { op: "remove_rule"; text: string }
  | { op: "add_banned"; pattern: string }
  | { op: "remove_banned"; pattern: string }
  | {
      op: "promote_example";
      draft_id: string;
      example_text: string;
      source_context_text?: string | null;
      why_it_works?: string | null;
      performance_score?: number | null;
    };

export interface Patch {
  target: PatchTarget;
  ops: PatchOp[];
}

export interface EvidenceItem {
  label: string;
  draft_id: string | null;
  detail: string;
}

export interface Evidence {
  support: number;
  total: number;
  items: EvidenceItem[];
}

export interface DiffLine {
  kind: "add" | "remove" | "same";
  text: string;
}

export type InsightStatus = "proposed" | "accepted" | "rejected" | "rolled_back";

export interface Insight {
  id: string;
  cycle_date: string;
  category: string;
  hypothesis: string;
  evidence: Evidence;
  confidence: number;
  patch: Patch;
  original_patch: Patch | null;
  edited: boolean;
  status: InsightStatus;
  decided_at: string | null;
  created_at: string;
  sample_size: number;
  low_confidence: boolean;
  applied_version: number | null;
  source: string;
  diff: DiffLine[];
  stale: boolean;
}

export interface PhraseStat {
  phrase: string;
  count: number;
  draft_ids: string[];
}

export interface EditReport {
  drafts: number;
  edited_drafts: number;
  avg_edit_distance: number;
  shortened_drafts: number;
  median_final_length: number | null;
  avg_length_change_pct: number;
  emoji_removed_drafts: number;
  emoji_added_drafts: number;
  hashtag_removed_drafts: number;
  removed: PhraseStat[];
  added: PhraseStat[];
}

export interface DismissalReport {
  dismissed: number;
  drafted: number;
  avg_score_dismissed: number | null;
  avg_score_drafted: number | null;
  reasons: [string, number][];
  authors: [string, number][];
}

export interface ModelStat {
  model: string;
  drafts: number;
  kept_as_is: number;
  kept_lightly_edited: number;
  keep_rate: number;
  avg_edit_distance: number;
  avg_normalized: number | null;
}

export interface BucketStat {
  label: string;
  n: number;
  mean_normalized: number;
}

export interface TimingReport {
  age: BucketStat[];
  tier: BucketStat[];
  overall_mean: number | null;
  scored_items: number;
}

export interface OutcomeBrief {
  draft_id: string;
  text: string;
  normalized: number | null;
  impressions: number | null;
  mature: boolean;
}

export type ReflectionMode = "light" | "deep";

export interface ReflectionSummary {
  run_id: string;
  mode: ReflectionMode;
  published_items: number;
  min_sample: number;
  low_confidence: boolean;
  insights_created: number;
  used_llm: boolean;
  note: string;
  started_at: string;
}

export interface Signals {
  published_items: number;
  mature_items: number;
  baseline_impressions: number | null;
  min_sample: number;
  low_confidence: boolean;
  edit: EditReport;
  dismissal: DismissalReport;
  models: ModelStat[];
  timing: TimingReport;
  rubric_correlation: number | null;
  best: OutcomeBrief[];
  weakest: OutcomeBrief[];
  last_run: ReflectionSummary | null;
  proposed: number;
}

export interface LearningOverview {
  signals: Signals;
  proposed: Insight[];
  decided: Insight[];
}

export type VersionKind = "rubric";

export interface VersionInfo {
  kind: VersionKind;
  version: number;
  is_active: boolean;
  created_at: string;
  lines: string[];
  diff: DiffLine[];
  insight_id: string | null;
}

export interface ChangelogEntry {
  id: string;
  kind: "rubric" | "style" | "examples";
  action: "accepted" | "rolled_back";
  from_version: number | null;
  to_version: number | null;
  insight_id: string | null;
  created_at: string;
}

export interface ExampleInfo {
  id: string;
  kind: string;
  example_text: string;
  source_context_text: string | null;
  why_it_works: string | null;
  source: string;
  performance_score: number | null;
  pinned: boolean;
  created_at: string;
}

export interface ExportSummary {
  path: string;
  bytes: number;
  tables: Record<string, number>;
  excluded: string[];
}

export interface Manifest {
  format: string;
  format_version: number;
  app_version: string;
  schema_version: number;
  created_at: string;
  tables: Record<string, number>;
  excluded: string[];
}

export interface ImportSummary {
  tables: Record<string, number>;
  rows: number;
  accounts_needing_keys: number;
  exported_at: string;
  source_app_version: string;
  safety_backup: string | null;
  warnings: string[];
}

export interface WipeReport {
  rows_deleted: number;
  keychain_removed: number;
  files_removed: number;
  warnings: string[];
}
