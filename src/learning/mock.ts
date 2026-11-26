import type {
  ChangelogEntry,
  ExampleInfo,
  Insight,
  LearningOverview,
  Patch,
  PatchOp,
  ReflectionSummary,
  Signals,
  VersionInfo,
  VersionKind,
} from "./types";

export const LEARNING_MOCK_KEY = "sgh-mock-learning";

interface MockLearning {
  insights: Insight[];
  versions: { rubric: VersionInfo[] };
  examples: ExampleInfo[];
  changelog: ChangelogEntry[];
  minSample: number;
  lastRun: ReflectionSummary | null;
}

const day = (n: number) => new Date(Date.UTC(2026, 10, 26 - n, 9, 0, 0)).toISOString();

function opLines(op: PatchOp): { add?: string; remove?: string } {
  switch (op.op) {
    case "add_banned":
      return { add: `never: ${op.pattern}` };
    case "remove_banned":
      return { remove: `never: ${op.pattern}` };
    case "add_rule":
      return { add: `rule: ${op.text}` };
    case "remove_rule":
      return { remove: `rule: ${op.text}` };
    case "add_criterion":
      return { add: `- ${op.text}` };
    case "remove_criterion":
      return { remove: `- ${op.text}` };
    case "set_weight":
      return { add: `weight ${op.key} = ${op.value}` };
    default:
      return {};
  }
}

function seed(): MockLearning {
  const insights: Insight[] = [
    {
      id: "i1",
      cycle_date: "2026-11-26",
      category: "anti_pattern",
      hypothesis: 'You delete "great point" in 7 of 11 edited drafts. Ban it so drafts stop including it.',
      evidence: {
        support: 7,
        total: 11,
        items: [
          { label: "removed in edit", draft_id: "d-104", detail: "" },
          { label: "removed in edit", draft_id: "d-101", detail: "" },
          { label: "removed in edit", draft_id: "d-097", detail: "" },
        ],
      },
      confidence: 0.5,
      patch: { target: "style", ops: [{ op: "add_banned", pattern: "great point" }] },
      original_patch: null,
      edited: false,
      status: "proposed",
      decided_at: null,
      created_at: day(0),
      sample_size: 18,
      low_confidence: true,
      applied_version: null,
      source: "rules",
      diff: [
        { kind: "same", text: "never: love this" },
        { kind: "add", text: "never: great point" },
      ],
      stale: false,
    },
    {
      id: "i2",
      cycle_date: "2026-11-26",
      category: "scout_timing",
      hypothesis: "Replies sent 15 to 60 minutes after the original averaged 1.9x your baseline (6 replies, overall 1.1x).",
      evidence: { support: 6, total: 16, items: [{ label: "15 to 60 minutes", draft_id: null, detail: "1.92x over 6 replies" }] },
      confidence: 0.5,
      patch: { target: "rubric", ops: [{ op: "add_criterion", text: "Prefer posts you can answer within 15 to 60 minutes of when they were posted" }] },
      original_patch: null,
      edited: false,
      status: "proposed",
      decided_at: null,
      created_at: day(0),
      sample_size: 18,
      low_confidence: true,
      applied_version: null,
      source: "rules",
      diff: [
        { kind: "same", text: "Niche topics: rust, local-first" },
        { kind: "add", text: "- Prefer posts you can answer within 15 to 60 minutes of when they were posted" },
      ],
      stale: false,
    },
    {
      id: "i3",
      cycle_date: "2026-11-26",
      category: "few_shot_promotion",
      hypothesis: "This reply did 2.8x your usual results after adjusting for the author's reach. Use it as a voice example.",
      evidence: { support: 1, total: 18, items: [{ label: "winner", draft_id: "d-088", detail: "4210 impressions, 2.84x baseline" }] },
      confidence: 0.5,
      patch: {
        target: "examples",
        ops: [{ op: "promote_example", draft_id: "d-088", example_text: "Shipping beats polishing. A rough version in front of ten people teaches more than a perfect one in your head.", why_it_works: "2.8x your baseline reply performance", performance_score: 2.84 }],
      },
      original_patch: null,
      edited: false,
      status: "proposed",
      decided_at: null,
      created_at: day(0),
      sample_size: 18,
      low_confidence: true,
      applied_version: null,
      source: "rules",
      diff: [{ kind: "add", text: "example: Shipping beats polishing. A rough version in front of ten people teaches more than a perfect one in your head." }],
      stale: false,
    },
    {
      id: "i0",
      cycle_date: "2026-11-19",
      category: "voice",
      hypothesis: "You remove emoji from 5 of 11 edited drafts.",
      evidence: { support: 5, total: 11, items: [] },
      confidence: 0.5,
      patch: { target: "style", ops: [{ op: "add_rule", text: "Do not use emoji" }] },
      original_patch: null,
      edited: false,
      status: "accepted",
      decided_at: day(7),
      created_at: day(7),
      sample_size: 12,
      low_confidence: true,
      applied_version: 3,
      source: "rules",
      diff: [],
      stale: false,
    },
  ];
  return {
    insights,
    versions: {
      rubric: [
        { kind: "rubric", version: 1, is_active: true, created_at: day(40), lines: ["Niche topics: rust, local-first"], diff: [], insight_id: null },
      ],
    },
    examples: [],
    changelog: [{ id: "c1", kind: "style", action: "accepted", from_version: null, to_version: null, insight_id: "i0", created_at: day(7) }],
    minSample: 30,
    lastRun: { run_id: "r1", mode: "light", published_items: 18, min_sample: 30, low_confidence: true, insights_created: 3, used_llm: false, note: "Only 18 published replies so far (minimum 30); suggestions are marked low confidence.", started_at: day(0) },
  };
}

