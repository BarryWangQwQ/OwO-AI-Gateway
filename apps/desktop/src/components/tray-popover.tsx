import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";

import { ArrowRight, CircleCheck, CircleSlash, CircleX, LayoutDashboard, LoaderCircle, Power, PowerOff, X } from "@/components/icons";
import { AppVendorIcon, VendorIcon } from "@/components/vendor-icon";
import { api, type AppInfo, type Status, type StoredCall, type UsageReport } from "@/lib/api";
import { appName, appTitle, compact, duration, money } from "@/lib/format";
import { useNames } from "@/lib/names";
import { cn } from "@/lib/utils";

export function TrayPopover() {
  const { t, i18n } = useTranslation();
  const names = useNames();
  const [status, setStatus] = useState<Status>();
  const [usage, setUsage] = useState<UsageReport>();
  const [recentCalls, setRecentCalls] = useState<StoredCall[]>([]);
  const [apps, setApps] = useState<AppInfo[]>([]);
  const [appsLoading, setAppsLoading] = useState(true);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [busyApp, setBusyApp] = useState<string>();
  const [gatewayError, setGatewayError] = useState<string>();
  const [appError, setAppError] = useState<string>();

  const refresh = useCallback(async () => {
    const [statusResult, usageResult, callsResult] = await Promise.allSettled([
      api.status(),
      api.usage(7, "day"),
      api.calls({ failedOnly: false, limit: 3 }),
    ]);
    if (statusResult.status === "fulfilled") setStatus(statusResult.value);
    if (usageResult.status === "fulfilled") setUsage(usageResult.value);
    if (callsResult.status === "fulfilled") setRecentCalls(callsResult.value);
    setLoading(false);
  }, []);

  const refreshApps = useCallback(async () => {
    try {
      setApps(await api.apps());
    } catch {
      // The status and usage preview remain useful if listing app integrations fails.
    } finally {
      setAppsLoading(false);
    }
  }, []);

  useEffect(() => {
    document.documentElement.classList.add("tray-window");
    void refresh();
    void refreshApps();
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void refresh();
    }, 5_000);
    const appsTimer = window.setInterval(() => {
      if (document.visibilityState === "visible") void refreshApps();
    }, 20_000);
    const onVisibility = () => {
      if (document.visibilityState === "visible") {
        void refresh();
        void refreshApps();
      }
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      document.documentElement.classList.remove("tray-window");
      window.clearInterval(timer);
      window.clearInterval(appsTimer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [refresh, refreshApps]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void listen<string>("tray:language", ({ payload }) => void i18n.changeLanguage(payload))
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [i18n]);

  const trend = useMemo(() => {
    const rows = [...(usage?.rows ?? [])].sort((a, b) => a.key.localeCompare(b.key)).slice(-7);
    return [...Array.from({ length: 7 - rows.length }, (_, index) => ({ key: `empty-${index}`, tokens: 0, calls: 0 })), ...rows.map((row) => ({
      key: row.key,
      tokens: row.input_tokens + row.output_tokens,
      calls: row.calls,
    }))];
  }, [usage]);
  const maxTokens = Math.max(1, ...trend.map((day) => day.tokens));
  const totalTokens = usage ? usage.total.input_tokens + usage.total.output_tokens : undefined;
  const address = status?.gatewayAddress
    ? status.gatewayAddress.startsWith("http")
      ? status.gatewayAddress
      : `http://${status.gatewayAddress}`
    : "127.0.0.1:8787";

  const close = () => void api.trayClosePopover().catch(() => {});

  const toggleGateway = async () => {
    if (!status?.configExists || busy) return;
    setBusy(true);
    setGatewayError(undefined);
    try {
      const result = await (status.gatewayRunning ? api.gatewayStop() : api.gatewayStart());
      if (!result.ok) setGatewayError(result.output.split("\n").map((line) => line.trim()).find(Boolean) ?? t("toast.failed"));
    } catch (error) {
      setGatewayError(String(error));
    } finally {
      await refresh();
      void api.trayRefresh().catch(() => {});
      setBusy(false);
    }
  };

  const openDashboard = () => void api.trayOpenMain("dashboard").catch(() => {});
  const openUsage = () => void api.trayOpenMain("usage").catch(() => {});
  const openApps = () => void api.trayOpenMain("apps").catch(() => {});
  const openHistory = () => void api.trayOpenMain("history").catch(() => {});

  const toggleApp = async (app: AppInfo) => {
    if (busyApp) return;
    // Copilot requires manual setup in its own settings; the Apps page has the instructions.
    if (app.app === "copilot") {
      openApps();
      return;
    }
    setBusyApp(app.app);
    setAppError(undefined);
    try {
      const model = app.takes_model ? (await api.general()).clients[app.client_id]?.model ?? null : null;
      const result = app.connected ? await api.appDisconnect(app.app) : await api.appConnect(app.app, model);
      if (!result.ok) {
        const lines = result.output.split("\n").map((line) => line.trim()).filter(Boolean);
        setAppError(lines.find((line) => /^error\s*:/i.test(line)) ?? lines[0] ?? t("toast.failed"));
      }
    } catch (error) {
      setAppError(String(error));
    } finally {
      await refreshApps();
      void api.trayRefresh().catch(() => {});
      setBusyApp(undefined);
    }
  };

  return (
    <main className="tray-window-root h-svh w-svw p-0.5 text-foreground">
      <section className="relative flex h-full flex-col overflow-hidden rounded-[24px] border border-border bg-card p-3.5 shadow-sm">
        <header className="flex shrink-0 items-center gap-2.5">
          <div className="min-w-0 flex-1">
            <p className="truncate text-base font-semibold tracking-[-0.025em]">{t("tray.productName")}</p>
          </div>
          <button
            type="button"
            onClick={close}
            aria-label={t("common.close")}
            className="flex size-7 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </header>

        <div className="mt-3 flex w-full shrink-0 items-center gap-2.5">
          <button
            type="button"
            onClick={openDashboard}
            className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-md text-left text-[13px] transition-colors hover:text-foreground"
          >
            <span className={cn("size-2.5 shrink-0 rounded-full", status?.gatewayRunning ? "bg-emerald-500" : "bg-muted-foreground/60")} />
            <span className="shrink-0 font-medium">{status?.gatewayRunning ? t("nav.running") : status ? t("nav.stopped") : t("common.loading")}</span>
            <span className="ml-auto truncate font-mono text-[11px] text-muted-foreground">{address.replace(/^https?:\/\//, "")}</span>
          </button>
          <button
            type="button"
            onClick={() => void toggleGateway()}
            aria-label={status?.gatewayRunning ? t("nav.stopGateway") : t("nav.startGateway")}
            disabled={!status?.configExists || busy || loading}
            className={cn(
              "flex size-8 shrink-0 items-center justify-center rounded-full transition-colors",
              status?.gatewayRunning
                ? "bg-secondary text-secondary-foreground hover:bg-secondary/75"
                : "bg-primary text-primary-foreground hover:bg-primary/80",
              "disabled:cursor-not-allowed disabled:opacity-55",
            )}
          >
            {busy ? <LoaderCircle className="size-4 animate-spin" /> : status?.gatewayRunning ? <PowerOff className="size-4" /> : <Power className="size-4" />}
          </button>
        </div>
        {gatewayError && <p role="alert" className="mt-1 line-clamp-1 shrink-0 text-[11px] text-destructive">{gatewayError}</p>}
        {!status?.configExists && status && <p className="mt-1 line-clamp-1 shrink-0 text-[11px] text-muted-foreground">{status.configError || t("dashboard.getStarted")}</p>}

        <section className="no-scrollbar mt-4 flex min-h-0 flex-1 flex-col overflow-y-auto">
          <button
            type="button"
            onClick={openUsage}
            className="flex w-full shrink-0 flex-col text-left transition-opacity hover:opacity-80"
          >
            <div className="flex shrink-0 items-center justify-between text-[11px] font-medium text-muted-foreground">
              <span>{t("tray.lastSevenDays")}</span>
            </div>
            <div className="mt-1.5 flex shrink-0 items-end justify-between gap-4">
              <div>
                <p className="text-[25px] font-semibold leading-none tracking-[-0.055em] tabular-nums">{totalTokens === undefined ? "—" : compact(totalTokens)}</p>
                <p className="mt-0.5 text-[11px] text-muted-foreground">{t("common.tokens")}</p>
              </div>
              <div className="text-right">
                <p className="text-lg font-semibold leading-none tabular-nums">{usage ? compact(usage.total.calls) : "—"}</p>
                <div className="mt-0.5 flex items-center justify-end gap-1 text-[11px] text-muted-foreground">
                  <span>{t("dashboard.calls")}</span>
                  {!!usage?.total.failed && <span className="text-[10px] text-destructive">· {t("dashboard.failedCount", { count: usage.total.failed })}</span>}
                </div>
              </div>
            </div>
            <div className="mt-1.5 flex h-4 shrink-0 items-end gap-2" aria-hidden="true">
              {trend.map((day) => (
                <span
                  key={day.key}
                  className="min-h-1 flex-1 rounded-t-[3px] bg-chart-1 transition-[height] duration-500"
                  style={{ height: `${Math.max(12, (day.tokens / maxTokens) * 100)}%`, opacity: day.tokens === 0 ? 0.25 : 1 }}
                />
              ))}
            </div>
          </button>

          <section className="mt-5 flex shrink-0 flex-col">
            <button
              type="button"
              onClick={openHistory}
              className="flex shrink-0 items-center justify-between text-left text-[11px] font-medium text-muted-foreground transition-colors hover:text-foreground"
            >
              <span>{t("dashboard.recent")}</span>
              <span className="flex items-center gap-1">
                {t("nav.history")}
                <ArrowRight className="size-3" />
              </span>
            </button>
            <div className="mt-1.5 flex flex-col gap-0.5">
              {recentCalls.length > 0 ? recentCalls.map((call) => {
                const model = call.model ?? call.requested_model;
                const tokens = call.input_tokens == null ? undefined : call.input_tokens + (call.output_tokens ?? 0);
                const client = appName(call.client);
                const time = call.time.slice(11, 16);
                const StatusIcon = call.status === "ok" ? CircleCheck : call.status === "error" ? CircleX : CircleSlash;
                const statusColor = call.status === "ok" ? "text-emerald-500" : call.status === "error" ? "text-destructive" : "text-muted-foreground";
                const details = [
                  names.model(model),
                  appTitle(client),
                  time,
                  tokens == null ? undefined : `${tokens} tokens`,
                  call.cost_usd == null ? duration(call.duration_ms) : money(call.cost_usd),
                ].filter(Boolean).join(", ");
                return (
                  <button
                    key={call.id}
                    type="button"
                    onClick={openHistory}
                    aria-label={details}
                    className="flex h-6 w-full shrink-0 items-center gap-2 rounded-md px-1 text-left transition-colors hover:bg-muted/60"
                  >
                    <StatusIcon className={cn("size-3.5 shrink-0", statusColor)} aria-hidden="true" />
                    <VendorIcon id={model} provider={call.provider} className="size-3.5 shrink-0" />
                    <span className="min-w-0 flex-1 truncate text-[11px] font-medium" title={model}>{names.model(model)}</span>
                    <span aria-hidden="true" className="flex size-3.5 shrink-0 items-center justify-center">
                      <AppVendorIcon app={client} className="size-3.5" />
                    </span>
                    <span className="shrink-0 text-[10px] tabular-nums text-muted-foreground">{time}</span>
                    <span className="min-w-8 shrink-0 text-right text-[10px] tabular-nums text-muted-foreground">{tokens == null ? "—" : compact(tokens)}</span>
                  </button>
                );
              }) : (
                <button type="button" onClick={openHistory} className="w-full py-1 text-left text-[11px] text-muted-foreground hover:text-foreground">
                  {t("dashboard.noCallsYet")}
                </button>
              )}
            </div>
          </section>

          <div className="mt-4 flex w-full shrink-0 items-center gap-2.5">
            <button
              type="button"
              onClick={openApps}
              className="shrink-0 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
            >
              {t("apps.title")}
            </button>
            <div role="group" aria-label={t("apps.title")} className="flex min-w-0 flex-1 flex-wrap items-center justify-end gap-0.5">
              {appsLoading ? <LoaderCircle className="mr-1 size-3.5 animate-spin text-muted-foreground" /> : apps.length > 0 ? apps.map((app) => {
                const manualSetup = app.app === "copilot";
                const action = manualSetup ? t("tray.configureApp") : t(app.connected ? "common.disconnect" : "common.connect");
                const label = `${action} ${appTitle(app.app)}`;
                return (
                  <button
                    key={app.app}
                    type="button"
                    title={label}
                    aria-label={label}
                    aria-pressed={manualSetup ? undefined : app.connected}
                    disabled={!!busyApp}
                    onClick={() => void toggleApp(app)}
                    className={cn(
                      "relative flex size-8 shrink-0 items-center justify-center rounded-full border transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60 disabled:cursor-wait disabled:opacity-60",
                      app.connected ? "border-emerald-500/40 bg-emerald-500/10" : "border-border/60 opacity-65 hover:border-border hover:opacity-100",
                    )}
                  >
                    {busyApp === app.app ? <LoaderCircle className="size-4 animate-spin" /> : <AppVendorIcon app={app.app} className="size-4" />}
                    {app.connected && <span aria-hidden="true" className="absolute right-0 bottom-0 size-1.5 rounded-full bg-emerald-500 ring-2 ring-card" />}
                  </button>
                );
              }) : <span className="mr-1 text-[11px] text-muted-foreground">{t("common.notConnected")}</span>}
            </div>
          </div>
          {appError && <p role="alert" className="mt-1 line-clamp-1 shrink-0 text-[10px] text-destructive">{appError}</p>}
        </section>

        <footer className="mt-2 flex h-8 shrink-0 items-center justify-between pt-1">
          <button
            type="button"
            onClick={openDashboard}
            className="flex h-7 items-center gap-1.5 rounded-lg px-0 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground"
          >
            <LayoutDashboard className="size-4" />
            <span>{t("nav.dashboard")}</span>
            <ArrowRight className="size-3" />
          </button>
          <button
            type="button"
            onClick={() => void api.trayQuit().catch(() => {})}
            className="h-7 rounded-lg px-0 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
          >
            {t("tray.quit")}
          </button>
        </footer>
      </section>
    </main>
  );
}
