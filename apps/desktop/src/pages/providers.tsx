import { useState } from "react";
import { cn } from "cn";
import { Trans, useTranslation } from "react-i18next";

import { useApp } from "@/components/app-context";
import { Globe, KeyRound, MoreHorizontal, Pencil, Plus, Trash2 } from "@/components/icons";
import { ModelIdList, parseModelIds } from "@/components/model-id-list";
import { ChoiceTile, ErrorAlert, IconButton, PageHeader } from "@/components/page";
import { ProviderCardSkeleton, repeat } from "@/components/skeletons";
import { VendorIcon } from "@/components/vendor-icon";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Stepper } from "@/components/ui/stepper";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { REFRESH, useQuery } from "@/hooks/use-query";
import { toast } from "@/hooks/use-toast";
import { api, type Preset, type ProviderEdit, type ProviderInfo } from "@/lib/api";

const CUSTOM = "__custom__";
type KeyMode = "keep" | "keyring" | "env" | "inline" | "none";

/** Wizard steps; `providers.steps.*` holds the labels. */
const STEPS = ["choose", "connection", "authentication"] as const;

/** Adapter id → `providers.adapters.*` label key. */
const ADAPTERS: Record<string, string> = { "openai-chat": "providers.adapters.openaiChat", anthropic: "providers.adapters.anthropic" };
const SHOWN_MODELS = 3;
/** Preset ids shown first in the wizard, in this order; the rest follow alphabetically by name. */
const FEATURED = ["openai", "anthropic", "google", "deepseek", "openrouter", "xai", "moonshot", "zhipu-bigmodel"];

function sortPresets(list: Preset[]): Preset[] {
  const rank = (p: Preset) => {
    const i = FEATURED.indexOf(p.id);
    return i < 0 ? FEATURED.length : i;
  };
  return [...list].sort((a, b) => rank(a) - rank(b) || a.displayName.localeCompare(b.displayName));
}

/** `https://api.example.com/v1/` → `api.example.com`; unparsable input is returned as typed. */
function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url.replace(/^\w+:\/\//, "").replace(/\/.*$/, "");
  }
}

/** Icon-only key state; the tooltip carries the reference or the error. */
function KeyState({ p }: { p: ProviderInfo }) {
  const { t } = useTranslation();
  const [color, label] =
    p.keyStatus === "ok"
      ? ["text-emerald-500", p.raw?.inlineKey ? t("providers.key.inConfig") : t("providers.key.ref", { ref: p.apiKey })]
      : p.keyStatus === "missing"
        ? ["text-destructive", p.keyMessage ?? t("providers.key.missing")]
        : ["text-muted-foreground/50", t("providers.key.none")];
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <KeyRound className={cn("size-3.5 shrink-0 cursor-help", color)} aria-label={label} />
      </TooltipTrigger>
      <TooltipContent className="max-w-72 font-mono">{label}</TooltipContent>
    </Tooltip>
  );
}

/** Trailing-slash-insensitive URL equality, for "did this provider override its preset's base URL". */
function sameUrl(a: string, b: string): boolean {
  return a.replace(/\/+$/, "") === b.replace(/\/+$/, "");
}

/**
 * Title row: name, the id only when it says something the name doesn't, key state.
 * Detail row (only when there is one): the protocol for custom endpoints, the host when it overrides the preset default.
 * Bottom row: the model badges, then the enabled switch bottom-right under the ⋯ button; a disabled provider dims its content but never the switch.
 */
