import { useState } from "react";
import { cn } from "cn";
import { useTranslation } from "react-i18next";

import { AppToggles, type AppToggleItem } from "@/components/app-toggles";
import {
  ChevronRight,
  CircleSlash,
  Download,
  Globe,
  KeyRound,
  Link2,
  Lock,
  MoreHorizontal,
  Pencil,
  Plug,
  Plus,
  Radio,
  RefreshCw,
  Terminal,
  Trash2,
  TriangleAlert,
  Unlock,
  X,
} from "@/components/icons";
import { ChoiceTile, ErrorAlert, Hint, IconButton, PageHeader } from "@/components/page";
import { McpCardSkeleton, repeat } from "@/components/skeletons";
import { AppVendorIcon } from "@/components/vendor-icon";
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
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Empty, EmptyContent, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { REFRESH, useQuery } from "@/hooks/use-query";
import { toast } from "@/hooks/use-toast";
import { api, mcpApi, type ActionResult, type McpAppInfo, type McpFound, type McpPair, type McpServer, type McpServerView, type McpTransport } from "@/lib/api";
import { appTitle } from "@/lib/format";

const TRANSPORTS: { id: McpTransport; icon: typeof Terminal }[] = [
  { id: "stdio", icon: Terminal },
  { id: "http", icon: Globe },
  { id: "sse", icon: Radio },
];
const TRANSPORT_ICON: Record<McpTransport, typeof Terminal> = { stdio: Terminal, http: Globe, sse: Radio };
/** Chip states that need the user's attention (a dot on the chip, the reason in its tooltip). */
const ATTENTION = new Set(["outdated", "modified", "missing", "pending", "not_installed", "unsupported", "stale"]);
const NAME = /^[A-Za-z0-9_-]{1,64}$/;

/** Mirrors `owo_config::looks_secret`: names whose values are kept in the keyring by default. */
function looksSecret(key: string): boolean {
  const k = key.toUpperCase().replace(/-/g, "_");
  const marks = ["TOKEN", "SECRET", "PASSWORD", "PASSWD", "API_KEY", "APIKEY", "ACCESS_KEY", "PRIVATE_KEY", "CREDENTIAL", "AUTH", "COOKIE", "SESSION"];
  return marks.some((m) => k.includes(m)) || k.endsWith("_KEY") || k.endsWith("_PAT") || k === "KEY";
}

/** Mirrors `owo_config::mcp_keyring_name`. */
function keyringName(server: string, key: string): string {
  return `mcp-${server}-${key.replace(/[^A-Za-z0-9_.-]/g, "_")}`;
}

const isRef = (v: string) => /^(keyring|env):/.test(v.trim());

/** The first `error:` / `FAILED` line of CLI output. */
function firstError(output: string): string | undefined {
  for (const line of output.split(/\r?\n/)) {
    const m = /^\s*(?:error\s*:|\S+:\s+FAILED\s+—)\s*(.*?)\s*$/i.exec(line);
    if (m?.[1]) return m[1];
  }
  return undefined;
}

/** `https://mcp.example.com/mcp` → `mcp.example.com`; unparsable input is returned as typed. */
function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url.replace(/^\w+:\/\//, "").replace(/\/.*$/, "");
  }
}

/** What the server runs: `cmd args…` for stdio, the URL host for remote ones (`full` goes in the tooltip). */
function targetOf(s: McpServerView): { short: string; full: string } {
  if (s.transport === "stdio") {
    const full = [s.command ?? "", ...s.args].join(" ").trim() || s.summary;
    return { short: full, full };
  }
  const full = s.url ?? s.summary;
  return { short: hostOf(full), full };
}

function useAppLabel() {
  const { t } = useTranslation();
  return (app: string) => t(`mcp.appNames.${app}`, { defaultValue: appTitle(app) });
}

/** A one-line muted mono value, the full text in a tooltip. */
function MonoLine({ short, full, className }: { short: string; full: string; className?: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <p className={cn("truncate font-mono text-xs text-muted-foreground", className)}>{short}</p>
      </TooltipTrigger>
      <TooltipContent className="max-w-96 font-mono break-all">{full}</TooltipContent>
    </Tooltip>
  );
}

