import { lineDiff } from "../learning/textdiff";
import type { DiffLine } from "../learning/types";
import type { StyleGuide } from "./types";

type GuideFields = Pick<StyleGuide, "free_text_guide" | "learned_rules" | "banned_patterns" | "max_reply_chars" | "emoji_policy" | "hashtag_policy">;

/** One comparable line per piece of a style guide version. */
export function styleGuideLines(g: GuideFields): string[] {
  return [
    ...g.free_text_guide.split("\n").map((l) => l.trimEnd()).filter((l) => l.trim().length > 0),
    ...g.learned_rules.map((r) => `rule: ${r}`),
    ...g.banned_patterns.map((b) => `banned: ${b}`),
    `max reply chars: ${g.max_reply_chars}`,
    `emoji: ${g.emoji_policy}`,
    `hashtags: ${g.hashtag_policy}`,
  ];
}

export function styleVersionDiff(previous: GuideFields | null, current: GuideFields): DiffLine[] {
  return lineDiff(previous ? styleGuideLines(previous) : [], styleGuideLines(current));
}

/** The version saved just before `version`, or null for the oldest one. `versions` may be in any order. */
export function previousVersion<T extends { version: number }>(versions: T[], version: number): T | null {
  return versions.filter((v) => v.version < version).sort((a, b) => b.version - a.version)[0] ?? null;
}