function ProviderCard({
  p,
  presetUrl,
  busy,
  onEdit,
  onToggle,
  onRemove,
}: {
  p: ProviderInfo;
  /** The preset's default base URL, when the preset is known to the UI. */
  presetUrl?: string;
  /** A toggle for this provider is being saved; the switch waits for it. */
  busy?: boolean;
  onEdit: () => void;
  onToggle: (enabled: boolean) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const shown = p.models.slice(0, SHOWN_MODELS);
  const rest = p.models.slice(SHOWN_MODELS);
  const showId = p.id !== p.preset && p.id.toLowerCase() !== p.displayName.toLowerCase();
  const showAdapter = !p.preset;
  const showUrl = !p.preset || (presetUrl ? !sameUrl(p.baseUrl, presetUrl) : p.raw?.baseUrl != null);
  const dim = !p.enabled && "opacity-60";
  const stateLabel = p.enabled ? t("common.enabled") : t("common.disabled");
  return (
    <Card className="flex flex-col">
      <CardHeader>
        <div className={cn("flex min-w-0 items-center gap-3", dim)}>
          <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-muted">
            <VendorIcon provider={p.preset ?? p.id} className="size-4" />
          </span>
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-2">
              <CardTitle className="min-w-0 truncate font-semibold leading-tight">{p.displayName}</CardTitle>
              {showId && <span className="truncate font-mono text-xs text-muted-foreground">{p.id}</span>}
              <KeyState p={p} />
            </div>
            {(showAdapter || showUrl) && (
              <div className="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground">
                {showAdapter && <span className="shrink-0">{ADAPTERS[p.adapter] ? t(ADAPTERS[p.adapter]) : p.adapter}</span>}
                {showAdapter && showUrl && <span aria-hidden>·</span>}
                {showUrl && (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span className="min-w-0 truncate">{hostOf(p.baseUrl)}</span>
                    </TooltipTrigger>
                    <TooltipContent className="font-mono">{p.baseUrl}</TooltipContent>
                  </Tooltip>
                )}
              </div>
            )}
          </div>
        </div>
        <CardAction>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" aria-label={t("providers.actions")}>
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
      {/* Bottom row: models on the left, the enabled switch bottom-right, flush with the ⋯ button above it. Only the models dim. */}
      <CardContent className="mt-auto flex items-end gap-3">
        {p.models.length === 0 ? (
          <span className={cn("min-w-0 flex-1 text-xs text-muted-foreground/70", dim)}>{t("providers.noModelsListed")}</span>
        ) : (
          <div className={cn("flex min-w-0 flex-1 flex-wrap items-center gap-1.5", dim)}>
            {shown.map((m) => (
              <Badge key={m} variant="secondary" className="max-w-full font-normal text-muted-foreground">
                <span className="truncate">{m}</span>
              </Badge>
            ))}
            {rest.length > 0 && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Badge variant="outline" className="cursor-help font-normal text-muted-foreground tabular-nums">
                    +{rest.length}
                  </Badge>
                </TooltipTrigger>
                <TooltipContent className="max-w-72">
                  <ul className="list-none space-y-0.5 py-0.5">
                    {rest.map((m) => (
                      <li key={m}>{m}</li>
                    ))}
                  </ul>
                </TooltipContent>
              </Tooltip>
            )}
          </div>
        )}
        <Tooltip>
          {/* A span, not `asChild`: the trigger's own `data-state` (open/closed) would override the switch's checked/unchecked. */}
          <TooltipTrigger asChild>
            <span className="inline-flex shrink-0">
              <Switch checked={p.enabled} disabled={busy} aria-label={stateLabel} onCheckedChange={onToggle} />
            </span>
          </TooltipTrigger>
          <TooltipContent>{stateLabel}</TooltipContent>
        </Tooltip>
      </CardContent>
    </Card>
  );
}

type Draft = {
  isNew: boolean;
  /** The id in config.toml right now; the provider is renamed when `id` differs. */
  originalId: string;
  id: string;
  preset: string;
  adapter: string;
  baseUrl: string;
  auth: string;
  models: string;
  enabled: boolean;
  keyMode: KeyMode;
  secret: string;
  envVar: string;
};

function draftFrom(p?: ProviderInfo): Draft {
  const raw = p?.raw;
  const key = raw?.apiKey ?? "";
  const keyMode: KeyMode = !p ? "keyring" : raw?.inlineKey || key.startsWith("keyring:") ? "keep" : key.startsWith("env:") ? "env" : key === "none" ? "none" : "keep";
  return {
    isNew: !p,
    originalId: p?.id ?? "",
    id: p?.id ?? "",
    preset: p ? (p.preset ?? CUSTOM) : "",
    adapter: raw?.adapter ?? "",
    baseUrl: raw?.baseUrl ?? "",
    auth: raw?.auth ?? "",
    models: (raw?.models ?? []).join("\n"),
    enabled: p?.enabled ?? true,
    keyMode,
    secret: "",
    envVar: key.startsWith("env:") ? key.slice(4) : "",
  };
}

