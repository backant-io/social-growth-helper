import type { WidgetQueryResult, WidgetSpec } from "../types";

const SERIES_COLORS = [
  "var(--fg)",
  "var(--fg-faint)",
  "var(--line-strong)",
  "var(--fg-muted)",
  "var(--bg-inverse)",
  "var(--line)",
];

export function formatMetricValue(value: unknown, format?: string | null): string {
  if (value === null || value === undefined || value === "") return "—";
  const num = Number(value);
  if (Number.isNaN(num)) return String(value);
  if (format === "percent") return `${num.toFixed(2)}%`;
  if (format === "signed") {
    const sign = num > 0 ? "+" : "";
    return `${sign}${formatCompact(num)}`;
  }
  return formatCompact(num);
}

export function formatCompact(num: number): string {
  const abs = Math.abs(num);
  if (abs >= 1_000_000) return `${(num / 1_000_000).toFixed(1)}M`;
  if (abs >= 10_000) return `${(num / 1_000).toFixed(1)}K`;
  if (Number.isInteger(num)) return num.toLocaleString();
  return num.toLocaleString(undefined, { maximumFractionDigits: 2 });
}

function humanize(name: string): string {
  return name.replace(/^avg_/, "").replace(/_/g, " ");
}

function sentenceCase(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function kindLabel(kind: string): string {
  if (kind === "ogc") return "Post";
  return sentenceCase(kind);
}

function asNumber(v: unknown): number {
  const n = Number(v ?? 0);
  return Number.isFinite(n) ? n : 0;
}

function pickField(
  preferred: string | null | undefined,
  columns: string[],
  fallbackIndex: number,
): string {
  if (preferred && columns.includes(preferred)) return preferred;
  return columns[fallbackIndex] ?? columns[0] ?? "";
}

function EmptyChart({ message = "No data in this time window yet." }: { message?: string }) {
  return (
    <p className="py-6 text-sm text-zinc-500">{message}</p>
  );
}

function KpiChart({ spec, data }: { spec: WidgetSpec; data: WidgetQueryResult }) {
  const row = data.rows[0];
  const valueField = pickField(spec.options.value_field, data.columns, data.columns.length - 1);
  const raw = row ? row[valueField] : null;
  return (
    <div>
      <div className="font-display text-3xl font-semibold tracking-tight text-zinc-950 dark:text-zinc-50">
        {formatMetricValue(raw, spec.options.format)}
      </div>
    </div>
  );
}

function LineChart({ spec, data }: { spec: WidgetSpec; data: WidgetQueryResult }) {
  if (data.rows.length === 0) return <EmptyChart />;
  const xField = pickField(spec.options.x_field, data.columns, 0);
  const yField = pickField(spec.options.y_field, data.columns, data.columns.length - 1);
  const seriesField =
    spec.options.series_field && data.columns.includes(spec.options.series_field)
      ? spec.options.series_field
      : null;

  const xValues = Array.from(new Set(data.rows.map((r) => String(r[xField] ?? ""))));
  const seriesNames = seriesField
    ? Array.from(new Set(data.rows.map((r) => String(r[seriesField] ?? "All"))))
    : ["Value"];

  const lookup = new Map<string, number>();
  let maxVal = 0;
  for (const r of data.rows) {
    const x = String(r[xField] ?? "");
    const s = seriesField ? String(r[seriesField] ?? "All") : "Value";
    const y = asNumber(r[yField]);
    const key = `${s}\0${x}`;
    const next = (lookup.get(key) ?? 0) + y;
    lookup.set(key, next);
    if (next > maxVal) maxVal = next;
  }
  const yMax = maxVal > 0 ? maxVal : 1;

  const width = 560;
  const height = 190;
  const padL = 44;
  const padR = 14;
  const padT = 14;
  const padB = 26;
  const plotW = width - padL - padR;
  const plotH = height - padT - padB;

  const xPos = (i: number) =>
    xValues.length <= 1 ? padL + plotW / 2 : padL + (i / (xValues.length - 1)) * plotW;
  const yPos = (v: number) => padT + plotH - (v / yMax) * plotH;

  return (
    <div className="space-y-2">
      <svg viewBox={`0 0 ${width} ${height}`} className="w-full h-48 overflow-visible" role="img" aria-label={spec.title}>
        {[0, 0.5, 1].map((t) => {
          const y = padT + plotH * (1 - t);
          return (
            <g key={t}>
              <line x1={padL} x2={width - padR} y1={y} y2={y} className="stroke-zinc-200 dark:stroke-zinc-800" />
              <text x={padL - 6} y={y + 3} textAnchor="end" className="fill-zinc-400 text-[10px]">
                {formatCompact(yMax * t)}
              </text>
            </g>
          );
        })}

        {seriesNames.map((series, sIdx) => {
          const color = SERIES_COLORS[sIdx % SERIES_COLORS.length];
          const points = xValues
            .map((x, i) => ({ x, i, val: lookup.get(`${series}\0${x}`) ?? 0 }))
            .map((p) => `${xPos(p.i)},${yPos(p.val)}`);
          return (
            <g key={series}>
              {points.length > 1 && (
                <polyline
                  fill="none"
                  stroke={color}
                  strokeWidth={1.5}
                  strokeLinejoin="round"
                  strokeLinecap="round"
                  points={points.join(" ")}
                />
              )}
              {xValues.map((x, i) => {
                const val = lookup.get(`${series}\0${x}`) ?? 0;
                const isLast = i === xValues.length - 1;
                return (
                  <g key={x}>
                    {isLast && <circle cx={xPos(i)} cy={yPos(val)} r={2.5} fill={color} />}
                    <circle cx={xPos(i)} cy={yPos(val)} r={8} fill="transparent">
                      <title>{`${series} • ${x}: ${formatMetricValue(val, spec.options.format)}`}</title>
                    </circle>
                  </g>
                );
              })}
            </g>
          );
        })}

        {xValues.map((x, i) => {
          const step = Math.max(1, Math.ceil(xValues.length / 6));
          if (i % step !== 0 && i !== xValues.length - 1) return null;
          return (
            <text key={x} x={xPos(i)} y={height - 6} textAnchor="middle" className="fill-zinc-400 text-[10px]">
              {x.length > 10 ? x.slice(5) : x}
            </text>
          );
        })}
      </svg>

      {seriesNames.length > 1 && (
        <div className="flex flex-wrap gap-3 text-xs text-zinc-600 dark:text-zinc-400">
          {seriesNames.map((s, i) => (
            <span key={s} className="inline-flex items-center gap-1.5">
              <span className="h-0.5 w-3" style={{ backgroundColor: SERIES_COLORS[i % SERIES_COLORS.length] }} />
              {s}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function BarChart({ spec, data }: { spec: WidgetSpec; data: WidgetQueryResult }) {
  if (data.rows.length === 0) return <EmptyChart />;
  const xField = pickField(spec.options.x_field, data.columns, 0);
  const yField = pickField(spec.options.y_field, data.columns, 1);
  const extraMetrics = data.columns.filter((c) => c !== xField && c !== yField).slice(0, 3);

  const items = data.rows.map((r) => ({
    label: String(r[xField] ?? "Unknown"),
    value: asNumber(r[yField]),
    raw: r,
  }));
  const maxVal = Math.max(1, ...items.map((i) => i.value));

  return (
    <ul className="space-y-4">
      {items.map((item, idx) => {
        const pct = Math.max(2, Math.round((item.value / maxVal) * 100));
        const extras = extraMetrics.map((m) => `${humanize(m)} ${formatCompact(asNumber(item.raw[m]))}`);
        return (
          <li key={`${item.label}-${idx}`} className="space-y-1.5">
            <div className="flex items-baseline justify-between gap-3 text-sm">
              <span className="min-w-0 truncate">{item.label}</span>
              <span className="shrink-0 font-mono tabular-nums">
                {formatMetricValue(item.value, spec.options.format)}
              </span>
            </div>
            <div className="h-1 w-full overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800">
              <div className="h-full rounded-full bg-zinc-950 dark:bg-zinc-100" style={{ width: `${pct}%` }} />
            </div>
            {extras.length > 0 && <p className="text-xs text-zinc-500">{extras.join(" · ")}</p>}
          </li>
        );
      })}
    </ul>
  );
}

function StackedBarChart({ spec, data }: { spec: WidgetSpec; data: WidgetQueryResult }) {
  if (data.rows.length === 0) return <EmptyChart />;
  const xField = pickField(spec.options.x_field, data.columns, 0);
  const seriesField = pickField(spec.options.series_field, data.columns, 1);
  const yField = pickField(spec.options.y_field, data.columns, data.columns.length - 1);

  const xValues = Array.from(new Set(data.rows.map((r) => String(r[xField] ?? ""))));
  const seriesNames = Array.from(new Set(data.rows.map((r) => String(r[seriesField] ?? "All"))));

  const stacks = xValues.map((x) => {
    const parts = seriesNames.map((s) => {
      const sum = data.rows
        .filter((r) => String(r[xField] ?? "") === x && String(r[seriesField] ?? "All") === s)
        .reduce((acc, r) => acc + asNumber(r[yField]), 0);
      return { series: s, value: sum };
    });
    const total = parts.reduce((acc, p) => acc + p.value, 0);
    return { x, parts, total };
  });
  const maxTotal = Math.max(1, ...stacks.map((s) => s.total));

  return (
    <div className="space-y-3">
      <div className="space-y-2">
        {stacks.map((stack) => (
          <div key={stack.x} className="space-y-1">
            <div className="flex justify-between text-xs">
              <span className="text-zinc-700 dark:text-zinc-300">{stack.x}</span>
              <span className="font-mono text-zinc-500">{formatMetricValue(stack.total, spec.options.format)}</span>
            </div>
            <div className="flex h-1.5 w-full overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800">
              {stack.parts.map((p, idx) => {
                const widthPct = (p.value / maxTotal) * 100;
                if (widthPct <= 0) return null;
                return (
                  <div
                    key={p.series}
                    style={{ width: `${widthPct}%`, backgroundColor: SERIES_COLORS[idx % SERIES_COLORS.length] }}
                    title={`${stack.x} • ${p.series}: ${formatMetricValue(p.value, spec.options.format)}`}
                  />
                );
              })}
            </div>
          </div>
        ))}
      </div>
      <div className="flex flex-wrap gap-3 text-xs text-zinc-500">
        {seriesNames.map((s, idx) => (
          <span key={s} className="inline-flex items-center gap-1.5">
            <span className="h-0.5 w-3" style={{ backgroundColor: SERIES_COLORS[idx % SERIES_COLORS.length] }} />
            {s}
          </span>
        ))}
      </div>
    </div>
  );
}

function HeatmapChart({ spec, data }: { spec: WidgetSpec; data: WidgetQueryResult }) {
  if (data.rows.length === 0) return <EmptyChart />;
  const xField = pickField(spec.options.x_field, data.columns, 0);
  const yField = pickField(spec.options.y_field, data.columns, 1);
  const valueField = pickField(spec.options.value_field, data.columns, 2);

  const timingOrder = ["<15m", "15-60m", "1-3h", ">3h", "unknown"];
  const tierOrder = ["100k+", "10k-100k", "1k-10k", "<1k", "unknown"];

  const sortByOrder = (vals: string[], order: string[]) =>
    vals.sort((a, b) => {
      const ia = order.indexOf(a);
      const ib = order.indexOf(b);
      if (ia === -1 && ib === -1) return a.localeCompare(b);
      if (ia === -1) return 1;
      if (ib === -1) return -1;
      return ia - ib;
    });

  const xs = sortByOrder(Array.from(new Set(data.rows.map((r) => String(r[xField] ?? "unknown")))), timingOrder);
  const ys = sortByOrder(Array.from(new Set(data.rows.map((r) => String(r[yField] ?? "unknown")))), tierOrder);

  const grid = new Map<string, { value: number; posts?: number }>();
  let maxVal = 0;
  for (const r of data.rows) {
    const key = `${String(r[yField] ?? "unknown")}\0${String(r[xField] ?? "unknown")}`;
    const val = asNumber(r[valueField]);
    if (val > maxVal) maxVal = val;
    grid.set(key, { value: val, posts: r.posts !== undefined ? asNumber(r.posts) : undefined });
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full border-separate border-spacing-1 text-xs">
        <thead>
          <tr>
            <th className="text-left font-normal text-zinc-400 px-1.5 py-1">{yField.replace(/_/g, " ")}</th>
            {xs.map((x) => (
              <th key={x} className="font-medium text-zinc-600 dark:text-zinc-300 px-1.5 py-1 text-center">
                {x}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {ys.map((y) => (
            <tr key={y}>
              <td className="font-medium text-zinc-600 dark:text-zinc-300 px-1.5 py-1 whitespace-nowrap">{y}</td>
              {xs.map((x) => {
                const cell = grid.get(`${y}\0${x}`);
                const ratio = cell && maxVal > 0 ? cell.value / maxVal : 0;
                const alpha = cell ? Math.max(0.06, Math.min(0.85, ratio)) : 0;
                return (
                  <td
                    key={x}
                    className="rounded px-2 py-2 text-center font-mono transition-colors"
                    style={
                      cell
                        ? {
                            backgroundColor: `color-mix(in srgb, var(--fg) ${Math.round(alpha * 100)}%, transparent)`,
                            color: ratio > 0.55 ? "var(--bg)" : undefined,
                          }
                        : undefined
                    }
                    title={cell ? `${y} × ${x}: ${formatCompact(cell.value)}${cell.posts ? ` (${cell.posts} posts)` : ""}` : "No posts"}
                  >
                    {cell ? (
                      cell.value === 0 && (cell.posts ?? 0) > 0 ? (
                        `${cell.posts}p`
                      ) : (
                        formatCompact(cell.value)
                      )
                    ) : (
                      <span className="text-zinc-300 dark:text-zinc-700">—</span>
                    )}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ScatterChart({ spec, data }: { spec: WidgetSpec; data: WidgetQueryResult }) {
  if (data.rows.length === 0) return <EmptyChart />;
  const xField = pickField(spec.options.x_field, data.columns, 0);
  const yField = pickField(spec.options.y_field, data.columns, 1);
  const seriesField =
    spec.options.series_field && data.columns.includes(spec.options.series_field)
      ? spec.options.series_field
      : null;

  const seriesNames = seriesField
    ? Array.from(new Set(data.rows.map((r) => String(r[seriesField] ?? "All"))))
    : ["All"];
  const maxX = Math.max(1, ...data.rows.map((r) => asNumber(r[xField])));
  const maxY = Math.max(1, ...data.rows.map((r) => asNumber(r[yField])));

  const width = 520;
  const height = 190;
  const padL = 46;
  const padR = 16;
  const padT = 14;
  const padB = 26;
  const plotW = width - padL - padR;
  const plotH = height - padT - padB;

  return (
    <div className="space-y-2">
      <svg viewBox={`0 0 ${width} ${height}`} className="w-full h-48 overflow-visible" role="img" aria-label={spec.title}>
        <line x1={padL} x2={padL} y1={padT} y2={padT + plotH} className="stroke-zinc-300 dark:stroke-zinc-700" />
        <line x1={padL} x2={width - padR} y1={padT + plotH} y2={padT + plotH} className="stroke-zinc-300 dark:stroke-zinc-700" />
        <text x={padL - 6} y={padT + 4} textAnchor="end" className="fill-zinc-400 text-[10px]">
          {formatCompact(maxY)}
        </text>
        <text x={padL - 6} y={padT + plotH} textAnchor="end" className="fill-zinc-400 text-[10px]">
          0
        </text>
        <text x={width - padR} y={height - 6} textAnchor="end" className="fill-zinc-400 text-[10px]">
          {xField}: {formatCompact(maxX)}
        </text>
        {data.rows.map((r, idx) => {
          const xVal = asNumber(r[xField]);
          const yVal = asNumber(r[yField]);
          const series = seriesField ? String(r[seriesField] ?? "All") : "All";
          const color = SERIES_COLORS[Math.max(0, seriesNames.indexOf(series)) % SERIES_COLORS.length];
          const cx = padL + (xVal / maxX) * plotW;
          const cy = padT + plotH - (yVal / maxY) * plotH;
          return (
            <circle key={idx} cx={cx} cy={cy} r={4.5} fill={color} fillOpacity={0.82}>
              <title>{`${series} • ${xField}=${formatCompact(xVal)}, ${yField}=${formatCompact(yVal)}`}</title>
            </circle>
          );
        })}
      </svg>
      {seriesNames.length > 1 && (
        <div className="flex flex-wrap gap-3 text-xs text-zinc-500">
          {seriesNames.map((s, idx) => (
            <span key={s} className="inline-flex items-center gap-1.5">
              <span className="h-2.5 w-2.5 rounded-full" style={{ backgroundColor: SERIES_COLORS[idx % SERIES_COLORS.length] }} />
              {s}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function FunnelChart({ spec, data }: { spec: WidgetSpec; data: WidgetQueryResult }) {
  if (data.rows.length === 0) return <EmptyChart />;
  const xField = pickField(spec.options.x_field, data.columns, 0);
  const valueField = pickField(spec.options.value_field ?? spec.options.y_field, data.columns, 1);
  const items = data.rows.map((r) => ({
    label: String(r[xField] ?? "Stage"),
    value: asNumber(r[valueField]),
  }));
  const top = Math.max(1, items[0]?.value ?? 1);

  return (
    <div className="space-y-2">
      {items.map((item, idx) => {
        const pct = Math.max(12, Math.round((item.value / top) * 100));
        return (
          <div key={`${item.label}-${idx}`} className="flex flex-col items-center gap-1">
            <div
              className="flex items-center justify-between rounded-md px-3 py-1.5 text-xs text-white transition-all"
              style={{
                width: `${pct}%`,
                backgroundColor: SERIES_COLORS[idx % SERIES_COLORS.length],
              }}
            >
              <span className="font-medium truncate">{item.label}</span>
              <span className="font-mono ml-2 shrink-0">{formatMetricValue(item.value, spec.options.format)}</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}

function WinnersFlopsGallery({ data }: { data: WidgetQueryResult }) {
  if (data.rows.length === 0) return <EmptyChart message="Posts you publish show up here once their numbers sync." />;
  const winners = data.rows.filter((r) => String(r.tier) === "winner");
  const flops = data.rows.filter((r) => String(r.tier) === "flop");

  const renderRow = (row: Record<string, unknown>, idx: number) => {
    const isExternal = Number(row.is_external ?? 0) === 1 || String(row.draft_kind) === "external";
    const meta = [
      isExternal ? "External" : kindLabel(String(row.draft_kind ?? "post")),
      String(row.model_family ?? ""),
      `${formatCompact(asNumber(row.impressions))} impressions`,
      `${asNumber(row.engagement_rate).toFixed(2)}% engagement`,
    ].filter(Boolean);
    return (
      <li key={idx} className="space-y-1 py-3">
        <p className="whitespace-pre-wrap text-sm leading-relaxed">
          {String(row.post_text ?? "(no post text captured)")}
        </p>
        {row.context_text ? (
          <p className="text-xs text-zinc-500">
            Replying to @{String(row.context_author ?? "author")}: {String(row.context_text)}
          </p>
        ) : null}
        <p className="text-xs text-zinc-500">{meta.join(" · ")}</p>
      </li>
    );
  };

  const group = (label: string, rows: Array<Record<string, unknown>>, empty: string) => (
    <div>
      <h4 className="text-sm font-medium">{label}</h4>
      {rows.length === 0 ? (
        <p className="py-3 text-sm text-zinc-500">{empty}</p>
      ) : (
        <ul className="mt-1 divide-y divide-zinc-200 border-t border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
          {rows.map(renderRow)}
        </ul>
      )}
    </div>
  );

  return (
    <div className="grid gap-x-10 gap-y-6 md:grid-cols-2">
      {group("Top 10%", winners, "Nothing yet.")}
      {group("Bottom 10%", flops, "Nothing yet.")}
    </div>
  );
}

function TableChart({ spec, data }: { spec: WidgetSpec; data: WidgetQueryResult }) {
  if (spec.options.gallery_mode && data.columns.includes("tier") && data.columns.includes("post_text")) {
    return <WinnersFlopsGallery data={data} />;
  }
  if (data.rows.length === 0) return <EmptyChart />;
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-left text-sm">
        <thead>
          <tr className="border-b border-zinc-200 text-zinc-500 dark:border-zinc-800">
            {data.columns.map((c) => (
              <th key={c} className="py-2 pr-4 font-normal whitespace-nowrap">
                {sentenceCase(humanize(c))}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-200 dark:divide-zinc-800">
          {data.rows.map((row, idx) => (
            <tr key={idx}>
              {data.columns.map((c) => {
                const v = row[c];
                const isNum = typeof v === "number";
                return (
                  <td
                    key={c}
                    className={`py-2.5 pr-4 max-w-xs truncate ${
                      isNum ? "font-mono text-zinc-800 dark:text-zinc-200" : "text-zinc-700 dark:text-zinc-300"
                    }`}
                  >
                    {c === "is_external" ? (
                      Number(v) === 1 ? "External" : "—"
                    ) : isNum ? (
                      formatCompact(v)
                    ) : (
                      String(v ?? "—")
                    )}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function WidgetChartRenderer({ spec, data }: { spec: WidgetSpec; data: WidgetQueryResult }) {
  switch (spec.chart_type) {
    case "kpi":
      return <KpiChart spec={spec} data={data} />;
    case "line":
      return <LineChart spec={spec} data={data} />;
    case "bar":
      return <BarChart spec={spec} data={data} />;
    case "stacked_bar":
      return <StackedBarChart spec={spec} data={data} />;
    case "heatmap":
      return <HeatmapChart spec={spec} data={data} />;
    case "scatter":
      return <ScatterChart spec={spec} data={data} />;
    case "funnel":
      return <FunnelChart spec={spec} data={data} />;
    case "table":
    default:
      return <TableChart spec={spec} data={data} />;
  }
}
