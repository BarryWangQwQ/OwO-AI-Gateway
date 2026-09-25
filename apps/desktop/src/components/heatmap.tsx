import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { compact, money, monthLabel } from "@/lib/format";

export type HeatDay = { calls: number; tokens: number; cost: number | null };

const WEEKS = 53;
/** Intensity levels 0..4: level 0 uses the theme's muted color; 1..4 use GitHub's contribution-graph greens (light / dark). */
export const HEAT_LEVELS = [
  "bg-muted",
  "bg-[#9be9a8] dark:bg-[#0e4429]",
  "bg-[#40c463] dark:bg-[#006d32]",
  "bg-[#30a14e] dark:bg-[#26a641]",
  "bg-[#216e39] dark:bg-[#39d353]",
];

function isoDate(d: Date): string {
  return d.toLocaleDateString("sv-SE");
}

const GAP = 3;

/**
 * A year of days as a GitHub-style grid: one column per week, one row per weekday.
 * Cells scale with the container width, up to `maxCell` px each (so the height stays bounded on wide windows).
 * With `loading` (pass an empty `days`) the grid is its own placeholder: same size, all cells at level 0, pulsing, no tooltips.
 */
export function Heatmap({ days, maxCell, loading = false }: { days: Map<string, HeatDay>; maxCell?: number; loading?: boolean }) {
  const { t, i18n } = useTranslation();
  const { cells, months } = useMemo(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const start = new Date(today);
    start.setDate(start.getDate() - start.getDay() - (WEEKS - 1) * 7);
    const values = [...days.values()].map((d) => d.tokens).filter((t) => t > 0).sort((a, b) => a - b);
    const quantile = (q: number) => values[Math.min(values.length - 1, Math.floor(q * values.length))] ?? 0;
    const cuts = [quantile(0.25), quantile(0.5), quantile(0.75)];
    const level = (tokens: number) => (tokens <= 0 ? 0 : 1 + cuts.filter((c) => tokens > c).length);

    const cells: { key: string; level: number; title: string; future: boolean }[] = [];
    const months: { label: string; column: number }[] = [];
    let lastMonth = -1;
    for (let i = 0; i < WEEKS * 7; i++) {
      const date = new Date(start);
      date.setDate(start.getDate() + i);
      const key = isoDate(date);
      const day = days.get(key);
      const future = date > today;
      if (i % 7 === 0 && date.getMonth() !== lastMonth && !future) {
        // Label a month at the first week that starts inside it (skip a label crammed at the very end).
        if (i / 7 < WEEKS - 2) months.push({ label: monthLabel(date), column: i / 7 });
        lastMonth = date.getMonth();
      }
      const title = day
        ? t("heatmap.day", { date: key, calls: t("common.calls", { count: day.calls }), tokens: compact(day.tokens) }) +
          (day.cost != null ? ` · ${money(day.cost)}` : "")
        : t("heatmap.noCalls", { date: key });
      cells.push({ key, level: day ? level(day.tokens) : 0, title, future });
    }
    return { cells, months };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [days, t, i18n.language]);

  // Fills the width; once the cap kicks in it is centered so any spare room splits evenly (the label row shares the width).
  return (
    <div className="mx-auto w-full space-y-1.5" style={maxCell ? { maxWidth: WEEKS * maxCell + (WEEKS - 1) * GAP } : undefined}>
      <div
        aria-busy={loading || undefined}
        className={`grid w-full ${loading ? "animate-pulse" : ""}`}
        style={{ gap: GAP, gridTemplateColumns: `repeat(${WEEKS}, minmax(0, 1fr))`, gridTemplateRows: "repeat(7, auto)", gridAutoFlow: "column" }}
      >
        {cells.map((c) => (
          <div key={c.key} title={c.future || loading ? undefined : c.title} className={`aspect-square rounded-[2px] ${c.future ? "opacity-0" : HEAT_LEVELS[c.level]}`} />
        ))}
      </div>
      <div className="relative h-4 text-xs text-muted-foreground">
        {months.map((m) => (
          <span key={m.column} className="absolute" style={{ left: `${(m.column / WEEKS) * 100}%` }}>
            {m.label}
          </span>
        ))}
      </div>
    </div>
  );
}
