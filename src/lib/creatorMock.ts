import type { CreatorApi } from "./creatorApi";
import type { FormatBlueprint, FormatCandidate, FormatRecord, OgcDraft, ViralThresholds, WatchEntry } from "./types";

let seq = 0;
const id = (p: string) => `${p}-${++seq}`;
const now = () => new Date().toISOString();

const SEED_TEXT = "Unpopular opinion: hustle culture is wrong. It burns people out before they find work they love.";

function record(bp: FormatBlueprint, extra: Partial<FormatRecord> = {}): FormatRecord {
  return {
    ...bp,
    id: id("fmt"),
    observation_count: 1,
    observed_win_rate: 0.42,
    personal_win_rate: null,
    status: "active",
    origin: "detected",
    sources: [{ post_id: "1", author: "alice", url: "https://x.com/alice/status/1", seen_at: now(), score: 61 }],
    example_texts: [SEED_TEXT],
    recent_sightings: 1,
    detected_at: now(),
    updated_at: now(),
    ...extra,
  };
}

const seedBlueprint: FormatBlueprint = {
  name: "Contrarian take with a reason",
  archetype: "contrarian_take",
  hook_formula: "Reject a popular belief in the first line",
  structural_pattern: "Hook that rejects a belief, then one sentence on why, then a reframe",
  skeleton: "Unpopular opinion: {belief} is wrong.\n\n{reason}\n\n{reframe}",
  slots: [
    { key: "belief", label: "Popular belief", hint: "What most people in your niche accept" },
    { key: "reason", label: "Why it is wrong", hint: "One concrete consequence" },
    { key: "reframe", label: "What to do instead", hint: "" },
  ],
  media_hint: "",
};

