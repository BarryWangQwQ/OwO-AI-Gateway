import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useApp } from "@/components/app-context";
import { ChevronRight, CircleCheck, CircleX, Link2, Link2Off, RefreshCw, Sparkles } from "@/components/icons";
import { ErrorAlert, Hint, IconButton, PageHeader } from "@/components/page";
import { AppCardSkeleton, repeat } from "@/components/skeletons";
import { Dot } from "@/components/status-badge";
import { AppVendorIcon, VendorIcon } from "@/components/vendor-icon";
import { Button } from "@/components/ui/button";
import { Card, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { REFRESH, useQuery } from "@/hooks/use-query";
import { toast } from "@/hooks/use-toast";
import { api, type ActionResult, type AppInfo } from "@/lib/api";
import { appSubtitle, appTitle } from "@/lib/format";
import { useNames } from "@/lib/names";

const DEFAULT = "__default__";

type Outcome = { app: string; connect: boolean; result: ActionResult };

/**
 * The command the CLI suggests after connecting (`Try it: codex -p owo (your normal sessions are unchanged)`),
 * split into the bare command and the optional trailing parenthetical note.
 */
function tryCommand(output: string): { cmd: string; note?: string } | undefined {
  for (const line of output.split(/\r?\n/)) {
    const m = /^\s*(?:try it|use)\s*:\s*(.+?)\s*$/i.exec(line);
    if (!m?.[1]) continue;
    const note = /^(.*?)\s*\((.+)\)$/.exec(m[1]);
    return note?.[1] ? { cmd: note[1], note: note[2].replace(/`/g, "") } : { cmd: m[1] };
  }
  return undefined;
}

/** A card hint such as `start with: codex -p owo` reduced to the command itself. */
function hintCommand(hint: string): string {
  return hint.replace(/^\s*(?:start with|try it|use|run)\s*:\s*/i, "").trim() || hint;
}

/** The first `error:` line of the CLI output, without the prefix. */
function firstError(output: string): string | undefined {
  for (const line of output.split(/\r?\n/)) {
    const m = /^\s*error\s*:\s*(.*?)\s*$/i.exec(line);
    if (m) return m[1] || undefined;
  }
  return undefined;
}

/** Friendly result dialog: one-line summary first, raw CLI output behind a "Details" toggle (open by default on failure). */
function OutcomeDialog({ outcome, onClose }: { outcome: Outcome | undefined; onClose: () => void }) {
  const { t } = useTranslation();
  // Details toggle, tracked per outcome so a fresh result resets it: collapsed on success, expanded on failure.
  const [toggle, setToggle] = useState<{ of: Outcome; open: boolean }>();
  const ok = outcome?.result.ok ?? false;
  const expanded = toggle && toggle.of === outcome ? toggle.open : !ok;
  const setOpen = (open: boolean) => outcome && setToggle({ of: outcome, open });
  const output = outcome?.result.output ?? "";
  const title = outcome ? appTitle(outcome.app) : "";
  const heading = outcome?.connect ? t("apps.outcome.connectTitle", { app: title }) : t("apps.outcome.disconnectTitle", { app: title });
  const command = outcome?.connect && ok ? tryCommand(output) : undefined;
  const summary = ok
    ? outcome?.connect
      ? t("apps.outcome.nowConnected", { app: title })
      : t("apps.outcome.disconnected", { app: title })
    : (firstError(output) ?? (outcome?.connect ? t("apps.outcome.connectFailed") : t("apps.outcome.disconnectFailed")));

  return (
    <Dialog open={!!outcome} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{heading}</DialogTitle>
          <DialogDescription className="flex items-start gap-1.5">
            {ok ? <CircleCheck className="mt-0.5 size-4 shrink-0 text-emerald-500" /> : <CircleX className="mt-0.5 size-4 shrink-0 text-destructive" />}
            <span className="min-w-0 break-words">{summary}</span>
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          {command && (
            <p className="text-sm leading-6">
              <span className="text-muted-foreground">{t("apps.outcome.tryIt")}</span>
              <code className="rounded-md bg-muted px-1.5 py-0.5 font-mono text-xs">{command.cmd}</code>
              {command.note && <span className="ml-2 text-xs text-muted-foreground">{command.note}</span>}
            </p>
          )}
          <Collapsible open={expanded} onOpenChange={setOpen}>
            <CollapsibleTrigger asChild>
              <Button variant="ghost" size="sm" className="group -ml-2 h-7 px-2 text-muted-foreground">
                <ChevronRight className="transition-transform duration-200 group-data-[state=open]:rotate-90" />
                {t("common.details")}
              </Button>
            </CollapsibleTrigger>
            <CollapsibleContent className="duration-200 ease-out data-open:animate-in data-open:fade-in-0 data-open:slide-in-from-top-1">
              <pre className="mt-2 max-h-[50vh] overflow-auto rounded-lg bg-muted p-4 font-mono text-xs leading-relaxed whitespace-pre-wrap">{output || t("common.noOutput")}</pre>
            </CollapsibleContent>
          </Collapsible>
        </div>
        <DialogFooter>
          <DialogClose asChild>
            <Button>{t("common.ok")}</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function AppIcon({ app }: { app: string }) {
  return (
    <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-muted">
      <AppVendorIcon app={app} className="size-4" />
    </span>
  );
}

export function AppsPage() {
  const { t } = useTranslation();
  const { saved } = useApp();
  const apps = useQuery(api.apps, [], { refreshInterval: REFRESH.config });
  // `names.models` is the model list for the default-model pickers.
  const names = useNames();
  const general = useQuery(api.general, [], { refreshInterval: REFRESH.config });
  const [busy, setBusy] = useState<{ app: string; connect: boolean }>();
  const [outcome, setOutcome] = useState<Outcome>();
  /** A default-model pick that is being saved; the picker shows it right away instead of the old value until `general` reloads. */
  const [picking, setPicking] = useState<{ app: string; model: string }>();
  /**
   * Skeletons until every card can render its final content in one go: the cards themselves, the default models
   * (`general`) and the model display names. Once shown, the cards stay mounted through every reload (`refreshing`);
   * connect / disconnect only flip the per-card busy state.
   */
  const loading = apps.loading || general.loading || !names.ready;
  const refreshing = apps.refreshing || general.refreshing;

  const run = async (app: AppInfo, connect: boolean) => {
    setBusy({ app: app.app, connect });
    try {
      const model = general.data?.clients[app.client_id]?.model ?? null;
      const result = connect ? await api.appConnect(app.app, app.takes_model ? model : null) : await api.appDisconnect(app.app);
      setOutcome({ app: app.app, connect, result });
      if (result.ok) toast({ title: t(connect ? "apps.toast.connected" : "apps.toast.disconnected", { app: appTitle(app.app) }) });
      await apps.reload();
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.failed"), description: String(e) });
    } finally {
      setBusy(undefined);
    }
  };

  const setModel = async (app: AppInfo, model: string) => {
    setPicking({ app: app.app, model });
    try {
      const current = general.data?.clients[app.client_id];
      await api.saveClient(app.app, model === DEFAULT ? null : model, current?.name ?? null);
      await general.reload();
      saved(app.connected ? t("apps.toast.defaultModelSavedReconnect", { app: appTitle(app.app) }) : t("apps.toast.defaultModelSaved"));
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.notSaved"), description: String(e) });
    } finally {
      setPicking(undefined);
    }
  };

  return (
    <div className="space-y-6">
      <PageHeader
        title={t("apps.title")}
        actions={<IconButton variant="default" label={t("common.refresh")} icon={<RefreshCw className={refreshing ? "animate-spin" : ""} />} onClick={() => void apps.refresh()} />}
      />
      {apps.error && <ErrorAlert title={t("apps.cannotList")} error={apps.error} />}
      <div className="grid gap-3 md:grid-cols-2 2xl:grid-cols-3">
        {loading && repeat(6, (i) => <AppCardSkeleton key={i} />)}
        {!loading &&
          apps.data?.map((a) => {
            const model = picking?.app === a.app ? picking.model : (general.data?.clients[a.client_id]?.model ?? DEFAULT);
            const spinning = (connect: boolean) => busy?.app === a.app && busy.connect === connect;
            const subtitle = appSubtitle(a.app);
            // Once connected, the CLI's launch hint (`start with: codex -p owo`) replaces the generic subtitle.
            const hint = a.connected && a.hint ? hintCommand(a.hint) : undefined;
            // The default-model trigger shows icon + display name only; the items below also carry the muted id.
            const trigger = (
              <SelectTrigger size="sm" className="min-w-0 flex-1 disabled:pointer-events-none" aria-label={t("apps.defaultModel")}>
                <SelectValue>
                  {model === DEFAULT ? (
                    <>
                      <Sparkles className="size-4 text-muted-foreground" /> {t("apps.auto")}
                    </>
                  ) : (
                    <>
                      <VendorIcon id={model} provider={names.models?.find((m) => m.id === model)?.provider} className="size-4" /> {names.model(model)}
                    </>
                  )}
                </SelectValue>
              </SelectTrigger>
            );
            return (
              <Card key={a.app} size="sm" className="gap-3">
                <CardHeader className="flex items-center gap-3">
                  <AppIcon app={a.app} />
                  <div className="min-w-0 flex-1">
                    <CardTitle className="flex items-center text-sm font-semibold leading-tight">
                      <span className="truncate">{appTitle(a.app)}</span>
                      {a.about && <Hint text={a.about} />}
                    </CardTitle>
                    {hint ? (
                      <p className="mt-0.5 truncate font-mono text-xs text-muted-foreground" title={a.hint ?? undefined}>
                        {hint}
                      </p>
                    ) : (
                      subtitle && <p className="mt-0.5 truncate text-xs text-muted-foreground">{subtitle}</p>
                    )}
                  </div>
                  {/* Always rendered so every header lays out the same; the dot is grey while disconnected. */}
                  <span className="flex h-9 shrink-0 items-center gap-1.5 self-start text-xs text-muted-foreground">
                    <Dot on={a.connected} />
                    {t(a.connected ? "apps.connected" : "common.notConnected")}
                  </span>
                </CardHeader>
                <CardFooter className="gap-2">
                  {/*
                   * Every card carries the default-model picker in the same slot so the footers line up. Apps that pick
                   * the model in their own UI (`takes_model: false`; `owo connect` rejects `--model` for them) get it
                   * disabled, with the reason in a tooltip. That tooltip hangs off a focusable wrapper because a
                   * disabled trigger gets no pointer events.
                   */}
                  <Select value={model} disabled={!a.takes_model} onValueChange={(v) => void setModel(a, v)}>
                    {a.takes_model ? (
                      trigger
                    ) : (
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <span className="flex min-w-0 flex-1 cursor-not-allowed" tabIndex={0}>
                            {trigger}
                          </span>
                        </TooltipTrigger>
                        <TooltipContent className="max-w-64">{t("apps.noDefaultModel", { app: appTitle(a.app) })}</TooltipContent>
                      </Tooltip>
                    )}
                    <SelectContent>
                      <SelectItem value={DEFAULT}>
                        <Sparkles className="size-4 text-muted-foreground" /> {t("apps.auto")}
                      </SelectItem>
                      {names.models
                        ?.filter((m) => m.available)
                        .map((m) => (
                          <SelectItem key={m.id} value={m.id} title={m.id}>
                            <VendorIcon id={m.id} provider={m.provider} className="size-4" /> {names.model(m.id)}
                            {names.modelNamed(m.id) && <span className="truncate text-xs text-muted-foreground">{m.id}</span>}
                          </SelectItem>
                        ))}
                    </SelectContent>
                  </Select>
                  {a.connected ? (
                    <>
                      <IconButton label={t("common.reconnect")} icon={<RefreshCw />} busy={spinning(true)} disabled={!!busy} onClick={() => void run(a, true)} />
                      <IconButton label={t("common.disconnect")} icon={<Link2Off />} variant="ghost" busy={spinning(false)} disabled={!!busy} onClick={() => void run(a, false)} />
                    </>
                  ) : (
                    <IconButton label={t("common.connect")} icon={<Link2 />} busy={spinning(true)} disabled={!!busy} onClick={() => void run(a, true)} />
                  )}
                </CardFooter>
              </Card>
            );
          })}
      </div>

      <OutcomeDialog outcome={outcome} onClose={() => setOutcome(undefined)} />
    </div>
  );
}
