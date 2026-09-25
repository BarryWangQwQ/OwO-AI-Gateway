import { useState } from "react";
import i18next from "i18next";
import { Trans, useTranslation } from "react-i18next";

import { useApp } from "@/components/app-context";
import { MoreHorizontal, Pencil, Plus, Trash2 } from "@/components/icons";
import { ChoiceTile, ErrorAlert, IconButton, PageHeader, TABLE_EDGE_INSET } from "@/components/page";
import { TableRowsSkeleton } from "@/components/skeletons";
import { Dot } from "@/components/status-badge";
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
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { InputGroup, InputGroupAddon, InputGroupInput, InputGroupText } from "@/components/ui/input-group";
import { Stepper } from "@/components/ui/stepper";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { REFRESH, useQuery } from "@/hooks/use-query";
import { toast } from "@/hooks/use-toast";
import { api, type ModelInfo, type Price } from "@/lib/api";
import { priceLabel } from "@/lib/format";
import { providerLabel } from "@/lib/names";

type Draft = {
  isNew: boolean;
  /** The id in config.toml right now; the model is renamed when `id` differs. */
  originalId: string;
  id: string;
  provider: string;
  displayName: string;
  upstreamModel: string;
  input: string;
  output: string;
  cacheRead: string;
  cacheWrite: string;
};

/** Wizard steps; `models.steps.*` holds the labels. */
const STEPS = ["provider", "model", "price"] as const;

/** Price fields (with their `models.price.*` label key) and a typical value as the placeholder (Claude Sonnet, USD / 1M tokens). */
const PRICE_FIELDS = [
  ["input", "models.price.input", "3"],
  ["output", "models.price.output", "15"],
  ["cacheRead", "models.price.cacheRead", "0.3"],
  ["cacheWrite", "models.price.cacheWrite", "3.75"],
] as const;

function draftFrom(m?: ModelInfo, provider = ""): Draft {
  const p = m?.price;
  return {
    isNew: !m,
    originalId: m?.id ?? "",
    id: m?.id ?? "",
    provider: m?.provider ?? provider,
    displayName: m && m.displayName !== m.id ? m.displayName : "",
    // Shown even when it equals the id, so a rename makes plain where requests still go.
    upstreamModel: m?.upstreamModel ?? "",
    input: p ? String(p.input) : "",
    output: p ? String(p.output) : "",
    cacheRead: p?.cache_read != null ? String(p.cache_read) : "",
    cacheWrite: p?.cache_write != null ? String(p.cache_write) : "",
  };
}

function priceFrom(d: Draft): Price | null {
  if (!d.input.trim() && !d.output.trim()) return null;
  // `what` is a `models.price.*` label key; the message starts with the translated label so the field can be marked invalid.
  const num = (s: string, what: string) => {
    const n = Number(s);
    if (s.trim() === "" || !Number.isFinite(n) || n < 0) throw new Error(i18next.t("models.price.invalid", { field: i18next.t(what) }));
    return n;
  };
  const opt = (s: string, what: string) => (s.trim() ? num(s, what) : null);
  return {
    input: num(d.input, "models.price.input"),
    output: num(d.output, "models.price.output"),
    cache_read: opt(d.cacheRead, "models.price.cacheRead"),
    cache_write: opt(d.cacheWrite, "models.price.cacheWrite"),
  };
}

/** The message `priceFrom` would throw for this draft, or `null` when the prices are fine. */
function priceProblem(d: Draft): string | null {
  try {
    priceFrom(d);
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : String(e);
  }
}

