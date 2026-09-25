import { Fragment, useLayoutEffect, useRef, useState } from "react";
import { Area, AreaChart, Bar, BarChart, Label, Pie, PieChart, type LabelProps } from "recharts";
import { useTranslation } from "react-i18next";
import { cn } from "cn";

import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { compact } from "@/lib/format";

const PALETTE = ["var(--chart-1)", "var(--chart-2)", "var(--chart-3)", "var(--chart-4)", "var(--chart-5)"];

export const color = (i: number) => PALETTE[i % PALETTE.length];

/**
 * Call outcomes → the same tokens the status icons use (`text-emerald-500`, `text-destructive`, `text-muted-foreground`),
 * so a ring segment and its legend icon always agree. Never indexed by palette position.
 */
export const OUTCOME_COLORS = {
  ok: "var(--color-emerald-500)",
  failed: "var(--destructive)",
  cancelled: "var(--muted-foreground)",
} as const;

export type Outcome = keyof typeof OUTCOME_COLORS;

/** Tailwind text sizes (class, px) for a ring's big line, largest first; a long value steps down this ladder. */
const BIG_STEPS = [
  ["text-3xl", 30],
  ["text-2xl", 24],
  ["text-xl", 20],
  ["text-lg", 18],
  ["text-base", 16],
] as const;
const CAPTION_PX = 12; // text-xs

/**
 * Two centered lines inside a ring's hole: `value` (semibold, `size` = `3xl` for the gauge, `xl` for donuts) over
 * `caption` (`text-xs`, muted). The value stays at its base size for the common short strings (`80%`, `294.2K`) and steps
 * down only for genuinely long ones: one step past 6 characters (`999.99M`), two past 8 (`12345.67B`).
 */
function RingLabel({ viewBox, value, caption, size }: { viewBox: LabelProps["viewBox"]; value: string; caption: string; size: "3xl" | "2xl" | "xl" }) {
  const polar = viewBox && "cx" in viewBox ? viewBox : undefined;
  const base = BIG_STEPS.findIndex(([cls]) => cls === `text-${size}`);
  const [, stepPx] = BIG_STEPS[Math.min(BIG_STEPS.length - 1, base + (value.length > 8 ? 2 : value.length > 6 ? 1 : 0))];
  // Fonts differ in width across platforms, so the value is measured and shrunk until it sits within 70% of the hole.
  const valueRef = useRef<SVGTSpanElement>(null);
  const [fitPx, setFitPx] = useState<number | null>(null);
  const maxWidth = (polar?.innerRadius ?? 0) * 2 * 0.7;
  useLayoutEffect(() => {
    const el = valueRef.current;
    if (!el || maxWidth <= 0) return;
    el.style.fontSize = `${stepPx}px`;
    const width = el.getComputedTextLength();
    setFitPx(width > maxWidth ? Math.floor((stepPx * maxWidth) / width) : null);
  }, [value, stepPx, maxWidth]);
  if (!polar) return null;
  const cx = polar.cx ?? 0;
  const cy = polar.cy ?? 0;
  const bigPx = fitPx ?? stepPx;
  const gap = caption ? 4 : 0;
  // Explicit alphabetic baselines instead of `dominant-baseline`: WebKit (macOS) doesn't pass it on to
  // `<tspan>`s the way Chromium does. Digits and caps are ~0.7em tall, so a baseline 0.35em below a line's
  // middle centres it.
  const valueMid = caption ? cy - (CAPTION_PX + gap) / 2 : cy;
  const captionMid = cy + (bigPx + gap) / 2;
  return (
    <text x={cx} y={cy} textAnchor="middle">
      <tspan ref={valueRef} x={cx} y={valueMid + bigPx * 0.35} style={{ fontSize: bigPx }} className="fill-foreground font-semibold tabular-nums">
        {value}
      </tspan>
      {caption && (
        <tspan x={cx} y={captionMid + CAPTION_PX * 0.35} className="fill-muted-foreground text-xs">
          {caption}
        </tspan>
      )}
    </text>
  );
}

/** A tiny trend line for KPI cards. `className` sets the size (default 7rem × 3rem). */
export function Sparkline({ data, dataKey, kind = "area", index = 0, className }: { data: object[]; dataKey: string; kind?: "area" | "bar"; index?: number; className?: string }) {
  const config = { [dataKey]: { label: dataKey, color: color(index) } } satisfies ChartConfig;
  const id = `spark-${dataKey}-${index}`;
  return (
    <ChartContainer config={config} className={cn("aspect-auto h-12 w-28", className)}>
      {kind === "bar" ? (
        <BarChart data={data} margin={{ top: 2, bottom: 0, left: 0, right: 0 }}>
          <Bar dataKey={dataKey} fill={`var(--color-${dataKey})`} radius={2} />
        </BarChart>
      ) : (
        <AreaChart data={data} margin={{ top: 6, bottom: 2, left: 2, right: 6 }}>
          <defs>
            <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={`var(--color-${dataKey})`} stopOpacity={0.45} />
              <stop offset="100%" stopColor={`var(--color-${dataKey})`} stopOpacity={0} />
            </linearGradient>
          </defs>
          <Area dataKey={dataKey} type="basis" stroke={`var(--color-${dataKey})`} strokeWidth={1.5} strokeLinecap="round" strokeLinejoin="round" fill={`url(#${id})`} isAnimationActive={false} />
        </AreaChart>
      )}
    </ChartContainer>
  );
}

