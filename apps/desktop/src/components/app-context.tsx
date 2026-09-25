import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import i18next from "i18next";

import { ToastAction } from "@/components/ui/toast";
import { readAutoStartGateway } from "@/hooks/use-auto-start-gateway";
import { REFRESH, useQuery } from "@/hooks/use-query";
import { toast } from "@/hooks/use-toast";
import { api, type ActionResult, type Status } from "@/lib/api";

export type Page = "dashboard" | "usage" | "history" | "models" | "providers" | "apps" | "mcp" | "skills" | "settings";

type GatewayAction = "start" | "stop" | "restart";

type AppState = {
  page: Page;
  navigate: (page: Page) => void;
  status?: Status;
  refreshStatus: () => Promise<void>;
  gatewayBusy: boolean;
  gateway: (action: GatewayAction) => Promise<void>;
  /** Shows the outcome of a config change and offers a restart when the gateway runs. */
  saved: (what: string) => void;
};

const Context = createContext<AppState | null>(null);

export function useApp(): AppState {
  const ctx = useContext(Context);
  if (!ctx) throw new Error("useApp outside AppProvider");
  return ctx;
}

/** Toasts the first useful line of a CLI result. */
export function toastResult(result: ActionResult, success: string) {
  const lines = result.output.split("\n").filter((l) => l.trim());
  if (result.ok) {
    toast({ title: success, description: lines.slice(0, 2).join(" · ") || undefined });
  } else {
    toast({
      variant: "destructive",
      title: i18next.t("toast.failed"),
      description: lines.find((l) => l.startsWith("error")) ?? lines.at(-1) ?? i18next.t("toast.unknownError"),
    });
  }
}

export function AppProvider({ children }: { children: ReactNode }) {
  const [page, setPage] = useState<Page>("dashboard");
  const [gatewayBusy, setGatewayBusy] = useState(false);
  // Polled while the window is visible; the last good status stays put when a poll fails.
  const { data: status, error: statusError, reload: refreshStatus } = useQuery(api.status, [], { refreshInterval: REFRESH.status });

  // Once per distinct failure, not on every tick.
  useEffect(() => {
    if (statusError) toast({ variant: "destructive", title: i18next.t("toast.cannotReadState"), description: statusError });
  }, [statusError]);

  const gateway = useCallback(
    async (action: GatewayAction) => {
      setGatewayBusy(true);
      try {
        const run = { start: api.gatewayStart, stop: api.gatewayStop, restart: api.gatewayRestart }[action];
        const label = { start: "toast.gatewayStarted", stop: "toast.gatewayStopped", restart: "toast.gatewayRestarted" }[action];
        toastResult(await run(), i18next.t(label));
      } catch (e) {
        toast({ variant: "destructive", title: i18next.t("toast.failed"), description: String(e) });
      } finally {
        setGatewayBusy(false);
        await refreshStatus();
      }
    },
    [refreshStatus],
  );

  // "Start gateway on launch": decided once, on the first status that arrives, so a later manual stop is respected.
  const autoStartDecided = useRef(false);
  useEffect(() => {
    if (!status || autoStartDecided.current) return;
    autoStartDecided.current = true;
    if (readAutoStartGateway() && status.configExists && !status.gatewayRunning) void gateway("start");
  }, [status, gateway]);

  const saved = useCallback(
    (what: string) => {
      if (status?.gatewayRunning) {
        toast({
          title: what,
          description: i18next.t("toast.restartToApply"),
          action: (
            <ToastAction altText={i18next.t("toast.restartGateway")} onClick={() => void gateway("restart")}>
              {i18next.t("toast.restart")}
            </ToastAction>
          ),
        });
      } else {
        toast({ title: what });
      }
      void refreshStatus();
    },
    [status?.gatewayRunning, gateway, refreshStatus],
  );

  return (
    <Context.Provider value={{ page, navigate: setPage, status, refreshStatus, gatewayBusy, gateway, saved }}>
      {children}
    </Context.Provider>
  );
}