function load(): MockLearning {
  const raw = localStorage.getItem(LEARNING_MOCK_KEY);
  return raw ? (JSON.parse(raw) as MockLearning) : seed();
}

function save(s: MockLearning) {
  localStorage.setItem(LEARNING_MOCK_KEY, JSON.stringify(s));
}

function signals(s: MockLearning): Signals {
  const proposed = s.insights.filter((i) => i.status === "proposed").length;
  return {
    published_items: 18,
    mature_items: 14,
    baseline_impressions: 1180,
    min_sample: s.minSample,
    low_confidence: 18 < s.minSample,
    edit: {
      drafts: 18,
      edited_drafts: 11,
      avg_edit_distance: 24.5,
      shortened_drafts: 8,
      median_final_length: 118,
      avg_length_change_pct: -21,
      emoji_removed_drafts: 5,
      emoji_added_drafts: 0,
      hashtag_removed_drafts: 2,
      removed: [
        { phrase: "great point", count: 7, draft_ids: ["d-104"] },
        { phrase: "absolutely", count: 4, draft_ids: ["d-099"] },
      ],
      added: [{ phrase: "in practice", count: 3, draft_ids: ["d-090"] }],
    },
    dismissal: {
      dismissed: 14,
      drafted: 18,
      avg_score_dismissed: 61.2,
      avg_score_drafted: 78.4,
      reasons: [["wrong niche", 6], ["too late", 4], ["no reason given", 4]],
      authors: [["growthbro", 3]],
    },
    models: [
      { model: "claude-sonnet", drafts: 12, kept_as_is: 3, kept_lightly_edited: 8, keep_rate: 0.67, avg_edit_distance: 21, avg_normalized: 1.2 },
      { model: "gpt-mini", drafts: 6, kept_as_is: 0, kept_lightly_edited: 2, keep_rate: 0.33, avg_edit_distance: 41, avg_normalized: 0.9 },
    ],
    timing: {
      age: [
        { label: "under 15 minutes", n: 4, mean_normalized: 1.3 },
        { label: "15 to 60 minutes", n: 6, mean_normalized: 1.9 },
        { label: "1 to 3 hours", n: 4, mean_normalized: 0.8 },
        { label: "over 3 hours", n: 2, mean_normalized: 0.5 },
      ],
      tier: [
        { label: "under 1k", n: 3, mean_normalized: 0.9 },
        { label: "1k to 10k", n: 8, mean_normalized: 1.4 },
        { label: "10k to 100k", n: 5, mean_normalized: 1.0 },
      ],
      overall_mean: 1.1,
      scored_items: 16,
    },
    rubric_correlation: 0.18,
    best: [{ draft_id: "d-088", text: "Shipping beats polishing. A rough version in front of ten people teaches more than a perfect one in your head.", normalized: 2.84, impressions: 4210, mature: true }],
    weakest: [{ draft_id: "d-061", text: "Totally agree, this is the way.", normalized: 0.31, impressions: 240, mature: true }],
    last_run: s.lastRun,
    proposed,
  };
}

