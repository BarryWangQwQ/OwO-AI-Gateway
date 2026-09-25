import { useMemo, useState } from "react";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { useTranslation } from "react-i18next";

import { Donut } from "@/components/charts";
import { RefreshCw } from "@/components/icons";
import { ErrorAlert, Hint, IconButton, PageHeader, TABLE_EDGE_INSET } from "@/components/page";
import { RingSkeleton, TableRowsSkeleton } from "@/components/skeletons";
import { AppVendorIcon, VendorIcon } from "@/components/vendor-icon";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableFooter, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { REFRESH, useQuery } from "@/hooks/use-query";
import { api, type GroupBy, type Summary } from "@/lib/api";
import { appName, compact, money, shortDate } from "@/lib/format";
import { useNames } from "@/lib/names";

const GROUPS: GroupBy[] = ["model", "app", "provider", "day"];
const RANGES = ["1", "7", "30", "90"];

export function UsagePage() {
  const { t } = useTranslation();
  const [days, setDays] = useState("7");
  const [by, setBy] = useState<GroupBy>("model");
  const report = useQuery(() => api.usage(Number(days), by), [days, by], { refreshInterval: REFRESH.live });
  const names = useNames();
  const data = report.data;
  /**
   * The first load renders the two chart cards and the table with skeletons in place of their content, so the page has
   * its final shape from the start. A range / grouping change or a reload keeps the current report on screen
   * (`refreshing`, the header icon spins) until the new one lands.
   */
  // Names only label rows, so an empty report doesn't wait for them.
  const loading = report.loading || (!names.ready && (report.data?.rows.length ?? 0) > 0);
  const priced = data?.total.cost_usd != null;
  const showCache = (data?.total.cached_input_tokens ?? 0) > 0;
  /** The display name of a row (`day` rows keep their date key). */
  const label = (s: Summary) => (by === "app" ? names.app(s.key) : by === "model" ? names.model(s.key) : by === "provider" ? names.provider(s.key) : s.key);
  /** The row's brand mark: the model's vendor (via its provider), the provider's preset, or the app's product logo; none for `day`. */
  const icon = (s: Summary) =>
    by === "model" ? (
      <VendorIcon id={s.key} provider={names.models?.find((m) => m.id === s.key)?.provider} className="size-4 shrink-0" />
    ) : by === "provider" ? (
      <VendorIcon provider={names.providers?.find((p) => p.id === s.key)?.preset ?? s.key} className="size-4 shrink-0" />
    ) : by === "app" ? (
      <AppVendorIcon app={appName(s.key)} className="size-4 shrink-0" />
    ) : null;
  const rows = data?.rows ?? [];
  const chartRows = (by === "day" ? rows : rows.slice(0, 10)).map((r) => ({ name: label(r), input: r.input_tokens, output: r.output_tokens }));

  const barConfig = useMemo(
    () =>
      ({
        input: { label: t("usage.series.input"), color: "var(--chart-1)" },
        output: { label: t("usage.series.output"), color: "var(--chart-2)" },
      }) satisfies ChartConfig,
    [t],
  );

  const cells = (s: Summary) => (
    <>
      <TableCell className="text-right tabular-nums">{s.calls}</TableCell>
      <TableCell className={`text-right tabular-nums ${s.failed > 0 ? "text-destructive" : "text-muted-foreground"}`}>{s.failed}</TableCell>
      <TableCell className="text-right tabular-nums">{compact(s.input_tokens)}</TableCell>
      {showCache && <TableCell className="text-right tabular-nums text-muted-foreground">{compact(s.cached_input_tokens)}</TableCell>}
      <TableCell className="text-right tabular-nums">{compact(s.output_tokens)}</TableCell>
      <TableCell className="text-right font-medium tabular-nums">{compact(s.input_tokens + s.output_tokens)}</TableCell>
      {priced && (
        <TableCell className="text-right tabular-nums">
          {s.cost_usd != null ? money(s.cost_usd) : "—"}
          {s.unpriced > 0 && <span className="text-muted-foreground">*</span>}
        </TableCell>
      )}
    </>
  );

  return (
    <div className="space-y-4">
      <PageHeader
        title={t("usage.title")}
        actions={
          <>
            <ToggleGroup type="single" variant="outline" size="sm" value={days} onValueChange={(v) => v && setDays(v)}>
              {RANGES.map((n) => (
                <ToggleGroupItem key={n} value={n}>
                  {t("usage.rangeDays", { count: Number(n) })}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <Select value={by} onValueChange={(v) => setBy(v as GroupBy)}>
              <SelectTrigger size="sm" className="w-32">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {GROUPS.map((g) => (
                  <SelectItem key={g} value={g}>
                    {t(`usage.by.${g}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <IconButton variant="default" label={t("common.refresh")} icon={<RefreshCw className={report.refreshing ? "animate-spin" : ""} />} onClick={() => void report.refresh()} />
          </>
        }
      />
      {report.error && <ErrorAlert error={report.error} />}

      {loading ? (
        <>
          <div className="grid gap-4 lg:grid-cols-3">
            <Card>
              <CardHeader>
                <CardTitle className="font-semibold">{t("usage.share")}</CardTitle>
              </CardHeader>
              <CardContent>
                <RingSkeleton className="mx-auto h-44" thickness="border-[14px]" />
              </CardContent>
            </Card>
            <Card className="lg:col-span-2">
              <CardHeader>
                <CardTitle className="font-semibold">{t("usage.tokens")}</CardTitle>
              </CardHeader>
              <CardContent>
                <Skeleton className="h-[220px] w-full rounded-xl" />
              </CardContent>
            </Card>
          </div>
          <Card className="py-0">
            <CardContent className="px-0">
              <Table className={TABLE_EDGE_INSET}>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t(`usage.by.${by}`)}</TableHead>
                    <TableHead className="text-right">{t("usage.columns.calls")}</TableHead>
                    <TableHead className="text-right">{t("usage.columns.failed")}</TableHead>
                    <TableHead className="text-right">{t("usage.columns.input")}</TableHead>
                    <TableHead className="text-right">{t("usage.columns.output")}</TableHead>
                    <TableHead className="text-right">{t("usage.columns.total")}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableRowsSkeleton
                  rows={5}
                  columns={[{ w: "w-40" }, { w: "w-8", right: true }, { w: "w-6", right: true }, { w: "w-12", right: true }, { w: "w-12", right: true }, { w: "w-14", right: true }]}
                />
              </Table>
            </CardContent>
          </Card>
        </>
      ) : !data ? null : rows.length === 0 ? (
        <Card>
          <CardContent className="py-16 text-center text-sm text-muted-foreground">{t("usage.noCallsInPeriod")}</CardContent>
        </Card>
      ) : (
        <>
          <div className="grid gap-4 lg:grid-cols-3">
            <Card>
              <CardHeader>
                <CardTitle className="font-semibold">{t("usage.share")}</CardTitle>
              </CardHeader>
              <CardContent>
                <Donut data={rows.slice(0, 8).map((r) => ({ name: label(r), value: r.input_tokens + r.output_tokens }))} unit={t("common.tokensUnit")} />
              </CardContent>
            </Card>
            <Card className="lg:col-span-2">
              <CardHeader>
                <CardTitle className="font-semibold">{t("usage.tokens")}</CardTitle>
              </CardHeader>
              <CardContent>
                <ChartContainer config={barConfig} className="aspect-auto h-[220px] w-full">
                  <BarChart data={chartRows} margin={{ left: 0, right: 12, top: 8 }}>
                    <CartesianGrid vertical={false} />
                    <XAxis dataKey="name" tickLine={false} axisLine={false} tickMargin={8} interval={0} tick={{ fontSize: 11 }} tickFormatter={(v: string) => (by === "day" ? shortDate(v) : v.length > 14 ? `${v.slice(0, 13)}…` : v)} />
                    <YAxis tickLine={false} axisLine={false} width="auto" tickMargin={4} tick={{ fontSize: 11 }} tickFormatter={(v: number) => compact(v)} />
                    <ChartTooltip content={<ChartTooltipContent />} />
                    <ChartLegend content={<ChartLegendContent />} />
                    <Bar dataKey="input" stackId="t" fill="var(--color-input)" radius={[0, 0, 4, 4]} />
                    <Bar dataKey="output" stackId="t" fill="var(--color-output)" radius={[4, 4, 0, 0]} />
                  </BarChart>
                </ChartContainer>
              </CardContent>
            </Card>
          </div>

          <Card className="py-0">
            <CardContent className="px-0">
              <Table className={TABLE_EDGE_INSET}>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t(`usage.by.${by}`)}</TableHead>
                    <TableHead className="text-right">{t("usage.columns.calls")}</TableHead>
                    <TableHead className="text-right">{t("usage.columns.failed")}</TableHead>
                    <TableHead className="text-right">
                      {t("usage.columns.input")}
                      <Hint text={t("usage.hints.input")} />
                    </TableHead>
                    {showCache && <TableHead className="text-right">{t("usage.columns.cached")}</TableHead>}
                    <TableHead className="text-right">
                      {t("usage.columns.output")}
                      <Hint text={t("usage.hints.output")} />
                    </TableHead>
                    <TableHead className="text-right">{t("usage.columns.total")}</TableHead>
                    {priced && (
                      <TableHead className="text-right">
                        {t("usage.columns.cost")}
                        <Hint text={t("usage.hints.cost")} />
                      </TableHead>
                    )}
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {rows.map((r) => (
                    <TableRow key={r.key}>
                      <TableCell className="font-medium" title={r.key}>
                        <div className="flex items-center gap-2">
                          {icon(r)}
                          <div className="min-w-0 truncate">{label(r)}</div>
                        </div>
                      </TableCell>
                      {cells(r)}
                    </TableRow>
                  ))}
                </TableBody>
                {rows.length > 1 && (
                  <TableFooter>
                    <TableRow>
                      <TableCell>{t("usage.columns.total")}</TableCell>
                      {cells(data.total)}
                    </TableRow>
                  </TableFooter>
                )}
              </Table>
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}