export function ModelsPage() {
  const { t } = useTranslation();
  const { saved } = useApp();
  // The edit dialog works on its own `draft` snapshot, so a poll landing mid-edit leaves the form alone.
  const models = useQuery(api.models, [], { refreshInterval: REFRESH.config });
  const providers = useQuery(api.providers, [], { refreshInterval: REFRESH.config });
  const [draft, setDraft] = useState<Draft>();
  const [step, setStep] = useState(0);
  const [deleting, setDeleting] = useState<ModelInfo>();
  const [saving, setSaving] = useState(false);

  /** Adding starts at the provider tiles (pre-picked when there is just one); editing skips to the model fields. */
  const open = (m?: ModelInfo) => {
    const only = providers.data?.length === 1 ? providers.data[0]?.id : undefined;
    setDraft(draftFrom(m, only));
    setStep(m ? 1 : 0);
  };

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    try {
      await api.saveModel({
        id: draft.id.trim(),
        originalId: draft.isNew ? undefined : draft.originalId,
        provider: draft.provider,
        displayName: draft.displayName.trim() || undefined,
        upstreamModel: draft.upstreamModel.trim() || undefined,
        price: priceFrom(draft),
      });
      setDraft(undefined);
      saved(t("toast.saved", { id: draft.id }));
      await models.reload();
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.notSaved"), description: e instanceof Error ? e.message : String(e) });
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!deleting) return;
    try {
      await api.deleteModel(deleting.id);
      saved(t("toast.removed", { id: deleting.id }));
      await models.reload();
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.notRemoved"), description: String(e) });
    } finally {
      setDeleting(undefined);
    }
  };

  const set = (patch: Partial<Draft>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const priceError = draft ? priceProblem(draft) : null;
  const renamed = !!draft && !draft.isNew && draft.id.trim() !== draft.originalId;
  const upstream = draft?.upstreamModel.trim() || draft?.id.trim();
  const editing = !!draft && !draft.isNew;
  const steps = STEPS.map((s) => ({ label: t(`models.steps.${s}`) }));
  const last = STEPS.length - 1;
  /** Whether step `i` is complete; Next stays off until it is, Save until all are. */
  const stepOk = (i: number): boolean => {
    if (!draft) return false;
    if (i === 0) return !!draft.provider;
    if (i === 1) return !!draft.id.trim();
    return !priceError;
  };
  const invalid = !STEPS.every((_, i) => stepOk(i));
  const next = () => stepOk(step) && setStep(Math.min(step + 1, last));
  const chosen = providers.data?.find((p) => p.id === draft?.provider);
  // Skeleton rows until both the models and the provider names they show are here; a reload after a save keeps the rows.
  // Providers only label rows, so an empty list doesn't wait for them.
  const loading = models.loading || (providers.loading && (models.data?.length ?? 0) > 0);

  return (
    <div className="space-y-6">
      <PageHeader
        title={t("models.title")}
        actions={
          <IconButton variant="default" label={t("models.add")} icon={<Plus />} onClick={() => open()} disabled={!providers.data?.length} />
        }
      />
      {models.error && <ErrorAlert error={models.error} />}
      <Card className="py-0">
        <CardContent className="px-0">
          <Table className={TABLE_EDGE_INSET}>
            <TableHeader>
              <TableRow>
                <TableHead>{t("models.columns.model")}</TableHead>
                <TableHead>{t("models.columns.provider")}</TableHead>
                <TableHead>{t("models.columns.upstreamId")}</TableHead>
                <TableHead>{t("models.columns.price")}</TableHead>
                <TableHead>{t("models.columns.status")}</TableHead>
                <TableHead className="w-12" />
              </TableRow>
            </TableHeader>
            {/* Rows are 48px (the `size-8` ⋯ button + `p-2`), so the skeleton lines are `h-8`. */}
            {loading && (
              <TableRowsSkeleton
                rows={4}
                line="h-8"
                columns={[{ w: "w-40" }, { w: "w-24" }, { w: "w-32", bar: "h-3" }, { w: "w-24" }, { w: "w-20" }, { w: "size-8", bar: "size-8 rounded-2xl" }]}
              />
            )}
            <TableBody>
              {!loading &&
                models.data?.map((m) => (
                  <TableRow key={m.id}>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <VendorIcon id={m.id} provider={m.provider} className="size-4 shrink-0" />
                        <div className="min-w-0">
                          <div className="font-medium">{m.displayName || m.id}</div>
                          {m.displayName !== m.id && <div className="font-mono text-xs text-muted-foreground">{m.id}</div>}
                        </div>
                      </div>
                    </TableCell>
                    <TableCell title={m.provider}>{providerLabel(m.provider, providers.data)}</TableCell>
                    <TableCell className="font-mono text-xs text-muted-foreground">{m.upstreamModel}</TableCell>
                    <TableCell className="tabular-nums">
                      {m.price ? priceLabel(m.price) : <span className="text-muted-foreground">—</span>}
                    </TableCell>
                    <TableCell>
                      <span className={`flex items-center gap-2 text-sm ${m.available ? "" : "text-muted-foreground"}`}>
                        <Dot on={m.available} /> {m.available ? t("models.available") : t("models.disabled")}
                      </span>
                    </TableCell>
                    <TableCell>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon">
                            <MoreHorizontal />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onClick={() => open(m)}>
                            <Pencil /> {t("common.edit")}
                          </DropdownMenuItem>
                          <DropdownMenuItem variant="destructive" onClick={() => setDeleting(m)}>
                            <Trash2 /> {t("common.remove")}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </TableCell>
                  </TableRow>
                ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Dialog open={!!draft} onOpenChange={(open) => !open && setDraft(undefined)}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{draft?.isNew ? t("models.dialog.addTitle") : t("models.dialog.editTitle", { id: draft?.originalId })}</DialogTitle>
            <DialogDescription>{draft?.isNew ? t("models.dialog.addDescription") : t("models.dialog.editDescription")}</DialogDescription>
          </DialogHeader>
          {draft && (
            <>
              <Stepper steps={steps} value={step} onValueChange={setStep} isNavigable={(i) => editing || i < step} />
              <div key={step} className="grid min-h-56 content-start gap-4 animate-in fade-in-0 duration-200">
                {step === 0 && (
                  <div role="group" aria-label={t("models.fields.provider")} className="-m-1 grid max-h-[50vh] grid-cols-3 gap-3 overflow-y-auto p-1 sm:grid-cols-5">
                    {providers.data?.map((p) => (
                      <ChoiceTile
                        key={p.id}
                        selected={draft.provider === p.id}
                        icon={<VendorIcon provider={p.preset ?? p.id} className="size-9" />}
                        label={p.displayName}
                        className={p.enabled ? undefined : "opacity-60"}
                        onSelect={() => set({ provider: p.id })}
                        onConfirm={next}
                      />
                    ))}
                  </div>
                )}
                {step === 1 && (
                  <>
                    <FieldDescription className="flex items-center gap-1.5">
                      <VendorIcon provider={chosen?.preset ?? draft.provider} className="size-3.5" />
                      {t("models.fields.provider")} · <span className="text-foreground">{chosen?.displayName || draft.provider}</span>
                      {chosen && chosen.displayName !== chosen.id && <span className="font-mono">({chosen.id})</span>}
                    </FieldDescription>
                    <div className="grid grid-cols-2 gap-4">
                      <Field>
                        <FieldLabel htmlFor="model-id">{t("models.fields.modelId")}</FieldLabel>
                        <InputGroup>
                          <InputGroupAddon>
                            <VendorIcon id={draft.id} provider={draft.provider} className="size-4" />
                          </InputGroupAddon>
                          <InputGroupInput
                            id="model-id"
                            value={draft.id}
                            placeholder="claude-sonnet-5"
                            autoComplete="off"
                            spellCheck={false}
                            autoFocus={draft.isNew}
                            onChange={(e) => set({ id: e.target.value })}
                          />
                        </InputGroup>
                      </Field>
                      <Field>
                        <FieldLabel htmlFor="model-name">{t("models.fields.displayName")}</FieldLabel>
                        <Input id="model-name" value={draft.displayName} placeholder={draft.id.trim() || "Claude Sonnet 5"} onChange={(e) => set({ displayName: e.target.value })} />
                      </Field>
                      <Field className="col-span-2">
                        <FieldLabel htmlFor="model-upstream">{t("models.fields.upstreamId")}</FieldLabel>
                        <InputGroup>
                          <InputGroupAddon>
                            <VendorIcon id={upstream} provider={draft.provider} className="size-4" />
                          </InputGroupAddon>
                          <InputGroupInput
                            id="model-upstream"
                            value={draft.upstreamModel}
                            placeholder={draft.id.trim() || t("models.fields.sameAsId")}
                            autoComplete="off"
                            spellCheck={false}
                            onChange={(e) => set({ upstreamModel: e.target.value })}
                          />
                        </InputGroup>
                      </Field>
                    </div>
                    {renamed && (
                      <FieldDescription>
                        <Trans i18nKey="models.renameNote" values={{ from: draft.originalId, to: upstream }} components={{ code: <span className="font-mono text-foreground" /> }} />
                      </FieldDescription>
                    )}
                  </>
                )}
                {step === 2 && (
                  <>
                    <FieldDescription>{t("models.price.description")}</FieldDescription>
                    <div className="grid grid-cols-2 gap-4">
                      {PRICE_FIELDS.map(([key, label, hint]) => (
                        <Field key={key}>
                          <FieldLabel htmlFor={`model-price-${key}`}>{t(label)}</FieldLabel>
                          <InputGroup>
                            <InputGroupAddon>
                              <InputGroupText>$</InputGroupText>
                            </InputGroupAddon>
                            <InputGroupInput
                              id={`model-price-${key}`}
                              inputMode="decimal"
                              value={draft[key]}
                              placeholder={hint}
                              aria-invalid={!!priceError && priceError.startsWith(t(label)) ? true : undefined}
                              onChange={(e) => set({ [key]: e.target.value })}
                            />
                          </InputGroup>
                        </Field>
                      ))}
                    </div>
                    {priceError && <FieldError>{priceError}</FieldError>}
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
              <Button onClick={() => void save()} disabled={saving || invalid}>
                {t("common.save")}
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog open={!!deleting} onOpenChange={(open) => !open && setDeleting(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("models.removeTitle", { id: deleting?.id })}</AlertDialogTitle>
            <AlertDialogDescription>{t("models.removeDescription")}</AlertDialogDescription>
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
