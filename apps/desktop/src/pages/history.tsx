import { useState } from "react";
import { useTranslation } from "react-i18next";

import { RefreshCw } from "@/components/icons";
import { DetailRow, ErrorAlert, IconButton, PageHeader, TABLE_EDGE_INSET } from "@/components/page";
import { TableRowsSkeleton } from "@/components/skeletons";
import { CallStatusIcon } from "@/components/status-badge";
import { AppVendorIcon, VendorIcon } from "@/components/vendor-icon";
import { Card, CardContent } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { REFRESH, useQuery } from "@/hooks/use-query";
import { api, type StoredCall } from "@/lib/api";
import { appName, appTitle, compact, duration, exact, money } from "@/lib/format";
import { useNames, type Names } from "@/lib/names";

const ALL = "__all__";

export function HistoryPage() {
  const { t } = useTranslation();
  const [failedOnly, setFailedOnly] = useState(false);
  const [client, setClient] = useState(ALL);
  const [model, setModel] = useState(ALL);
  const [limit, setLimit] = useState("50");
  const [selected, setSelected] = useState<StoredCall>();
  const apps = useQuery(api.apps, [], { refreshInterval: REFRESH.config });
  // Also the source of the model filter's options (`names.models`).
  const names = useNames();
  const calls = useQuery(
    () => api.calls({ failedOnly, client: client === ALL ? undefined : client, model: model === ALL ? undefined : model, limit: Number(limit) }),
    [failedOnly, client, model, limit],
    { refreshInterval: REFRESH.live },
  );
  const priced = calls.data?.some((c) => c.cost_usd != null) ?? false;
  // The selected app's `owo connect` name (the filter value is its client id), for the trigger's icon + title.
  const clientApp = apps.data?.find((a) => a.client_id === client)?.app ?? appName(client);
  /**
   * Skeleton rows under the real header until the first page of calls and the names they're shown with are here.
   * A filter change or a reload keeps the current rows on screen (`refreshing`, the header icon spins) until the new ones land.
   */
  // Names only label rows, so an empty list doesn't wait for them.
  const loading = calls.loading || (!names.ready && (calls.data?.length ?? 0) > 0);

  return (
    <div className="space-y-6">
      <PageHeader
        title={t("history.title")}
        actions={<IconButton variant="default" label={t("common.refresh")} icon={<RefreshCw className={calls.refreshing ? "animate-spin" : ""} />} onClick={() => void calls.refresh()} />}
      />
      <div className="flex flex-wrap items-center gap-3">
        <Select value={client} onValueChange={setClient}>
          <SelectTrigger size="sm" className="w-40">
            {/* Icon + product name in the trigger, like the model filter. */}
            <SelectValue>
              {client === ALL ? (
                t("history.allApps")
              ) : (
                <>
                  <AppVendorIcon app={clientApp} className="size-4" /> {appTitle(clientApp)}
                </>
              )}
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>{t("history.allApps")}</SelectItem>
            {apps.data?.map((a) => (
              <SelectItem key={a.client_id} value={a.client_id}>
                <AppVendorIcon app={a.app} className="size-4" /> {appTitle(a.app)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={model} onValueChange={setModel}>
          <SelectTrigger size="sm" className="w-52">
            {/* Icon + display name in the trigger; the items also carry the muted id. */}
            <SelectValue>
              {model === ALL ? (
                t("history.allModels")
              ) : (
                <>
                  <VendorIcon id={model} provider={names.models?.find((m) => m.id === model)?.provider} className="size-4" /> {names.model(model)}
                </>
              )}
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>{t("history.allModels")}</SelectItem>
            {names.models?.map((m) => (
              <SelectItem key={m.id} value={m.id} title={m.id}>
                <VendorIcon id={m.id} provider={m.provider} className="size-4" /> {names.model(m.id)}
                {names.modelNamed(m.id) && <span className="truncate text-xs text-muted-foreground">{m.id}</span>}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={limit} onValueChange={setLimit}>
          <SelectTrigger size="sm" className="w-28">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {["20", "50", "100", "500"].map((n) => (
              <SelectItem key={n} value={n}>
                {t("history.last", { count: Number(n) })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <div className="flex items-center gap-2">
          <Switch id="failed" checked={failedOnly} onCheckedChange={setFailedOnly} />
          <Label htmlFor="failed">{t("history.failedOnly")}</Label>
        </div>
      </div>
      {calls.error && <ErrorAlert error={calls.error} />}
      <Card className="py-0">
        <CardContent className="px-0">
          {!loading && calls.data?.length === 0 ? (
            <p className="p-8 text-center text-sm text-muted-foreground">{failedOnly ? t("history.noFailedCalls") : t("history.noCallsRecorded")}</p>
          ) : (
            <Table className={TABLE_EDGE_INSET}>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("history.columns.time")}</TableHead>
                  <TableHead>{t("history.columns.app")}</TableHead>
                  <TableHead>{t("history.columns.model")}</TableHead>
                  <TableHead>{t("history.columns.status")}</TableHead>
                  <TableHead className="text-right">{t("history.columns.input")}</TableHead>
                  <TableHead className="text-right">{t("history.columns.output")}</TableHead>
                  {priced && <TableHead className="text-right">{t("history.columns.cost")}</TableHead>}
                  <TableHead className="text-right">{t("history.columns.duration")}</TableHead>
                </TableRow>
              </TableHeader>
              {loading && (
                <TableRowsSkeleton
                  rows={8}
                  columns={[{ w: "w-24" }, { w: "w-20" }, { w: "w-36" }, { w: "size-4", bar: "size-4 rounded-full" }, { w: "w-10", right: true }, { w: "w-10", right: true }, { w: "w-12", right: true }]}
                />
              )}
              <TableBody>
                {!loading &&
                  calls.data?.map((c) => (
                    <TableRow key={c.id} className="cursor-pointer" onClick={() => setSelected(c)}>
                      <TableCell className="tabular-nums text-muted-foreground">{c.time.slice(5)}</TableCell>
                      <TableCell>{names.app(c.client)}</TableCell>
                      <TableCell className="max-w-64">
                        <div className="flex items-center gap-2">
                          <VendorIcon id={c.model ?? c.requested_model} provider={c.provider} className="size-4 shrink-0" />
                          {/* One line per row; the error lives in the status tooltip and the detail sheet, not here. */}
                          <div className="min-w-0 truncate font-medium" title={c.model ?? c.requested_model}>
                            {names.model(c.model ?? c.requested_model)}
                          </div>
                        </div>
                      </TableCell>
                      <TableCell>
                        <CallStatusIcon status={c.status} upstream={c.upstream_status} detail={c.error_kind && `${c.error_kind}: ${c.error_message ?? ""}`} />
                      </TableCell>
                      <TableCell className="text-right tabular-nums">{c.input_tokens != null ? compact(c.input_tokens) : "—"}</TableCell>
                      <TableCell className="text-right tabular-nums">{c.output_tokens != null ? compact(c.output_tokens) : "—"}</TableCell>
                      {priced && <TableCell className="text-right tabular-nums">{c.cost_usd != null ? money(c.cost_usd) : "—"}</TableCell>}
                      <TableCell className="text-right tabular-nums text-muted-foreground">{duration(c.duration_ms)}</TableCell>
                    </TableRow>
                  ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
      <CallSheet call={selected} names={names} onClose={() => setSelected(undefined)} />
    </div>
  );
}

function CallSheet({ call: c, names, onClose }: { call?: StoredCall; names: Names; onClose: () => void }) {
  const { t } = useTranslation();
  const modelId = c ? (c.model ?? c.requested_model) : "";
  return (
    <Sheet open={!!c} onOpenChange={(open) => !open && onClose()}>
      <SheetContent className="data-[side=right]:w-full data-[side=right]:sm:max-w-xl">
        {c && (
          <>
            <SheetHeader>
              <SheetTitle className="flex items-center gap-2">
                {t("history.call", { id: c.id })} <CallStatusIcon status={c.status} upstream={c.upstream_status} />
              </SheetTitle>
              <SheetDescription>{c.time}</SheetDescription>
            </SheetHeader>
            <div className="divide-y overflow-y-auto px-4">
              <DetailRow label={t("history.detail.app")}>{names.app(c.client)}</DetailRow>
              <DetailRow label={t("history.detail.model")}>
                <VendorIcon id={modelId} provider={c.provider} className="mr-1.5 size-4 align-text-bottom" />
                {names.model(modelId)}
                {c.model && c.model !== c.requested_model && <span className="text-muted-foreground"> {t("history.detail.askedFor", { model: c.requested_model })}</span>}
                {names.modelNamed(modelId) && <span className="block font-mono text-xs text-muted-foreground">{modelId}</span>}
              </DetailRow>
              {c.provider && (
                <DetailRow label={t("history.detail.provider")}>
                  {names.provider(c.provider)}
                  {c.upstream_model && <span className="text-muted-foreground"> · {c.upstream_model}</span>}
                  {names.providerNamed(c.provider) && <span className="block font-mono text-xs text-muted-foreground">{c.provider}</span>}
                </DetailRow>
              )}
              {c.error_kind && (
                <DetailRow label={t("history.detail.error")}>
                  <span className="font-medium text-destructive">
                    {c.error_kind}
                    {c.upstream_status && ` · HTTP ${c.upstream_status}`}
                  </span>
                  {c.error_message && <p className="mt-1 break-all whitespace-pre-wrap font-mono text-xs text-muted-foreground">{c.error_message}</p>}
                </DetailRow>
              )}
              <DetailRow label={t("history.detail.duration")}>
                {duration(c.duration_ms)}
                {c.first_token_ms != null && <span className="text-muted-foreground"> · {t("history.detail.firstToken", { duration: duration(c.first_token_ms) })}</span>}
              </DetailRow>
              <DetailRow label={t("history.detail.input")}>
                {c.input_tokens != null ? exact(c.input_tokens) : t("common.notReported")}
                {(c.cached_input_tokens ?? 0) > 0 && <span className="text-muted-foreground"> · {t("history.detail.cached", { n: exact(c.cached_input_tokens!) })}</span>}
                {(c.cache_creation_input_tokens ?? 0) > 0 && (
                  <span className="text-muted-foreground"> · {t("history.detail.cacheWrite", { n: exact(c.cache_creation_input_tokens!) })}</span>
                )}
              </DetailRow>
              <DetailRow label={t("history.detail.output")}>
                {c.output_tokens != null ? exact(c.output_tokens) : t("common.notReported")}
                {(c.reasoning_tokens ?? 0) > 0 && <span className="text-muted-foreground"> · {t("history.detail.reasoning", { n: exact(c.reasoning_tokens!) })}</span>}
              </DetailRow>
              <DetailRow label={t("history.detail.cost")}>
                {c.cost_usd != null ? `≈ ${money(c.cost_usd)}` : <span className="text-muted-foreground">{t("history.detail.noPrice")}</span>}
              </DetailRow>
              {c.stop_reason && <DetailRow label={t("history.detail.stopReason")}>{c.stop_reason}</DetailRow>}
              <DetailRow label={t("history.detail.streamed")}>{c.stream ? t("common.yes") : t("common.no")}</DetailRow>
              <DetailRow label={t("history.detail.requestId")}>
                <span className="font-mono text-xs">{c.request_id}</span>
              </DetailRow>
            </div>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}
