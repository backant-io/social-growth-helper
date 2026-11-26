/** Splits free-form input (commas, semicolons, newlines) into trimmed, de-duplicated, non-empty items. */
export function parseList(input: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of input.split(/[,;\n]/)) {
    const item = raw.trim();
    if (!item) continue;
    const key = item.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(item);
  }
  return out;
}

export function joinList(items: string[]): string {
  return items.join(", ");
}

export function stripHandle(h: string): string {
  return h.trim().replace(/^@+/, "");
}