const GAUGE_CONFIG = { ok: { color: OUTCOME_COLORS.ok }, failed: { color: OUTCOME_COLORS.failed }, cancelled: { color: OUTCOME_COLORS.cancelled } } satisfies ChartConfig;

/**
 * A ring split by call outcome — ok (green), failed (red), cancelled (grey), clockwise from the top — with the success
 * rate and the call count in the middle. A full muted track while loading (`counts` undefined) or with no calls.
 * `className` sets the size (default 11rem square).
 */
export function Gauge({ counts, className }: { counts?: Record<Outcome, number>; className?: string }) {
  const { t } = useTranslation();
  const total = counts ? counts.ok + counts.failed + counts.cancelled : 0;
  const rows =
    counts && total > 0
      ? (Object.keys(OUTCOME_COLORS) as Outcome[]).filter((k) => counts[k] > 0).map((k) => ({ name: k, value: counts[k], fill: `var(--color-${k})` }))
      : [{ name: "track", value: 1, fill: "var(--muted)" }];
  const split = rows.length > 1;
  return (
    <ChartContainer config={GAUGE_CONFIG} className={cn("mx-auto aspect-square h-44", className)}>
      <PieChart>
        {/* Ring 20% of the radius thick (100% − 80%), i.e. 10% of its diameter; the 80% hole fits `100%` + `12.3K calls` at full size. */}
        <Pie data={rows} dataKey="value" nameKey="name" startAngle={90} endAngle={-270} innerRadius="80%" outerRadius="100%" stroke="none" paddingAngle={split ? 2 : 0} cornerRadius={split ? 99 : 0}>
          <Label content={({ viewBox }) => <RingLabel viewBox={viewBox} value={counts && total > 0 ? `${Math.round((counts.ok / total) * 100)}%` : "—"} caption={counts ? t("common.calls", { count: total }) : ""} size="3xl" />} />
        </Pie>
      </PieChart>
    </ChartContainer>
  );
}

/** A donut of shares, the total in the middle. `className` sets the size (default 11rem square). */
export function Donut({ data, unit, className }: { data: { name: string; value: number }[]; unit: string; className?: string }) {
  const config: ChartConfig = Object.fromEntries(data.map((d, i) => [d.name, { label: d.name, color: color(i) }]));
  const total = data.reduce((sum, d) => sum + d.value, 0);
  const shares = data.map((d, i) => ({ ...d, fill: color(i) })).filter((d) => d.value > 0);
  const rows = shares.length ? shares : [{ name: "track", value: 1, fill: "var(--muted)" }];
  // Same look as the Gauge: rounded segments with a small gap, a seamless ring when there is only one.
  const split = rows.length > 1;
  return (
    <ChartContainer config={config} className={cn("mx-auto aspect-square h-44", className)}>
      {/* No chart margin: the ring stops at 92% of the radius anyway, and the dashboard renders this small (~134px). */}
      <PieChart margin={{ top: 0, right: 0, bottom: 0, left: 0 }}>
        {shares.length > 0 && <ChartTooltip content={<ChartTooltipContent hideLabel nameKey="name" />} />}
        {/*
         * Ring 16% of the box radius thick (92% − 76%); the 76% hole is ~102px on a 134px box, so a six-character
         * `999.9K` at text-xl (≈0.6em/char → 72px) takes ≤ 70% of it.
         */}
        <Pie
          data={rows}
          dataKey="value"
          nameKey="name"
          startAngle={90}
          endAngle={-270}
          innerRadius="76%"
          outerRadius="92%"
          stroke="none"
          paddingAngle={split ? 2 : 0}
          cornerRadius={split ? 99 : 0}
        >
          <Label content={({ viewBox }) => <RingLabel viewBox={viewBox} value={compact(total)} caption={unit} size="xl" />} />
        </Pie>
      </PieChart>
    </ChartContainer>
  );
}

/**
 * Horizontal bars, one per row, largest first: label | track | value, each row 1.5rem tall, starting at the top.
 * Plain DOM (not recharts) so the label column lines up with the container's left edge and rows have no dead space.
 * `id` (when it differs from the display `name`) keys the row and joins the tooltip, so two rows may share a name.
 */
export function RankBars({ data, formatter = compact, className }: { data: { name: string; id?: string; value: number }[]; formatter?: (n: number) => string; className?: string }) {
  const { t } = useTranslation();
  const max = Math.max(1, ...data.map((d) => d.value));
  return (
    <div className={cn("grid auto-rows-[1.5rem] grid-cols-[minmax(0,3fr)_minmax(0,2fr)_auto] content-start items-center gap-x-3 text-xs", className)}>
      {data.map((d) => {
        const who = d.id && d.id !== d.name ? `${d.name} (${d.id})` : d.name;
        const title = `${who} · ${formatter(d.value)} ${t("common.tokens")}`;
        return (
          <Fragment key={d.id ?? d.name}>
            <span className="truncate" title={title}>
              {d.name}
            </span>
            <div className="h-3 overflow-hidden rounded-[3px] bg-muted" title={title}>
              <div className="h-full rounded-[3px] bg-(--chart-1)" style={{ width: `${Math.max(1, (d.value / max) * 100)}%` }} />
            </div>
            <span className="tabular-nums text-muted-foreground" title={title}>
              {formatter(d.value)}
            </span>
          </Fragment>
        );
      })}
    </div>
  );
}
