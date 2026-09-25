import { useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";

import { useApp } from "@/components/app-context";
import { SettingRow } from "@/components/page";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { toast } from "@/hooks/use-toast";
import { api, type AppInfo } from "@/lib/api";
import { appTitle } from "@/lib/format";

type Action = "history" | "config" | "apps";

/** The word the user has to type before config.toml can be reset. */
const RESET_WORD = "reset";

/**
 * Maintenance actions that throw data away, each behind a confirmation. `onChanged` runs after
 * one of them succeeds so the page can reload what it shows (the config editor in particular).
 */
export function DangerZone({ onChanged }: { onChanged: () => Promise<unknown> }) {
  const { t } = useTranslation();
  const { status, saved, refreshStatus } = useApp();
  const [open, setOpen] = useState<Action>();
  const [busy, setBusy] = useState<Action>();
  const [confirmation, setConfirmation] = useState("");
  const [lastBackup, setLastBackup] = useState<string>();
  const [apps, setApps] = useState<AppInfo[]>();
  const [appsError, setAppsError] = useState<string>();

  // The disconnect dialog lists what it is about to touch, so it fetches a fresh list each time.
  useEffect(() => {
    if (open !== "apps") return;
    setApps(undefined);
    setAppsError(undefined);
    api
      .apps()
      .then(setApps)
      .catch((e) => setAppsError(String(e)));
  }, [open]);

  const connected = apps?.filter((a) => a.connected) ?? [];
  const backupsPath = status?.configBackupsPath ?? "…";

  const finish = async () => {
    await Promise.all([refreshStatus(), onChanged()]);
  };

  const run = async (action: Action, work: () => Promise<void>) => {
    setOpen(undefined);
    setBusy(action);
    try {
      await work();
      await finish();
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.failed"), description: String(e) });
    } finally {
      setBusy(undefined);
    }
  };

  const clearHistory = () =>
    run("history", async () => {
      const deleted = await api.clearHistory();
      toast({ title: t("settings.reset.historyCleared"), description: t("settings.reset.historyClearedDetail", { count: deleted }) });
    });

  const resetConfig = () =>
    run("config", async () => {
      const backup = await api.resetConfig();
      setLastBackup(backup ?? undefined);
      saved(t("settings.reset.configReset"));
    });

  const disconnectAll = () =>
    run("apps", async () => {
      const outcomes = await api.disconnectAll();
      const failed = outcomes.filter((o) => !o.ok);
      if (failed.length === 0) {
        toast({ title: t("settings.reset.appsDisconnected", { count: outcomes.length }) });
      } else {
        const detail = failed.map((o) => `${appTitle(o.app)}: ${o.output.split("\n").find((l) => l.trim()) ?? t("toast.unknownError")}`);
        toast({
          variant: "destructive",
          title: t("settings.reset.appsDisconnectFailed", { count: failed.length }),
          description: detail.join("\n"),
        });
      }
    });

  const openDialog = (action: Action) => {
    setConfirmation("");
    setOpen(action);
  };
  const closeDialog = (isOpen: boolean) => !isOpen && setOpen(undefined);
  const destructiveOutline = "border-destructive/40 text-destructive hover:bg-destructive/10 hover:text-destructive";

  return (
    <Card className="border border-destructive/30 ring-0">
      <CardContent>
        <SettingRow title={t("settings.reset.clearHistory")} description={t("settings.reset.clearHistoryDescription")}>
          <Button size="sm" variant="outline" className={destructiveOutline} disabled={!!busy} onClick={() => openDialog("history")}>
            {t("settings.reset.actions.clear")}
          </Button>
        </SettingRow>
        <Separator />
        <SettingRow title={t("settings.reset.disconnectAll")} description={t("settings.reset.disconnectAllDescription")}>
          <Button size="sm" variant="outline" className={destructiveOutline} disabled={!!busy} onClick={() => openDialog("apps")}>
            {t("settings.reset.actions.disconnect")}
          </Button>
        </SettingRow>
        <Separator />
        <SettingRow
          title={t("settings.reset.resetConfig")}
          description={
            <>
              {t("settings.reset.resetConfigDescription")}
              {lastBackup && (
                <>
                  <br />
                  <span className="font-mono text-xs">{t("settings.reset.backupWritten", { path: lastBackup })}</span>
                </>
              )}
            </>
          }
        >
          <Button size="sm" variant="outline" className={destructiveOutline} disabled={!!busy} onClick={() => openDialog("config")}>
            {t("settings.reset.actions.reset")}
          </Button>
        </SettingRow>
      </CardContent>

      <AlertDialog open={open === "history"} onOpenChange={closeDialog}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("settings.reset.clearHistoryTitle")}</AlertDialogTitle>
            <AlertDialogDescription>{t("settings.reset.clearHistoryConfirm")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => void clearHistory()}>
              {t("settings.reset.clearHistoryButton")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={open === "apps"} onOpenChange={closeDialog}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("settings.reset.disconnectAllTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {apps === undefined && !appsError
                ? t("common.loading")
                : connected.length === 0
                  ? t("settings.reset.noConnectedApps")
                  : t("settings.reset.disconnectAllConfirm", { count: connected.length })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {appsError && <p className="whitespace-pre-wrap font-mono text-xs text-destructive">{appsError}</p>}
          {connected.length > 0 && (
            <ul className="grid gap-1 text-sm">
              {connected.map((a) => (
                <li key={a.app} className="flex items-baseline gap-2">
                  <span className="font-medium">{appTitle(a.app)}</span>
                  <span className="truncate text-muted-foreground">{a.about}</span>
                </li>
              ))}
            </ul>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction variant="destructive" disabled={connected.length === 0} onClick={() => void disconnectAll()}>
              {t("settings.reset.disconnectAllButton")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={open === "config"} onOpenChange={closeDialog}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("settings.reset.resetConfigTitle")}</AlertDialogTitle>
            <AlertDialogDescription>{t("settings.reset.resetConfigConfirm")}</AlertDialogDescription>
          </AlertDialogHeader>
          <ul className="grid list-disc gap-1.5 pl-5 text-sm text-muted-foreground">
            <li>{t("settings.reset.resetConfigLoses")}</li>
            <li>
              <Trans
                i18nKey="settings.reset.resetConfigBackup"
                values={{ path: backupsPath }}
                components={{ path: <span className="break-all font-mono text-xs text-foreground" /> }}
              />
            </li>
            <li>{t("settings.reset.resetConfigKeyring")}</li>
            <li>{t("settings.reset.resetConfigApps")}</li>
            <li>{t("settings.reset.resetConfigRestart")}</li>
          </ul>
          <div className="grid gap-2">
            <label htmlFor="danger-reset-confirm" className="text-sm">
              <Trans i18nKey="settings.reset.typeToConfirm" values={{ word: RESET_WORD }} components={{ code: <span className="font-mono font-medium text-foreground" /> }} />
            </label>
            <Input
              id="danger-reset-confirm"
              value={confirmation}
              autoComplete="off"
              spellCheck={false}
              placeholder={RESET_WORD}
              className="font-mono"
              onChange={(e) => setConfirmation(e.target.value)}
            />
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction variant="destructive" disabled={confirmation.trim() !== RESET_WORD} onClick={() => void resetConfig()}>
              {t("settings.reset.resetConfigButton")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}