function applyToVersions(s: MockLearning, patch: Patch, insightId: string): number | null {
  if (patch.target === "examples") {
    for (const op of patch.ops) {
      if (op.op === "promote_example") {
        s.examples.unshift({ id: `ex-${op.draft_id}`, kind: "reply", example_text: op.example_text, source_context_text: op.source_context_text ?? null, why_it_works: op.why_it_works ?? null, source: "promoted_winner", performance_score: op.performance_score ?? null, pinned: false, created_at: new Date().toISOString() });
      }
    }
    s.changelog.unshift({ id: `c-${insightId}`, kind: "examples", action: "accepted", from_version: null, to_version: null, insight_id: insightId, created_at: new Date().toISOString() });
    return null;
  }
  if (patch.target === "style") {
    s.changelog.unshift({ id: `c-${insightId}`, kind: "style", action: "accepted", from_version: null, to_version: null, insight_id: insightId, created_at: new Date().toISOString() });
    return null;
  }
  const kind: VersionKind = "rubric";
  const list = s.versions[kind];
  const cur = list.find((v) => v.is_active)!;
  const lines = cur.lines.filter((l) => !patch.ops.some((o) => opLines(o).remove === l));
  const added = patch.ops.map((o) => opLines(o).add).filter((l): l is string => !!l);
  const next = [...lines, ...added];
  const version = Math.max(...list.map((v) => v.version)) + 1;
  list.forEach((v) => (v.is_active = false));
  list.unshift({
    kind,
    version,
    is_active: true,
    created_at: new Date().toISOString(),
    lines: next,
    diff: [...cur.lines.filter((l) => !next.includes(l)).map((text) => ({ kind: "remove" as const, text })), ...next.map((text) => ({ kind: cur.lines.includes(text) ? ("same" as const) : ("add" as const), text }))],
    insight_id: insightId,
  });
  s.changelog.unshift({ id: `c-${insightId}`, kind, action: "accepted", from_version: cur.version, to_version: version, insight_id: insightId, created_at: new Date().toISOString() });
  return version;
}

export const learningMock = {
  overview(): LearningOverview {
    const s = load();
    return { signals: signals(s), proposed: s.insights.filter((i) => i.status === "proposed"), decided: s.insights.filter((i) => i.status !== "proposed") };
  },
  accept(id: string, edited: Patch | null): Insight {
    const s = load();
    const ins = s.insights.find((i) => i.id === id);
    if (!ins || ins.status !== "proposed") throw new Error("this insight was already decided");
    const patch = edited ?? ins.patch;
    if (edited && JSON.stringify(edited) !== JSON.stringify(ins.patch)) {
      ins.original_patch = ins.patch;
      ins.edited = true;
    }
    ins.patch = patch;
    ins.applied_version = applyToVersions(s, patch, id);
    ins.status = "accepted";
    ins.decided_at = new Date().toISOString();
    ins.diff = [];
    save(s);
    return ins;
  },
  reject(id: string) {
    const s = load();
    const ins = s.insights.find((i) => i.id === id);
    if (!ins || ins.status !== "proposed") throw new Error("this insight was already decided");
    ins.status = "rejected";
    ins.decided_at = new Date().toISOString();
    save(s);
  },
  versions(): VersionInfo[] {
    return load().versions.rubric;
  },
  rollback(version: number) {
    const kind: VersionKind = "rubric";
    const s = load();
    const list = s.versions[kind];
    const active = list.find((v) => v.is_active)!;
    if (version >= active.version) throw new Error("roll back to an older version than the active one");
    list.forEach((v) => (v.is_active = v.version === version));
    s.insights.forEach((i) => {
      if (i.status === "accepted" && i.applied_version !== null && i.applied_version > version && i.patch.target === kind) i.status = "rolled_back";
    });
    s.changelog.unshift({ id: `rb-${Date.now()}`, kind, action: "rolled_back", from_version: active.version, to_version: version, insight_id: null, created_at: new Date().toISOString() });
    save(s);
  },
  changelog: () => load().changelog,
  examples: () => load().examples,
  removeExample(id: string) {
    const s = load();
    s.examples = s.examples.filter((e) => e.id !== id);
    save(s);
  },
  setMinSample(value: number): number {
    if (value < 1 || value > 1000) throw new Error("minimum sample size must be between 1 and 1000");
    const s = load();
    s.minSample = value;
    s.insights.forEach((i) => {
      if (i.status === "proposed") i.low_confidence = i.sample_size < value;
    });
    save(s);
    return value;
  },
  reflect(mode: "light" | "deep"): ReflectionSummary {
    const s = load();
    const run: ReflectionSummary = { run_id: `r-${Date.now()}`, mode, published_items: 18, min_sample: s.minSample, low_confidence: 18 < s.minSample, insights_created: 0, used_llm: false, note: "Nothing new to suggest.", started_at: new Date().toISOString() };
    s.lastRun = run;
    save(s);
    return run;
  },
  reset() {
    localStorage.removeItem(LEARNING_MOCK_KEY);
  },
};