export function ProvidersPage() {
  const { t } = useTranslation();
  const { saved } = useApp();
  // The edit dialog works on its own `draft` snapshot, so a poll landing mid-edit leaves the form alone.
  const providers = useQuery(api.providers, [], { refreshInterval: REFRESH.config });
  const presets = useQuery(api.presets);
  const [draft, setDraft] = useState<Draft>();
  const [step, setStep] = useState(0);
  const [deleting, setDeleting] = useState<ProviderInfo>();
  const [saving, setSaving] = useState(false);
  /** Id of the provider whose enabled switch is mid-save. */
  const [toggling, setToggling] = useState<string>();

  const set = (patch: Partial<Draft>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const preset: Preset | undefined = presets.data?.find((p) => p.id === draft?.preset);
  const steps = STEPS.map((s) => ({ label: t(`providers.steps.${s}`) }));
  const last = STEPS.length - 1;

  /** Adding starts at the preset tiles; editing skips straight to the connection details (the tiles stay a click away). */
  const open = (p?: ProviderInfo) => {
    setDraft(draftFrom(p));
    setStep(p ? 1 : 0);
  };

  /** A tile was picked. A fresh draft follows the preset (id, overrides cleared); an existing one only swaps the preset. */
  const choose = (v: string) => {
    if (!draft || v === draft.preset) return;
    const idFollows = !draft.id.trim() || draft.id === draft.preset;
    const id = idFollows ? (v === CUSTOM ? "" : v) : draft.id;
    if (draft.isNew && v !== CUSTOM) set({ preset: v, id, adapter: "", baseUrl: "", auth: "" });
    else set({ preset: v, id });
  };

  const save = async (d: Draft) => {
    setSaving(true);
    try {
      const id = d.id.trim();
      let apiKey: string | null = null;
      if (d.keyMode === "keyring") {
        if (!d.secret.trim()) throw new Error(t("providers.errors.pasteKey"));
        apiKey = await api.setKey(id, d.secret);
      } else if (d.keyMode === "env") {
        if (!d.envVar.trim()) throw new Error(t("providers.errors.nameEnvVar"));
        apiKey = `env:${d.envVar.trim()}`;
      } else if (d.keyMode === "inline") {
        if (!d.secret.trim()) throw new Error(t("providers.errors.pasteKey"));
        apiKey = d.secret.trim();
      } else if (d.keyMode === "none") {
        apiKey = "none";
      }
      const edit: ProviderEdit = {
        id,
        originalId: d.isNew ? undefined : d.originalId,
        preset: d.preset && d.preset !== CUSTOM ? d.preset : null,
        adapter: d.adapter.trim() || null,
        baseUrl: d.baseUrl.trim() || null,
        apiKey,
        keepApiKey: d.keyMode === "keep",
        auth: d.auth || null,
        models: d.models.split(/[\n,]/).map((m) => m.trim()).filter(Boolean),
        enabled: d.enabled,
      };
      await api.saveProvider(edit);
      setDraft(undefined);
      saved(t("toast.saved", { id }));
      await providers.reload();
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.notSaved"), description: e instanceof Error ? e.message : String(e) });
    } finally {
      setSaving(false);
    }
  };

  /** The card switch; the same save path as the dialog, with the key and everything else kept as is. */
  const toggle = async (p: ProviderInfo, enabled: boolean) => {
    setToggling(p.id);
    try {
      await save({ ...draftFrom(p), keyMode: "keep", enabled });
    } finally {
      setToggling(undefined);
    }
  };

  const remove = async () => {
    if (!deleting) return;
    try {
      await api.deleteProvider(deleting.id);
      saved(t("toast.removed", { id: deleting.id }));
      await providers.reload();
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.notRemoved"), description: String(e) });
    } finally {
      setDeleting(undefined);
    }
  };

  const custom = draft?.preset === CUSTOM;
  const renamed = !!draft && !draft.isNew && draft.id.trim() !== draft.originalId;
  const needsSecret = draft?.keyMode === "keyring" || draft?.keyMode === "inline";
  /** Whether step `i` is complete; Next stays off until it is, Save until all are. */
  const stepOk = (i: number): boolean => {
    if (!draft) return false;
    if (i === 0) return !!draft.preset;
    if (i === 1) return !!draft.id.trim() && (!custom || (!!draft.adapter && !!draft.baseUrl.trim()));
    return !(needsSecret && !draft.secret.trim()) && !(draft.keyMode === "env" && !draft.envVar.trim());
  };
  const invalid = !STEPS.every((_, i) => stepOk(i));
  const editing = !!draft && !draft.isNew;
  const next = () => stepOk(step) && setStep(Math.min(step + 1, last));
  // Presets only label cards, so an empty list doesn't wait for them.
  const loading = providers.loading || (presets.loading && (providers.data?.length ?? 0) > 0);

  return (
    <div className="space-y-6">
      <PageHeader title={t("providers.title")} actions={<IconButton variant="default" label={t("providers.add")} icon={<Plus />} onClick={() => open()} />} />
      {providers.error && <ErrorAlert error={providers.error} />}
      {!loading && providers.data?.length === 0 && <p className="text-sm text-muted-foreground">{t("providers.noneYet")}</p>}
      {/*
       * Skeleton cards on the first load only (presets included: they decide whether a card shows its host line);
       * a reload after a save keeps the real cards in place.
       */}
      <div className="grid gap-4 md:grid-cols-2 2xl:grid-cols-3">
        {loading && repeat(3, (i) => <ProviderCardSkeleton key={i} />)}
        {!loading &&
          providers.data?.map((p) => (
            <ProviderCard
              key={p.id}
              p={p}
              presetUrl={presets.data?.find((x) => x.id === p.preset)?.baseUrl}
              busy={toggling === p.id}
              onEdit={() => open(p)}
              onToggle={(v) => void toggle(p, v)}
              onRemove={() => setDeleting(p)}
            />
          ))}
      </div>

      <Dialog open={!!draft} onOpenChange={(open) => !open && setDraft(undefined)}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              {draft && !draft.isNew && <VendorIcon provider={custom ? draft.id : draft.preset} className="size-5" />}
              {draft?.isNew ? t("providers.dialog.addTitle") : t("providers.dialog.editTitle", { id: draft?.originalId })}
            </DialogTitle>
            <DialogDescription>{draft?.isNew ? t("providers.dialog.addDescription") : t("providers.dialog.editDescription")}</DialogDescription>
          </DialogHeader>
          {draft && (
            <>
              <Stepper steps={steps} value={step} onValueChange={setStep} isNavigable={(i) => editing || i < step} />
              <div key={step} className="grid min-h-64 content-start gap-4 animate-in fade-in-0 duration-200">
                {step === 0 && (
                  <div role="group" aria-label={t("providers.fields.preset")} className="-m-1 grid max-h-[50vh] grid-cols-3 gap-3 overflow-y-auto p-1 sm:grid-cols-5">
                    {sortPresets(presets.data ?? []).map((p) => (
                      <ChoiceTile
                        key={p.id}
                        selected={draft.preset === p.id}
                        icon={<VendorIcon provider={p.id} className="size-9" />}
                        label={p.displayName}
                        onSelect={() => choose(p.id)}
                        onConfirm={next}
                      />
                    ))}
                    <ChoiceTile
                      selected={custom}
                      icon={<Globe className="size-8 text-muted-foreground" />}
                      label={t("providers.fields.customEndpoint")}
                      onSelect={() => choose(CUSTOM)}
                      onConfirm={next}
                    />
                  </div>
                )}
                {step === 1 && (
                  <>
                    <div className="grid grid-cols-2 gap-4">
                      <Field>
                        <FieldLabel htmlFor="provider-id">{t("providers.fields.id")}</FieldLabel>
                        <Input
                          id="provider-id"
                          value={draft.id}
                          placeholder={custom ? "relay" : preset?.id || "anthropic"}
                          autoComplete="off"
                          spellCheck={false}
                          onChange={(e) => set({ id: e.target.value })}
                        />
                      </Field>
                      {(custom || draft.adapter) && (
                        <Field>
                          <FieldLabel htmlFor="provider-adapter">{t("providers.fields.protocol")}</FieldLabel>
                          <Select value={draft.adapter} onValueChange={(v) => set({ adapter: v })}>
                            <SelectTrigger id="provider-adapter" className="w-full">
                              <SelectValue placeholder={t("common.choose")} />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="openai-chat">{t("providers.fields.protocolOpenai")}</SelectItem>
                              <SelectItem value="anthropic">{t("providers.fields.protocolAnthropic")}</SelectItem>
                            </SelectContent>
                          </Select>
                        </Field>
                      )}
                      <Field>
                        <FieldLabel htmlFor="provider-auth">{t("providers.fields.authHeader")}</FieldLabel>
                        <Select value={draft.auth || "default"} onValueChange={(v) => set({ auth: v === "default" ? "" : v })}>
                          <SelectTrigger id="provider-auth" className="w-full">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="default">{t("providers.fields.authDefault")}</SelectItem>
                            <SelectItem value="bearer">{t("providers.fields.authBearer")}</SelectItem>
                            <SelectItem value="none">{t("providers.fields.authNone")}</SelectItem>
                          </SelectContent>
                        </Select>
                      </Field>
                      <Field className="col-span-2">
                        <FieldLabel htmlFor="provider-url">{t("providers.fields.baseUrl")}</FieldLabel>
                        <Input
                          id="provider-url"
                          value={draft.baseUrl}
                          placeholder={preset?.baseUrl ?? "https://llm.example.com/v1"}
                          autoComplete="off"
                          spellCheck={false}
                          onChange={(e) => set({ baseUrl: e.target.value })}
                        />
                        {!custom && <FieldDescription>{t("providers.fields.baseUrlHint")}</FieldDescription>}
                      </Field>
                      <Field className="col-span-2">
                        {/* The draft keeps the newline string `save` expects; the editor works on the array. */}
                        <ModelIdList
                          id="provider-models"
                          label={
                            <FieldLabel htmlFor="provider-models">
                              {t("providers.fields.modelIds")}
                              <span className="font-normal text-muted-foreground">· {t("common.optional")}</span>
                            </FieldLabel>
                          }
                          value={parseModelIds(draft.models)}
                          placeholder={"claude-sonnet-5\nclaude-opus-5-5"}
                          hint={t("providers.fields.modelsHintSimple")}
                          bulkHint={t("providers.fields.modelsHint")}
                          onChange={(ids) => set({ models: ids.join("\n") })}
                        />
                      </Field>
                    </div>
                    {renamed && (
                      <FieldDescription>
                        <Trans i18nKey="providers.renameNote" values={{ id: draft.originalId }} components={{ code: <span className="font-mono text-foreground" /> }} />
                      </FieldDescription>
                    )}
                  </>
                )}
                {step === 2 && (
                  <>
                    <Field>
                      <FieldLabel htmlFor="provider-key-mode">{t("providers.fields.apiKey")}</FieldLabel>
                      <Select value={draft.keyMode} onValueChange={(v) => set({ keyMode: v as KeyMode })}>
                        <SelectTrigger id="provider-key-mode" className="w-full">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {!draft.isNew && <SelectItem value="keep">{t("providers.fields.keyModeKeep")}</SelectItem>}
                          <SelectItem value="keyring">{t("providers.fields.keyModeKeyring")}</SelectItem>
                          <SelectItem value="env">{t("providers.fields.keyModeEnv")}</SelectItem>
                          <SelectItem value="inline">{t("providers.fields.keyModeInline")}</SelectItem>
                          <SelectItem value="none">{t("providers.fields.keyModeNone")}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    {needsSecret && (
                      <Field>
                        <FieldLabel htmlFor="provider-secret">{t("providers.fields.key")}</FieldLabel>
                        <Input
                          id="provider-secret"
                          type="password"
                          autoComplete="off"
                          value={draft.secret}
                          placeholder="sk-…"
                          onChange={(e) => set({ secret: e.target.value })}
                        />
                        {draft.keyMode === "inline" && (
                          <FieldDescription className="text-amber-600 dark:text-amber-400">{t("providers.fields.inlineWarning")}</FieldDescription>
                        )}
                      </Field>
                    )}
                    {draft.keyMode === "env" && (
                      <Field>
                        <FieldLabel htmlFor="provider-env">{t("providers.fields.envVar")}</FieldLabel>
                        <Input
                          id="provider-env"
                          value={draft.envVar}
                          placeholder={preset?.apiKey?.replace(/^env:/, "") ?? "MY_API_KEY"}
                          autoComplete="off"
                          spellCheck={false}
                          onChange={(e) => set({ envVar: e.target.value })}
                        />
                        <FieldDescription>{t("providers.fields.envHint")}</FieldDescription>
                      </Field>
                    )}
                    <Field orientation="horizontal" className="w-auto">
                      <Switch id="provider-enabled" checked={draft.enabled} onCheckedChange={(v) => set({ enabled: v })} />
                      <FieldLabel htmlFor="provider-enabled">{t("providers.fields.enabled")}</FieldLabel>
                    </Field>
                  </>
                )}
              </div>
            </>
          )}
          <DialogFooter className="sm:items-center">
            <Button variant="outline" className="sm:mr-auto" onClick={() => setDraft(undefined)}>
              {t("common.cancel")}
            </Button>
            {step > 0 && (
              <Button variant="outline" onClick={() => setStep(step - 1)}>
                {t("common.back")}
              </Button>
            )}
            {step < last && (
              <Button variant={editing ? "outline" : "default"} onClick={next} disabled={!stepOk(step)}>
                {t("common.next")}
              </Button>
            )}
            {(step === last || editing) && (
              <Button onClick={() => draft && void save(draft)} disabled={saving || invalid}>
                {t("common.save")}
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog open={!!deleting} onOpenChange={(open) => !open && setDeleting(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("providers.removeTitle", { id: deleting?.id })}</AlertDialogTitle>
            <AlertDialogDescription>{t("providers.removeDescription")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => void remove()}>
              {t("common.remove")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