export function createMockCreator(): CreatorApi {
  let formats: FormatRecord[] = [record(seedBlueprint, { observation_count: 3, recent_sightings: 2 })];
  let candidates: FormatCandidate[] = [
    {
      id: id("cand"),
      platform_post_id: "2",
      author_handle: "bob",
      author_followers: 4200,
      text: "I spent 6 months building in public. Here is what nobody tells you: the audience is not the product.",
      metrics: { likes: 1800, reposts: 210, replies: 95, bookmarks: 420, views: 190000 },
      posted_at: null,
      source: "watchlist",
      virality_score: 74,
      reasons: ["Engagement is 1.2x the author's follower count", "Bookmarks are 23% of likes"],
      status: "pending",
      format_id: null,
      error_message: null,
      created_at: now(),
    },
  ];
  let watch: WatchEntry[] = [];
  let drafts: OgcDraft[] = [];
  let thresholds: ViralThresholds = { min_engagement: 150, reach_multiple: 0.25, bookmark_ratio: 0.15, min_bookmarks: 15, assumed_followers: 5000 };
  let settings = { similarity_threshold: 0.5, max_chars: 280 };

  const draft = (text: string, status: OgcDraft["status"], formatId: string | null, batch: string | null, topic: string): OgcDraft => ({
    id: id("drf"),
    viral_format_id: formatId,
    batch_id: batch,
    topic_seed: topic,
    initial_text: text,
    edited_text: text,
    edit_distance: 0,
    media_paths: [],
    model_used: "preview",
    status,
    similarity: null,
    published_url: null,
    error_message: null,
    user_approved_at: null,
    published_at: null,
    created_at: now(),
  });
  const replaceDraft = (d: OgcDraft) => {
    drafts = drafts.map((x) => (x.id === d.id ? d : x));
    return d;
  };
  const find = (draftId: string) => {
    const d = drafts.find((x) => x.id === draftId);
    if (!d) throw new Error("draft not found");
    return d;
  };

  return {
    formats: async (_sort, includeArchived) => formats.filter((f) => includeArchived || f.status === "active"),
    createFormat: async (bp) => {
      const f = record(bp, { origin: "manual", observation_count: 0, observed_win_rate: null, sources: [], example_texts: [], recent_sightings: 0 });
      formats = [f, ...formats];
      return f;
    },
    updateFormat: async (fid, bp) => {
      const f = formats.find((x) => x.id === fid);
      if (!f) throw new Error("format not found");
      const next = { ...f, ...bp, updated_at: now() };
      formats = formats.map((x) => (x.id === fid ? next : x));
      return next;
    },
    setFormatStatus: async (fid, status) => {
      formats = formats.map((x) => (x.id === fid ? { ...x, status } : x));
    },
    candidates: async (status = "pending") => candidates.filter((c) => c.status === status),
    dismissCandidate: async (cid) => {
      candidates = candidates.map((c) => (c.id === cid ? { ...c, status: "dismissed" } : c));
    },
    extract: async () => {
      throw new Error('No model is chosen for "format_extraction". Pick one in Settings > Model routing once the model router is connected; until then you can still build and edit formats by hand.');
    },
    extractPending: async () => ({
      created: 0,
      merged: 0,
      failed: 0,
      stopped_reason: 'No model is chosen for "format_extraction". Pick one in Settings > Model routing once the model router is connected; until then you can still build and edit formats by hand.',
    }),
    addPost: async (post) => {
      const m = post.url.match(/(?:x|twitter)\.com\/([A-Za-z0-9_]+)\/status\/(\d+)/);
      if (!m) throw new Error("paste a post link like https://x.com/name/status/123456789");
      const c: FormatCandidate = {
        id: id("cand"),
        platform_post_id: m[2],
        author_handle: m[1],
        author_followers: post.author_followers,
        text: post.text,
        metrics: { likes: post.likes, reposts: post.reposts, replies: post.replies, bookmarks: post.bookmarks, views: post.views },
        posted_at: null,
        source: "manual",
        virality_score: 0,
        reasons: ["Added by hand."],
        status: "pending",
        format_id: null,
        error_message: null,
        created_at: now(),
      };
      candidates = [c, ...candidates];
      return { verdict: { flagged: false, score: 0, reasons: c.reasons }, candidate: c, already_known: false };
    },
    thresholds: async () => thresholds,
    setThresholds: async (t) => (thresholds = t),
    watchlist: async () => watch,
    watchAdd: async (handle, followers) => {
      const h = handle.trim().replace(/^@+/, "");
      if (!/^[A-Za-z0-9_]{1,15}$/.test(h)) throw new Error(`"${handle}" is not a valid X handle (1-15 letters, digits or underscores)`);
      if (watch.some((w) => w.handle.toLowerCase() === h.toLowerCase())) throw new Error(`@${h} is already on the watchlist`);
      const w: WatchEntry = { id: id("w"), handle: h, followers, added_at: now(), last_scanned_at: null };
      watch = [...watch, w];
      return w;
    },
    watchRemove: async (wid) => {
      watch = watch.filter((w) => w.id !== wid);
    },
    watchScan: async () => {
      throw new Error("Browser control is only available in the desktop app.");
    },
    settings: async () => settings,
    saveSettings: async (s) => (settings = s),
    matchFormats: async () => formats.filter((f) => f.status === "active").map((f) => ({ format_id: f.id, score: 0.5 })),
    generate: async (formatId, topic) => {
      if (!topic.trim()) throw new Error("write a rough thought or topic first");
      const batch = id("batch");
      const texts = [
        `Unpopular opinion: ${topic.trim()} is overrated.\n\nMost people copy it without asking what it costs.\n\nPick the version that fits your week.`,
        `Hot take: ${topic.trim()} won't fix your real problem.\n\nThe real problem is what you skip to do it.`,
        `Everyone says ${topic.trim()} works. I tested it for a month and it mostly didn't.`,
      ];
      const created = texts.map((t) => ({ ...draft(t, "variant", formatId, batch, topic), similarity: 0.12 }));
      drafts = [...created, ...drafts];
      return { batch_id: batch, drafts: created };
    },
    createBlank: async (formatId, text) => {
      const d = draft(text, "draft", formatId, null, "");
      drafts = [d, ...drafts];
      return d;
    },
    drafts: async () => drafts.filter((d) => d.status !== "discarded"),
    edit: async (did, text) => {
      const d = find(did);
      return replaceDraft({ ...d, edited_text: text, status: "draft", edit_distance: Math.abs(text.length - d.initial_text.length) });
    },
    setMedia: async (did, paths) => replaceDraft({ ...find(did), media_paths: paths }),
    discard: async (did) => {
      replaceDraft({ ...find(did), status: "discarded" });
    },
    reopen: async (did) => replaceDraft({ ...find(did), status: "draft", error_message: null }),
    publish: async (did, expected) => {
      const d = find(did);
      if (d.edited_text !== expected) throw new Error("the draft changed since you last looked at it; review it and publish again");
      return replaceDraft({ ...d, status: "failed", error_message: "Browser control is only available in the desktop app, so nothing was posted." });
    },
  };
}