/** A small status icon with its explanation in a tooltip. */
function IconHint({ icon: Icon, className, title, lines }: { icon: typeof Terminal; className: string; title: string; lines?: string[] }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Icon className={cn("size-3.5 shrink-0 cursor-help", className)} aria-label={title} />
      </TooltipTrigger>
      <TooltipContent className="max-w-80 flex-col items-start gap-0.5">
        <span className="font-medium">{title}</span>
        {lines?.map((l) => (
          <span key={l} className="font-mono break-all opacity-80">
            {l}
          </span>
        ))}
      </TooltipContent>
    </Tooltip>
  );
}

// ---------------------------------------------------------------- card

/**
 * Header: round transport icon, the name (the description in its tooltip) with a key icon when env / headers are set,
 * the command or URL host under it, ⋯ top-right. Footer: one round chip per app that can run the server.
 */
function ServerCard({
  server,
  apps,
  busyApp,
  locked,
  syncing,
  onToggle,
  onSync,
  onEdit,
  onRemove,
}: {
  server: McpServer;
  apps: McpAppInfo[];
  /** The app whose toggle is being saved. */
  busyApp?: string;
  /** Another change is running; the chips wait. */
  locked: boolean;
  syncing: boolean;
  onToggle: (app: string, enabled: boolean) => void;
  onSync: () => void;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const label = useAppLabel();
  const Icon = TRANSPORT_ICON[server.transport];
  const target = targetOf(server);
  const kvs = [...server.env, ...server.headers];
  const missing = kvs.some((kv) => kv.missing);
  const needsSync = server.apps.some((a) => a.enabled && (a.state === "outdated" || a.state === "pending"));

  // An enabled app always shows (so it can be switched off); others only where the server can run.
  const toggles: AppToggleItem[] = [];
  for (const info of apps) {
    const state = server.apps.find((a) => a.app === info.app);
    const enabled = state?.enabled ?? false;
    if (!enabled && (!info.installed || info.reason || state?.unsupported || !info.transports.includes(server.transport))) continue;
    toggles.push({
      app: info.app,
      enabled,
      label: label(info.app),
      reason: (enabled && state?.unsupported) || t(`mcp.state.${state?.state ?? "off"}`),
      attention: !!state && ATTENTION.has(state.state),
      busy: busyApp === info.app,
      disabled: locked,
    });
  }

  const title = <CardTitle className="min-w-0 truncate font-semibold leading-tight">{server.name}</CardTitle>;
  return (
    <Card className="flex flex-col">
      <CardHeader>
        <div className="flex min-w-0 items-center gap-3">
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground" aria-label={t(`mcp.transport.${server.transport}`)}>
                <Icon className="size-4" />
              </span>
            </TooltipTrigger>
            <TooltipContent>{t(`mcp.transport.${server.transport}`)}</TooltipContent>
          </Tooltip>
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-2">
              {server.description ? (
                <Tooltip>
                  <TooltipTrigger asChild>{title}</TooltipTrigger>
                  <TooltipContent className="max-w-72">{server.description}</TooltipContent>
                </Tooltip>
              ) : (
                title
              )}
              {kvs.length > 0 && (
                <IconHint
                  icon={KeyRound}
                  className={missing ? "text-destructive" : "text-muted-foreground/70"}
                  title={missing ? t("mcp.keyMissing") : t(server.transport === "stdio" ? "mcp.form.env" : "mcp.form.headers")}
                  lines={kvs.map((kv) => (kv.missing ? `${kv.key}: ${kv.missing}` : kv.key))}
                />
              )}
            </div>
            <MonoLine short={target.short} full={target.full} className="mt-0.5" />
          </div>
        </div>
        <CardAction>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" aria-label={t("mcp.actions")}>
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={onEdit}>
                <Pencil /> {t("common.edit")}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem variant="destructive" onClick={onRemove}>
                <Trash2 /> {t("common.remove")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </CardAction>
      </CardHeader>
      <CardContent className="mt-auto flex min-h-7 items-center gap-2">
        <AppToggles className="min-w-0 flex-1" apps={toggles} empty={t("mcp.noApps")} onToggle={onToggle} />
        {needsSync && <IconButton variant="ghost" size="icon-sm" label={t("mcp.sync")} icon={<RefreshCw />} busy={syncing} disabled={locked} onClick={onSync} />}
      </CardContent>
    </Card>
  );
}

// ---------------------------------------------------------------- add / edit

type Row = {
  id: number;
  key: string;
  value: string;
  /** A plain-text secret the UI never sees: kept as it is until something is typed. */
  hidden: boolean;
  /** Keep the value in the keyring; `null` follows the name (`looksSecret`). */
  secret: boolean | null;
};

type Draft = {
  isNew: boolean;
  name: string;
  description: string;
  transport: McpTransport;
  command: string;
  args: string;
  cwd: string;
  url: string;
  env: Row[];
  headers: Row[];
  apps: string[];
};

let nextRow = 0;

function rowsFrom(list: McpServer["env"]): Row[] {
  return list.map((kv) => ({
    id: nextRow++,
    key: kv.key,
    value: kv.value ?? "",
    hidden: kv.value === null,
    secret: kv.value === null || kv.value.startsWith("keyring:") ? true : null,
  }));
}

function draftFrom(s?: McpServer): Draft {
  return {
    isNew: !s,
    name: s?.name ?? "",
    description: s?.description ?? "",
    transport: s?.transport ?? "stdio",
    command: s?.command ?? "",
    args: (s?.args ?? []).join("\n"),
    cwd: s?.cwd ?? "",
    url: s?.url ?? "",
    env: s ? rowsFrom(s.env) : [],
    headers: s ? rowsFrom(s.headers) : [],
    apps: s?.apps.filter((a) => a.enabled).map((a) => a.app) ?? [],
  };
}

/** A field label with its explanation behind an ⓘ. */
function Label({ htmlFor, text, hint }: { htmlFor?: string; text: string; hint?: string }) {
  return (
    <FieldLabel htmlFor={htmlFor} className="gap-0">
      {text}
      {hint && <Hint text={hint} />}
    </FieldLabel>
  );
}

function KeyValueEditor({ id, label, rows, keyPlaceholder, onChange }: { id: string; label: string; rows: Row[]; keyPlaceholder: string; onChange: (rows: Row[]) => void }) {
  const { t } = useTranslation();
  const patch = (rowId: number, p: Partial<Row>) => onChange(rows.map((r) => (r.id === rowId ? { ...r, ...p } : r)));
  return (
    <Field className="gap-2">
      <div className="flex items-center justify-between">
        <Label htmlFor={rows.length > 0 ? `${id}-0` : undefined} text={label} hint={t("mcp.form.secretHint")} />
        <IconButton
          variant="ghost"
          size="icon-sm"
          label={t("mcp.form.addRow")}
          icon={<Plus />}
          onClick={() => onChange([...rows, { id: nextRow++, key: "", value: "", hidden: false, secret: null }])}
        />
      </div>
      {rows.map((r, i) => {
        const secret = r.secret ?? looksSecret(r.key);
        return (
          <div key={r.id} className="flex items-center gap-1.5">
            <Input
              id={`${id}-${i}`}
              className="w-2/5 font-mono"
              value={r.key}
              placeholder={keyPlaceholder}
              autoComplete="off"
              spellCheck={false}
              aria-label={t("mcp.form.key")}
              onChange={(e) => patch(r.id, { key: e.target.value })}
            />
            <Input
              className="min-w-0 flex-1 font-mono"
              type={secret && !isRef(r.value) ? "password" : "text"}
              value={r.value}
              placeholder={r.hidden ? t("mcp.form.hiddenValue") : t("mcp.form.value")}
              autoComplete="off"
              spellCheck={false}
              aria-label={t("mcp.form.value")}
              onChange={(e) => patch(r.id, { value: e.target.value })}
            />
            <IconButton
              variant="ghost"
              size="icon-sm"
              label={secret ? t("mcp.form.secretOn") : t("mcp.form.secretOff")}
              icon={secret ? <Lock className="text-emerald-600 dark:text-emerald-400" /> : <Unlock className="text-muted-foreground" />}
              onClick={() => patch(r.id, { secret: !secret })}
            />
            <IconButton variant="ghost" size="icon-sm" label={t("mcp.form.removeRow")} icon={<X />} onClick={() => onChange(rows.filter((x) => x.id !== r.id))} />
          </div>
        );
      })}
    </Field>
  );
}

// ---------------------------------------------------------------- page

type Failure = { title: string; output: string; retry?: () => Promise<void> };

type Group = { name: string; entries: McpFound[]; importable: boolean; reason?: string };

export function McpPage() {
  const { t } = useTranslation();
  const label = useAppLabel();
  const list = useQuery(mcpApi.list, [], { refreshInterval: REFRESH.config, empty: (l) => l.servers.length === 0 });
  const found = useQuery(mcpApi.scan, [], { refreshInterval: REFRESH.config });
  const [draft, setDraft] = useState<Draft>();
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState<McpServer>();
  const [toggling, setToggling] = useState<{ name: string; app: string }>();
  const [failure, setFailure] = useState<Failure>();
  const [forcing, setForcing] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [keyring, setKeyring] = useState(true);
  /** The server being imported, or `*` for all of them. */
  const [importBusy, setImportBusy] = useState<string>();
  const [syncing, setSyncing] = useState(false);

  const apps = (list.data?.apps ?? []).filter((a) => a.supported);
  const servers = list.data?.servers ?? [];
  const groups: Group[] = [];
  for (const f of found.data ?? []) {
    if (f.managed || !f.name) continue;
    let g = groups.find((x) => x.name === f.name);
    if (!g) groups.push((g = { name: f.name, entries: [], importable: false }));
    g.entries.push(f);
  }
  for (const g of groups) {
    const usable = g.entries.filter((e) => e.server && !e.error && e.relation !== "different");
    g.importable = usable.length > 0;
    if (!g.importable) g.reason = g.entries.find((e) => e.error)?.error ?? t("mcp.importDialog.different");
  }
  const importable = groups.filter((g) => g.importable);
  const importSecrets = importable.some((g) => g.entries.some((e) => e.secrets.length > 0));

  const reload = async () => {
    await Promise.all([list.reload(), found.reload()]);
  };

  /** Runs a CLI action; a failure opens the details, with a Force retry when the CLI suggests one. */
  const act = async (title: string, call: (force: boolean) => Promise<ActionResult>, success: string): Promise<boolean> => {
    try {
      const result = await call(false);
      await reload();
      if (result.ok) {
        toast({ title: success });
        return true;
      }
      const retry = result.output.includes("--force")
        ? async () => {
            const forced = await call(true);
            await reload();
            if (forced.ok) toast({ title: success });
            else setFailure({ title, output: forced.output });
          }
        : undefined;
      setFailure({ title, output: result.output, retry });
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.failed"), description: String(e) });
    }
    return false;
  };

  const toggle = async (s: McpServer, app: string, enabled: boolean) => {
    setToggling({ name: s.name, app });
    try {
      const vars = { name: s.name, app: label(app) };
      await act(
        t(enabled ? "mcp.toast.enableFailed" : "mcp.toast.disableFailed", vars),
        (force) => mcpApi.toggle(s.name, app, enabled, force),
        t(enabled ? "mcp.toast.enabled" : "mcp.toast.disabled", vars),
      );
    } finally {
      setToggling(undefined);
    }
  };

  const sync = async () => {
    setSyncing(true);
    try {
      await act(t("mcp.toast.syncFailed"), (force) => mcpApi.sync(force), t("mcp.toast.synced"));
    } finally {
      setSyncing(false);
    }
  };

  const set = (patch: Partial<Draft>) => setDraft((d) => (d ? { ...d, ...patch } : d));

  const save = async (d: Draft) => {
    setSaving(true);
    try {
      const name = d.name.trim();
      const pairs = async (rows: Row[]): Promise<McpPair[]> => {
        const out: McpPair[] = [];
        for (const r of rows) {
          const key = r.key.trim();
          if (!key) continue;
          if (r.hidden && !r.value) {
            out.push({ key, value: null });
            continue;
          }
          let value = r.value.trim();
          // A secret goes into the keyring first; only its `keyring:` reference reaches config.toml and the CLI.
          if ((r.secret ?? looksSecret(key)) && value && !isRef(value)) value = await api.setKey(keyringName(name, key), value);
          out.push({ key, value });
        }
        return out;
      };
      const stdio = d.transport === "stdio";
      const edit = {
        name,
        replace: !d.isNew,
        transport: d.transport,
        description: d.description.trim() || null,
        command: stdio ? d.command.trim() : null,
        args: stdio ? d.args.split("\n").map((a) => a.trim()).filter(Boolean) : [],
        env: stdio ? await pairs(d.env) : [],
        cwd: stdio ? d.cwd.trim() || null : null,
        url: stdio ? null : d.url.trim(),
        headers: stdio ? [] : await pairs(d.headers),
        apps: d.apps,
      };
      const ok = await act(t("mcp.toast.notSaved", { name }), (force) => mcpApi.save({ ...edit, force }), t("toast.saved", { id: name }));
      if (ok) setDraft(undefined);
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.notSaved"), description: e instanceof Error ? e.message : String(e) });
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    const s = deleting;
    if (!s) return;
    setDeleting(undefined);
    await act(t("mcp.toast.notRemoved", { name: s.name }), (force) => mcpApi.remove(s.name, force), t("toast.removed", { id: s.name }));
  };

  const runImport = async (names: string[]) => {
    if (names.length === 0) return;
    setImportBusy(names.length === 1 ? names[0] : "*");
    try {
      const chosen = groups.filter((g) => names.includes(g.name));
      // Settings OwO AI Gateway cannot keep are flagged on each row, so importing it accepts dropping them.
      const force = chosen.some((g) => g.entries.some((e) => e.dropped.length > 0));
      const secrets = chosen.some((g) => g.entries.some((e) => e.secrets.length > 0));
      await act(t("mcp.toast.importFailed"), () => mcpApi.import("all", names, keyring && secrets, force), t("mcp.toast.imported", { count: names.length }));
    } finally {
      setImportBusy(undefined);
    }
  };

  const d = draft;
  const nameTaken = !!d?.isNew && servers.some((s) => s.name === d.name.trim());
  const nameOk = !!d && NAME.test(d.name.trim()) && !nameTaken;
  const urlOk = !!d && /^https?:\/\/\S+$/.test(d.url.trim());
  const valid = !!d && nameOk && (d.transport === "stdio" ? !!d.command.trim() : urlOk);
  const loading = list.loading;
  const refreshing = list.refreshing || found.refreshing;
  const locked = !!toggling || saving || !!importBusy || syncing;

  // Draft chips: the apps that can run the draft, plus any already picked (so they can be unpicked).
  const draftToggles: AppToggleItem[] = [];
  if (d) {
    for (const a of apps) {
      const on = d.apps.includes(a.app);
      const unsupported = !a.transports.includes(d.transport)
        ? t("mcp.form.transportUnsupported")
        : d.transport === "stdio" && d.cwd.trim() && !a.cwd
          ? t("mcp.form.cwdUnsupported")
          : null;
      if (!on && (unsupported || !a.installed || a.reason)) continue;
      const reason = unsupported ?? (!a.installed ? t("mcp.notInstalled") : undefined);
      draftToggles.push({ app: a.app, enabled: on, label: label(a.app), reason, attention: on && !!reason });
    }
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title={t("mcp.title")}
        actions={
          <>
            <IconButton variant="default" label={t("common.refresh")} icon={<RefreshCw className={refreshing ? "animate-spin" : ""} />} onClick={() => void Promise.all([list.refresh(), found.refresh()])} />
            <span className="relative inline-flex">
              <IconButton variant="default" label={t("mcp.import")} icon={<Download />} onClick={() => setImportOpen(true)} />
              {importable.length > 0 && (
                <span className="pointer-events-none absolute -top-1.5 -right-1.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-amber-500 px-1 text-[10px] font-semibold leading-none text-white tabular-nums ring-2 ring-background">
                  {importable.length}
                </span>
              )}
            </span>
            <IconButton variant="default" label={t("mcp.add")} icon={<Plus />} onClick={() => setDraft(draftFrom())} />
          </>
        }
      />
      {list.error && <ErrorAlert title={t("mcp.cannotList")} error={list.error} />}

      {!loading && !list.error && servers.length === 0 && (
        <Empty className="border">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <Plug />
            </EmptyMedia>
            <EmptyTitle>{t("mcp.empty")}</EmptyTitle>
          </EmptyHeader>
          <EmptyContent>
            <Button onClick={() => setDraft(draftFrom())}>
              <Plus /> {t("mcp.add")}
            </Button>
          </EmptyContent>
        </Empty>
      )}

      <div className="grid gap-4 md:grid-cols-2 2xl:grid-cols-3">
        {loading && repeat(3, (i) => <McpCardSkeleton key={i} />)}
        {!loading &&
          servers.map((s) => (
            <ServerCard
              key={s.name}
              server={s}
              apps={apps}
              busyApp={toggling?.name === s.name ? toggling.app : undefined}
              locked={locked}
              syncing={syncing}
              onToggle={(app, enabled) => void toggle(s, app, enabled)}
              onSync={() => void sync()}
              onEdit={() => setDraft(draftFrom(s))}
              onRemove={() => setDeleting(s)}
            />
          ))}
      </div>

      {/* Add / edit */}
      <Dialog open={!!draft} onOpenChange={(open) => !open && setDraft(undefined)}>
        <DialogContent className="sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>{d?.isNew ? t("mcp.dialog.addTitle") : t("mcp.dialog.editTitle", { name: d?.name })}</DialogTitle>
            <DialogDescription className="sr-only">{t("mcp.dialog.description")}</DialogDescription>
          </DialogHeader>
          {d && (
            <div className="-m-1 grid max-h-[65vh] gap-4 overflow-y-auto p-1">
              <div role="group" aria-label={t("mcp.form.transport")} className="grid grid-cols-3 gap-2">
                {TRANSPORTS.map(({ id, icon: Icon }) => (
                  <ChoiceTile
                    key={id}
                    className="gap-1 p-2"
                    selected={d.transport === id}
                    icon={<Icon className="size-5 text-muted-foreground" />}
                    label={t(`mcp.transport.${id}`)}
                    onSelect={() => set({ transport: id })}
                  />
                ))}
              </div>

              <div className="grid grid-cols-2 gap-3">
                <Field className="gap-2">
                  <Label htmlFor="mcp-name" text={t("mcp.form.name")} hint={t("mcp.form.nameHint")} />
                  <Input
                    id="mcp-name"
                    className="font-mono"
                    value={d.name}
                    disabled={!d.isNew}
                    placeholder="github"
                    autoComplete="off"
                    spellCheck={false}
                    aria-invalid={!!d.name && !nameOk}
                    onChange={(e) => set({ name: e.target.value })}
                  />
                  {nameTaken && <FieldDescription className="text-destructive">{t("mcp.form.nameTaken")}</FieldDescription>}
                </Field>
                <Field className="gap-2">
                  <Label htmlFor="mcp-description" text={t("mcp.form.description")} />
                  <Input id="mcp-description" value={d.description} placeholder={t("common.optional")} autoComplete="off" onChange={(e) => set({ description: e.target.value })} />
                </Field>
              </div>

              {d.transport === "stdio" ? (
                <>
                  <div className="grid grid-cols-2 gap-3">
                    <Field className="gap-2">
                      <Label htmlFor="mcp-command" text={t("mcp.form.command")} />
                      <Input id="mcp-command" className="font-mono" value={d.command} placeholder="npx" autoComplete="off" spellCheck={false} onChange={(e) => set({ command: e.target.value })} />
                    </Field>
                    <Field className="gap-2">
                      <Label htmlFor="mcp-cwd" text={t("mcp.form.cwd")} hint={t("mcp.form.cwdHint")} />
                      <Input id="mcp-cwd" className="font-mono" value={d.cwd} placeholder={t("common.optional")} autoComplete="off" spellCheck={false} onChange={(e) => set({ cwd: e.target.value })} />
                    </Field>
                  </div>
                  <Field className="gap-2">
                    <Label htmlFor="mcp-args" text={t("mcp.form.args")} hint={t("mcp.form.argsHint")} />
                    <Textarea
                      id="mcp-args"
                      className="min-h-14 font-mono"
                      rows={2}
                      value={d.args}
                      placeholder={"-y\n@modelcontextprotocol/server-github"}
                      spellCheck={false}
                      onChange={(e) => set({ args: e.target.value })}
                    />
                  </Field>
                  <KeyValueEditor id="mcp-env" label={t("mcp.form.env")} rows={d.env} keyPlaceholder="GITHUB_TOKEN" onChange={(env) => set({ env })} />
                </>
              ) : (
                <>
                  <Field className="gap-2">
                    <Label htmlFor="mcp-url" text={t("mcp.form.url")} />
                    <Input
                      id="mcp-url"
                      className="font-mono"
                      value={d.url}
                      placeholder={d.transport === "sse" ? "http://127.0.0.1:9000/sse" : "https://mcp.example.com/mcp"}
                      autoComplete="off"
                      spellCheck={false}
                      aria-invalid={!!d.url && !urlOk}
                      onChange={(e) => set({ url: e.target.value })}
                    />
                  </Field>
                  <KeyValueEditor id="mcp-headers" label={t("mcp.form.headers")} rows={d.headers} keyPlaceholder="Authorization" onChange={(headers) => set({ headers })} />
                </>
              )}

              <Field className="gap-2">
                <Label text={t("mcp.form.apps")} hint={t("mcp.form.appsHint")} />
                <AppToggles
                  apps={draftToggles}
                  empty={t("mcp.noApps")}
                  onToggle={(app, next) => set({ apps: next ? [...d.apps, app] : d.apps.filter((x) => x !== app) })}
                />
              </Field>
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" className="sm:mr-auto" onClick={() => setDraft(undefined)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => d && void save(d)} disabled={saving || !valid}>
              {saving && <Spinner />}
              {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Import: servers the apps have that OwO AI Gateway does not manage yet. */}
      <Dialog open={importOpen} onOpenChange={(open) => !open && !importBusy && setImportOpen(false)}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle className="flex items-center">
              {t("mcp.import")}
              <Hint text={t("mcp.importDialog.hint")} />
            </DialogTitle>
            <DialogDescription className="sr-only">{t("mcp.importDialog.hint")}</DialogDescription>
          </DialogHeader>
          {found.loading ? (
            <div className="flex justify-center py-6">
              <Spinner />
            </div>
          ) : groups.length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">{t("mcp.importDialog.empty")}</p>
          ) : (
            <div className="-m-1 max-h-[55vh] divide-y overflow-y-auto p-1">
              {groups.map((g) => {
                const first = g.entries.find((e) => e.server) ?? g.entries[0];
                const target = first?.server ? targetOf(first.server) : undefined;
                const secrets = [...new Set(g.entries.flatMap((e) => e.secrets))];
                const dropped = [...new Set(g.entries.flatMap((e) => e.dropped))];
                return (
                  <div key={g.name} className="flex items-center gap-3 py-2.5">
                    <div className={cn("flex shrink-0 items-center -space-x-1.5", !g.importable && "opacity-50")}>
                      {g.entries.map((e) => (
                        <Tooltip key={`${e.app}-${e.file}`}>
                          <TooltipTrigger asChild>
                            <span className="flex size-7 items-center justify-center rounded-full border-2 border-background bg-muted">
                              <AppVendorIcon app={e.app} className="size-3.5" />
                            </span>
                          </TooltipTrigger>
                          <TooltipContent className="max-w-96 flex-col items-start gap-0.5">
                            <span className="font-medium">{label(e.app)}</span>
                            <span className="font-mono break-all opacity-80">{e.file}</span>
                          </TooltipContent>
                        </Tooltip>
                      ))}
                    </div>
                    <div className={cn("min-w-0 flex-1", !g.importable && "opacity-50")}>
                      <div className="flex min-w-0 items-center gap-1.5">
                        <span className="truncate text-sm font-medium">{g.name}</span>
                        {first?.relation === "same" && <IconHint icon={Link2} className="text-muted-foreground/70" title={t("mcp.importDialog.same")} />}
                        {secrets.length > 0 && <IconHint icon={KeyRound} className="text-muted-foreground/70" title={t("mcp.importDialog.secrets")} lines={secrets} />}
                        {g.importable && dropped.length > 0 && <IconHint icon={TriangleAlert} className="text-amber-500" title={t("mcp.importDialog.dropped")} lines={dropped} />}
                      </div>
                      {target && <MonoLine short={target.short} full={target.full} />}
                    </div>
                    {g.importable ? (
                      <IconButton
                        variant="ghost"
                        label={t("mcp.importDialog.import")}
                        icon={<Download />}
                        busy={importBusy === g.name}
                        disabled={locked}
                        onClick={() => void runImport([g.name])}
                      />
                    ) : (
                      <span className="flex size-8 shrink-0 items-center justify-center">
                        <IconHint icon={CircleSlash} className="size-4 text-muted-foreground" title={g.reason ?? ""} />
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          )}
          {importable.length > 0 && (
            <DialogFooter className="items-center">
              {importSecrets && (
                <label className="flex items-center gap-2 text-sm sm:mr-auto">
                  <Switch checked={keyring} onCheckedChange={setKeyring} />
                  <Lock className="size-3.5 text-muted-foreground" />
                  {t("mcp.importDialog.keyring")}
                </label>
              )}
              {importable.length > 1 && (
                <Button disabled={locked} onClick={() => void runImport(importable.map((g) => g.name))}>
                  {importBusy === "*" ? <Spinner /> : <Download />}
                  {t("mcp.importDialog.importAll")}
                </Button>
              )}
            </DialogFooter>
          )}
        </DialogContent>
      </Dialog>

      {/* Remove */}
      <AlertDialog open={!!deleting} onOpenChange={(open) => !open && setDeleting(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("mcp.removeTitle", { name: deleting?.name })}</AlertDialogTitle>
            <AlertDialogDescription>{t("mcp.removeDescription")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => void remove()}>
              {t("common.remove")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* A failed action: the CLI's error line, its full output folded away, and a forced retry when it offers one. */}
      <Dialog open={!!failure} onOpenChange={(open) => !open && !forcing && setFailure(undefined)}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{failure?.title}</DialogTitle>
            <DialogDescription className="flex items-start gap-1.5">
              <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
              <span className="min-w-0 break-words">{(failure && firstError(failure.output)) ?? t("toast.unknownError")}</span>
            </DialogDescription>
          </DialogHeader>
          <Collapsible>
            <CollapsibleTrigger className="group flex items-center gap-1 text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:text-foreground">
              <ChevronRight className="size-3.5 transition-transform group-data-[state=open]:rotate-90" />
              {t("common.details")}
            </CollapsibleTrigger>
            <CollapsibleContent>
              <pre className="mt-2 max-h-[40vh] overflow-auto rounded-lg bg-muted p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap">{failure?.output || t("common.noOutput")}</pre>
            </CollapsibleContent>
          </Collapsible>
          <DialogFooter>
            <Button variant="outline" className="sm:mr-auto" disabled={forcing} onClick={() => setFailure(undefined)}>
              {t("common.close")}
            </Button>
            {failure?.retry && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="destructive"
                    disabled={forcing}
                    onClick={() => {
                      const retry = failure.retry;
                      setForcing(true);
                      setFailure(undefined);
                      void retry?.().finally(() => setForcing(false));
                    }}
                  >
                    {forcing && <Spinner />}
                    {t("mcp.failure.force")}
                  </Button>
                </TooltipTrigger>
                <TooltipContent className="max-w-64">{t("mcp.failure.forceHint")}</TooltipContent>
              </Tooltip>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
