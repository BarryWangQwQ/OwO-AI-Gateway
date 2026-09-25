import { useMemo, useState, type ReactNode } from "react";
import { Area, AreaChart, CartesianGrid, XAxis } from "recharts";
import { Activity, ArrowRight, CircleCheck, CircleDollarSign, CircleSlash, CircleX, Coins, DatabaseZap, Plus, Radio, RefreshCw } from "@/components/icons";
import { useTranslation } from "react-i18next";
import { cn } from "cn";

import { useApp } from "@/components/app-context";
import { Donut, Gauge, RankBars, Sparkline } from "@/components/charts";
import { HEAT_LEVELS, Heatmap, type HeatDay } from "@/components/heatmap";
import { ErrorAlert, IconButton, PageHeader } from "@/components/page";
import { RankBarsSkeleton, RecentRowsSkeleton, RingSkeleton } from "@/components/skeletons";
import { CallStatusIcon } from "@/components/status-badge";
import { VendorIcon } from "@/components/vendor-icon";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { Empty, EmptyContent, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { REFRESH, useQuery } from "@/hooks/use-query";
import { api } from "@/lib/api";
import { compact, duration, money, shortDate } from "@/lib/format";
import { useNames } from "@/lib/names";

/** Local dates `YYYY-MM-DD`, oldest first. */
function lastDays(n: number): string[] {
  return Array.from({ length: n }, (_, i) => {
    const d = new Date();
    d.setDate(d.getDate() - (n - 1 - i));
    return d.toLocaleDateString("sv-SE");
  });
}

const RECENT_LIMIT = 5;
const RANK_LIMIT = 5;

/**
 * The page is a grid that fills the viewport (`--page-height`, set in App.tsx): header, KPIs and the heatmap take
 * what they need, the two chart rows split the rest. Charts and lists sit in a `Fill` so they never grow the page;
 * only below the `Fill` floor (very short windows) does the page scroll.
 */
export function DashboardPage() {
  const { t, i18n } = useTranslation();
  const { status, navigate } = useApp();
  const live = { refreshInterval: REFRESH.live };
  const today = useQuery(api.today, [], live);
  const daily = useQuery(() => api.usage(14, "day"), [], live);
  const week = useQuery(() => api.usage(7, "model"), [], live);
  const byApp = useQuery(() => api.usage(7, "app"), [], live);
  const recent = useQuery(() => api.calls({ failedOnly: false, limit: RECENT_LIMIT }), [], live);
  const names = useNames();
  const [metric, setMetric] = useState<"tokens" | "cost">("tokens");
  const year = useQuery(() => api.usage(371, "day"), [], live);
  const providers = useQuery(api.providers, [], { refreshInterval: REFRESH.config });
  const queries = [today, daily, week, byApp, recent, year];
  /**
   * Every card has its own same-size placeholder for the first load (KPI value bars, the empty heatmap, chart / ring /
   * rank / row skeletons), so the page keeps its shape while the six queries land one by one. Polls keep every card's
   * content mounted; the header refresh button shows the skeletons again for a moment while its icon spins.
   */
  const refreshing = queries.some((q) => q.refreshing);
  const refresh = () => Promise.all(queries.map((q) => q.refresh()));
  const yearDays = useMemo(
    () => new Map<string, HeatDay>((year.data?.rows ?? []).map((r) => [r.key, { calls: r.calls, tokens: r.input_tokens + r.output_tokens, cost: r.cost_usd }])),
    [year.data],
  );

  const trendConfig = useMemo(
    () =>
      ({
        input: { label: t("dashboard.series.input"), color: "var(--chart-1)" },
        output: { label: t("dashboard.series.output"), color: "var(--chart-2)" },
        cost: { label: t("dashboard.series.cost"), color: "var(--chart-3)" },
      }) satisfies ChartConfig,
    [t],
  );

  const series = useMemo(() => {
    const byDay = new Map((daily.data?.rows ?? []).map((r) => [r.key, r]));
    return lastDays(14).map((day) => {
      const r = byDay.get(day);
      const input = r?.input_tokens ?? 0;
      const output = r?.output_tokens ?? 0;
      const cached = r?.cached_input_tokens ?? 0;
      return { day: shortDate(day), input, output, tokens: input + output, calls: r?.calls ?? 0, cost: r?.cost_usd ?? 0, cacheRate: input > 0 ? cached / input : 0 };
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [daily.data, i18n.language]);

  const fresh = providers.data?.length === 0 && year.data?.total.calls === 0;
  if ((status && !status.configExists) || fresh) {
    return (
      <div className="space-y-4">
        <PageHeader title={t("dashboard.title")} />
        <Empty className="mt-12 border">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <Radio />
            </EmptyMedia>
            <EmptyTitle>{t("dashboard.getStarted")}</EmptyTitle>
          </EmptyHeader>
          <EmptyContent>
            <Button onClick={() => navigate("providers")}>
              <Plus /> {t("dashboard.addProvider")}
            </Button>
          </EmptyContent>
        </Empty>
      </div>
    );
  }

  const td = today.data;
  const total = week.data?.total;
  const cacheRate = td && td.input_tokens > 0 ? (td.cached_input_tokens / td.input_tokens) * 100 : null;

  return (
    <div className="@container grid min-h-(--page-height) grid-rows-[auto_auto_auto_minmax(0,1fr)_minmax(0,1fr)] gap-3">
      <div className="space-y-3">
        <PageHeader title={t("dashboard.title")} actions={<IconButton variant="default" label={t("common.refresh")} icon={<RefreshCw className={refreshing ? "animate-spin" : ""} />} onClick={() => void refresh()} />} />
        {status?.configError && <ErrorAlert title={t("dashboard.configProblems")} error={status.configError} />}
      </div>

      <div className="grid grid-cols-2 gap-3 @xl:grid-cols-4">
        <Kpi icon={Activity} label={t("dashboard.calls")} value={td && String(td.calls)} title={td && td.failed > 0 ? t("dashboard.failedCount", { count: td.failed }) : undefined}>
          <Sparkline data={series} dataKey="calls" kind="bar" index={3} className="h-8 w-16" />
        </Kpi>
        <Kpi icon={Coins} label={t("dashboard.tokens")} value={td && compact(td.input_tokens + td.output_tokens)}>
          <Sparkline data={series} dataKey="tokens" index={0} className="h-8 w-16" />
        </Kpi>
        <Kpi icon={CircleDollarSign} label={t("dashboard.cost")} value={td && (td.cost_usd != null ? money(td.cost_usd) : "—")}>
          <Sparkline data={series} dataKey="cost" index={2} className="h-8 w-16" />
        </Kpi>
        <Kpi
          icon={DatabaseZap}
          label={t("dashboard.cacheHitRate")}
          value={td && (cacheRate === null ? "—" : `${Math.round(cacheRate)}%`)}
          title={td && td.input_tokens > 0 ? t("dashboard.cacheHitNote", { cached: compact(td.cached_input_tokens), input: compact(td.input_tokens) }) : undefined}
        >
          <Sparkline data={series} dataKey="cacheRate" index={1} className="h-8 w-16" />
        </Kpi>
      </div>

      <Card className="gap-2 py-3">
        <CardHeader>
          <CardTitle className="font-semibold">{t("dashboard.activity")}</CardTitle>
          <CardAction className="flex items-center gap-1 text-xs text-muted-foreground">
            {t("dashboard.less")}
            {HEAT_LEVELS.map((cls) => (
              <span key={cls} className={`inline-block size-2.5 rounded-[2px] ${cls}`} />
            ))}
            {t("dashboard.more")}
          </CardAction>
        </CardHeader>
        {/* The heatmap is its own placeholder: the grid of empty cells has exactly the final size, it just pulses until the year is in. */}
        <CardContent>
          <Heatmap days={yearDays} maxCell={14} loading={!year.data} />
        </CardContent>
      </Card>

      <div className="grid grid-cols-3 gap-3">
        <Card className="col-span-2 gap-2 py-3">
          <CardHeader>
            <CardTitle className="font-semibold">{t("dashboard.trend")}</CardTitle>
            <CardAction className="-my-1">
              <ToggleGroup type="single" variant="outline" size="sm" value={metric} onValueChange={(v) => v && setMetric(v as "tokens" | "cost")}>
                <ToggleGroupItem value="tokens" aria-label={t("dashboard.tokens")}>
                  <Coins />
                </ToggleGroupItem>
                <ToggleGroupItem value="cost" aria-label={t("dashboard.cost")}>
                  <CircleDollarSign />
                </ToggleGroupItem>
              </ToggleGroup>
            </CardAction>
          </CardHeader>
          <CardContent className="flex min-h-0 flex-1 flex-col">
            <Fill>
              {!daily.data ? (
                <Skeleton className="size-full" />
              ) : (
                <ChartContainer config={trendConfig} className="aspect-auto size-full">
                  <AreaChart data={series} margin={{ left: 12, right: 12, top: 8 }}>
                    <defs>
                      {(["input", "output", "cost"] as const).map((k) => (
                        <linearGradient key={k} id={`trend-${k}`} x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor={`var(--color-${k})`} stopOpacity={0.5} />
                          <stop offset="95%" stopColor={`var(--color-${k})`} stopOpacity={0.05} />
                        </linearGradient>
                      ))}
                    </defs>
                    <CartesianGrid vertical={false} />
                    <XAxis dataKey="day" tickLine={false} axisLine={false} tickMargin={8} minTickGap={24} interval="preserveStartEnd" padding={{ left: 8, right: 8 }} />
                    <ChartTooltip content={<ChartTooltipContent indicator="dot" />} />
                    {metric === "tokens" ? (
                      <>
                        <Area dataKey="input" type="monotone" stackId="t" stroke="var(--color-input)" fill="url(#trend-input)" />
                        <Area dataKey="output" type="monotone" stackId="t" stroke="var(--color-output)" fill="url(#trend-output)" />
                        <ChartLegend content={<ChartLegendContent />} />
                      </>
                    ) : (
                      <Area dataKey="cost" type="monotone" stroke="var(--color-cost)" fill="url(#trend-cost)" />
                    )}
                  </AreaChart>
                </ChartContainer>
              )}
            </Fill>
          </CardContent>
        </Card>

        <Card className="gap-2 py-3">
          <CardHeader>
            <CardTitle className="font-semibold">{t("dashboard.success")}</CardTitle>
          </CardHeader>
          <CardContent className="flex min-h-0 flex-1 flex-col">
            <Fill className="flex items-center justify-center gap-5 py-1">
              {/* `h-full` + `aspect-square` keeps the ring a square that fits the row; the legend sits right beside it, centered. */}
              <Gauge counts={total && { ok: total.calls - total.failed - total.cancelled, failed: total.failed, cancelled: total.cancelled }} className="mx-0 h-full w-auto max-w-3/5 min-w-0" />
              <div className="grid shrink-0 gap-1.5 text-sm">
                <Count icon={CircleCheck} className="text-emerald-500" value={total ? total.calls - total.failed - total.cancelled : undefined} />
                <Count icon={CircleX} className="text-destructive" value={total?.failed} />
                <Count icon={CircleSlash} className="text-muted-foreground" value={total?.cancelled} />
              </div>
            </Fill>
          </CardContent>
        </Card>
      </div>

      <div className="grid grid-cols-3 gap-3">
        <Card className="gap-2 py-3">
          <CardHeader>
            <CardTitle className="font-semibold">{t("dashboard.appsWeek")}</CardTitle>
          </CardHeader>
          <CardContent className="flex min-h-0 flex-1 flex-col">
            <Fill className="flex items-center justify-center">
              {!byApp.data ? (
                <RingSkeleton className="h-full" />
              ) : byApp.data.rows.length ? (
                <Donut data={byApp.data.rows.map((r) => ({ name: names.app(r.key), value: r.input_tokens + r.output_tokens }))} unit={t("common.tokensUnit")} className="mx-0 h-full" />
              ) : (
                <NoData />
              )}
            </Fill>
          </CardContent>
        </Card>
        <Card className="gap-2 py-3">
          <CardHeader>
            <CardTitle className="font-semibold">{t("dashboard.modelsWeek")}</CardTitle>
          </CardHeader>
          <CardContent className="flex min-h-0 flex-1 flex-col">
            <Fill>
              {!week.data || (!names.ready && week.data.rows.length > 0) ? (
                <RankBarsSkeleton rows={RANK_LIMIT} />
              ) : week.data.rows.length ? (
                <RankBars data={week.data.rows.slice(0, RANK_LIMIT).map((r) => ({ name: names.model(r.key), id: r.key, value: r.input_tokens + r.output_tokens }))} />
              ) : (
                <NoData />
              )}
            </Fill>
          </CardContent>
        </Card>
        <Card className="gap-2 py-3">
          <CardHeader>
            <CardTitle className="font-semibold">{t("dashboard.recent")}</CardTitle>
            <CardAction className="-my-1">
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button variant="ghost" size="icon-sm" aria-label={t("dashboard.allCalls")} onClick={() => navigate("history")}>
                    <ArrowRight />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>{t("dashboard.allCalls")}</TooltipContent>
              </Tooltip>
            </CardAction>
          </CardHeader>
          <CardContent className="flex min-h-0 flex-1 flex-col">
            <Fill>
              {!recent.data || (!names.ready && recent.data.length > 0) ? (
                <RecentRowsSkeleton rows={RECENT_LIMIT} />
              ) : recent.data.length === 0 ? (
                <NoData />
              ) : (
                <ScrollArea className="size-full">
                  <div className="space-y-1.5 pr-3">
                    {/* Fixed two-line rows (title + subline); failures are told by the icon + its tooltip. Clicking goes to History for the full details. */}
                    {recent.data?.map((c) => (
                      <div key={c.id} role="link" tabIndex={0} className="flex cursor-pointer items-center gap-2.5 text-sm" onClick={() => navigate("history")} onKeyDown={(e) => e.key === "Enter" && navigate("history")}>
                        <CallStatusIcon status={c.status} upstream={c.upstream_status} detail={c.error_kind && `${c.error_kind}: ${c.error_message ?? ""}`} />
                        <VendorIcon id={c.model ?? c.requested_model} provider={c.provider} className="size-4 shrink-0" />
                        <div className="min-w-0 flex-1">
                          <div className="truncate leading-5 font-medium" title={c.model ?? c.requested_model}>
                            {names.model(c.model ?? c.requested_model)}
                          </div>
                          <div className="truncate text-xs leading-4 text-muted-foreground">
                            {names.app(c.client)} · {c.time.slice(11, 16)}
                          </div>
                        </div>
                        <div className="text-right text-xs leading-4 tabular-nums text-muted-foreground">
                          <div>{c.input_tokens != null ? compact(c.input_tokens + (c.output_tokens ?? 0)) : "—"}</div>
                          <div>{c.cost_usd != null ? money(c.cost_usd) : duration(c.duration_ms)}</div>
                        </div>
                      </div>
                    ))}
                  </div>
                </ScrollArea>
              )}
            </Fill>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

/**
 * Takes the remaining height of a card and hands it to its children as a definite box, without letting their size
 * feed back into the layout (recharts' ResponsiveContainer otherwise grows the row it measures). `min-h-28` is the floor
 * below which the page starts to scroll instead of squeezing the charts further.
 */
function Fill({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div className="relative min-h-28 flex-1">
      <div className={cn("absolute inset-0", className)}>{children}</div>
    </div>
  );
}

/**
 * Two fixed-height rows (header `h-4`, value `h-8`) so all four cards share one height and the big numbers sit on a
 * common baseline. The sparkline is `h-8` and bottom-aligned, which makes it exactly centred on the value row.
 * Extra detail (failed count, cache hit breakdown) goes into the card's `title` tooltip instead of a caption line.
 */
function Kpi({ icon: Icon, label, value, title, children }: { icon: typeof Activity; label: string; value?: string; title?: string; children: ReactNode }) {
  return (
    <Card className="py-3.5" title={title}>
      <CardContent className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex h-4 items-center gap-1.5 text-xs leading-4 text-muted-foreground">
            <Icon className="size-3.5 shrink-0" /> <span className="truncate">{label}</span>
          </div>
          <div className="mt-2 flex h-8 items-center">
            {value === undefined ? <Skeleton className="h-6 w-16" /> : <div className="truncate text-2xl leading-none font-semibold tabular-nums">{value}</div>}
          </div>
        </div>
        <div className="hidden shrink-0 self-end @4xl:block">{children}</div>
      </CardContent>
    </Card>
  );
}

function Count({ icon: Icon, className, value }: { icon: typeof Activity; className: string; value?: number }) {
  return (
    <div className="flex items-center gap-1.5">
      <Icon className={`size-4 ${className}`} />
      <span className="font-medium tabular-nums">{value ?? "—"}</span>
    </div>
  );
}

function NoData() {
  const { t } = useTranslation();
  return <div className="flex size-full items-center justify-center text-sm text-muted-foreground">{t("dashboard.noCallsYet")}</div>;
}
