import type { WidgetQueryResult } from "./types";

function escapeCsvCell(raw: string): string {
  const first = raw[0];
  const needsFormulaGuard =
    first === "=" ||
    first === "+" ||
    first === "@" ||
    first === "\t" ||
    first === "\r" ||
    (first === "-" && raw.length > 1 && !/^\d+(\.\d+)?$/.test(raw.slice(1)));
  const safe = needsFormulaGuard ? `'${raw}` : raw;
  if (/[,"\n\r]/.test(safe)) {
    return `"${safe.replace(/"/g, '""')}"`;
  }
  return safe;
}

function formatCell(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

export function serializeWidgetCsv(result: WidgetQueryResult): string {
  const header = result.columns.map(escapeCsvCell).join(",");
  const rows = result.rows.map((row) =>
    result.columns.map((col) => escapeCsvCell(formatCell(row[col]))).join(","),
  );
  return [header, ...rows].join("\r\n");
}

export function downloadCsv(filename: string, csvContent: string): void {
  const safeName = filename.replace(/[^a-zA-Z0-9_-]+/g, "-").replace(/^-+|-+$/g, "") || "analytics-export";
  const blob = new Blob([csvContent], { type: "text/csv;charset=utf-8;" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `${safeName}.csv`;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
}
