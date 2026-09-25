import { Suspense, lazy, useEffect, useMemo, useState, type ReactNode } from "react";
import { cn } from "cn";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";

import { toastResult } from "@/components/app-context";
import { AppToggles, type AppToggleItem as Toggle } from "@/components/app-toggles";
import { Diagnostics, type Diagnostic } from "@/components/diagnostics";
import {
  Check,
  ChevronRight,
  CircleArrowUp,
  CircleSlash,
  Compass,
  Download,
  File,
  FilePlus,
  FileText,
  Flag,
  Folder,
  FolderOpen,
  FolderPlus,
  FolderTree,
  FolderZip,
  GitBranch,
  Link2,
  MoreHorizontal,
  PackagePlus,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Search,
  Settings,
  SquarePen,
  Trash2,
  TriangleAlert,
  WandSparkles,
  X,
} from "@/components/icons";
import { ErrorAlert, Hint, IconButton, PageHeader } from "@/components/page";
import { repeat, SkillCardSkeleton } from "@/components/skeletons";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Empty, EmptyContent, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from "@/components/ui/input-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { ToastAction } from "@/components/ui/toast";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { REFRESH, useQuery } from "@/hooks/use-query";
import { toast } from "@/hooks/use-toast";
import { appTitle } from "@/lib/format";
import {
  skillsApi,
  type DiscoveredSkill,
  type Skill,
  type SkillApp,
  type SkillChange,
  type SkillFile,
  type SkillLocation,
  type SkillRepo,
  type SkillSource,
  type SkillsResult,
} from "@/lib/skills-api";

const CodeEditor = lazy(() => import("@/components/code-editor"));

type Mode = "install" | "discover";

const SOURCE_ICON: Record<SkillSource["kind"], typeof Folder> = { github: GitBranch, local: Folder, zip: FolderZip, adopted: PackagePlus, authored: SquarePen };
/** The app whose own folder an unmanaged skill sits in (its logo stands for the folder). */
const LOCATION_APP: Record<SkillLocation, string | null> = { agents: null, codex: "codex", claude: "claude", cursor: "cursor", opencode: "opencode", grok: "grok", zcode: "zcode", copilot: "copilot" };
/** Discovery listings older than this are fetched again when the dialog opens (the CLI's freshness window). */
const STALE_SECS = 6 * 3600;
const ALL = "__all__";

const short = (commit?: string) => commit?.slice(0, 7) ?? "";

function useFailed() {
  const { t } = useTranslation();
  return (e: unknown) => toast({ variant: "destructive", title: t("toast.failed"), description: String(e) });
}

// ---------------------------------------------------------------- cards

function SkillCard({
  skill,
  apps,
  busy,
  locked,
  onToggle,
  onEdit,
  onUpdate,
  onOpen,
  onRemove,
}: {
  skill: Skill;
  apps: SkillApp[];
  /** An app id, or `update`, while that change runs. */
  busy?: string;
  locked: boolean;
  onToggle: (app: string, enabled: boolean) => void;
  onEdit: () => void;
  onUpdate: () => void;
  onOpen: () => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const kind = skill.source?.kind ?? "adopted";
  const Icon = SOURCE_ICON[kind];
  const origin = kind === "adopted" || kind === "authored" ? skill.path : (skill.source?.label ?? skill.path);
  const update = skill.update?.available ? skill.update : undefined;
  const notes = [...(skill.modified ? [t("skills.modified")] : []), ...skill.warnings];
  const toggles: Toggle[] = apps.map((app) => {
    const s = skill.apps.find((a) => a.app === app.id);
    const reason = [
      s && t(`skills.state.${s.state}`, { detail: s.detail ?? "" }),
      s?.detail === "copy" && t("skills.copy"),
      !app.detected && t("skills.notInstalled"),
      s?.duplicate && t("skills.duplicate"),
    ]
      .filter(Boolean)
      .join(" · ");
    // An app that is not installed loads nothing, so it shows faded whatever its setting says.
    return { app: app.id, enabled: (s?.enabled ?? false) && app.detected, busy: busy === app.id, disabled: locked || !s?.can_toggle, reason };
  });
  return (
    <Card className="flex flex-col">
      <CardHeader>
        <div className="flex min-w-0 items-center gap-3">
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground">
                {busy === "update" ? <Spinner className="size-4" /> : <Icon className="size-4" />}
              </span>
            </TooltipTrigger>
            <TooltipContent className="max-w-96 font-mono break-all">{origin}</TooltipContent>
          </Tooltip>
          <CardTitle className="min-w-0 truncate font-mono font-semibold leading-tight">{skill.name}</CardTitle>
          {update && (
            <Tooltip>
              <TooltipTrigger asChild>
                <CircleArrowUp className="size-3.5 shrink-0 cursor-help text-amber-500" aria-label={t("skills.updateAvailable")} />
              </TooltipTrigger>
              <TooltipContent>{t("skills.updateTo", { commit: short(update.latest_commit) })}</TooltipContent>
            </Tooltip>
          )}
          {notes.length > 0 && (
            <Tooltip>
              <TooltipTrigger asChild>
                <TriangleAlert className="size-3.5 shrink-0 cursor-help text-muted-foreground" aria-label={notes[0]} />
              </TooltipTrigger>
              <TooltipContent className="max-w-80 flex-col items-start gap-0.5">
                {notes.map((n) => (
                  <span key={n}>{n}</span>
                ))}
              </TooltipContent>
            </Tooltip>
          )}
        </div>
        <CardAction>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" aria-label={t("skills.actions")}>
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem disabled={skill.is_link || !!busy} onClick={onEdit}>
                <Pencil /> {t("skills.edit")}
              </DropdownMenuItem>
              <DropdownMenuItem disabled={kind === "adopted" || kind === "authored" || !!busy} onClick={onUpdate}>
                <CircleArrowUp /> {t("skills.update")}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={onOpen}>
                <FolderOpen /> {t("skills.openFolder")}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem variant="destructive" onClick={onRemove}>
                <Trash2 /> {t("common.remove")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </CardAction>
      </CardHeader>
      <CardContent className="flex flex-1 flex-col gap-4">
        {skill.description && (
          <Tooltip>
            <TooltipTrigger asChild>
              <p className="line-clamp-2 text-xs leading-relaxed text-muted-foreground">{skill.description}</p>
            </TooltipTrigger>
            <TooltipContent className="max-w-96">{skill.description}</TooltipContent>
          </Tooltip>
        )}
        <div className="mt-auto">
          <AppToggles apps={toggles} onToggle={onToggle} />
        </div>
      </CardContent>
    </Card>
  );
}

/** Skills OwO AI Gateway does not manage, folded away: one small row each. */
function OtherSkills({ skills, busy, locked, onAdopt, onImport }: { skills: Skill[]; busy?: string; locked: boolean; onAdopt: (s: Skill) => void; onImport: (s: Skill) => void }) {
  const { t } = useTranslation();
  return (
    <Collapsible>
      <CollapsibleTrigger asChild>
        <button type="button" className="group flex items-center gap-1.5 text-sm text-muted-foreground outline-none hover:text-foreground">
          <ChevronRight className="size-4 transition-transform duration-200 group-data-[state=open]:rotate-90" />
          {t("skills.others")}
          <span className="tabular-nums">· {skills.length}</span>
        </button>
      </CollapsibleTrigger>
      <CollapsibleContent className="data-open:animate-in data-open:fade-in-0">
        <div className="mt-3 grid gap-x-6 gap-y-0.5 md:grid-cols-2 2xl:grid-cols-3">
          {skills.map((s) => {
            const app = LOCATION_APP[s.location];
            return (
              <div key={s.path} className="flex h-9 min-w-0 items-center gap-2.5 rounded-xl px-2 hover:bg-muted/50">
                {app ? <AppVendorIcon app={app} className="size-3.5 shrink-0" /> : <WandSparkles className="size-3.5 shrink-0 text-muted-foreground" />}
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span className="min-w-0 flex-1 truncate font-mono text-sm">{s.name}</span>
                  </TooltipTrigger>
                  <TooltipContent className="max-w-96 flex-col items-start gap-0.5">
                    <span className="font-mono break-all">{s.path}</span>
                    {s.description && <span className="opacity-80">{s.description}</span>}
                  </TooltipContent>
                </Tooltip>
                {s.can_adopt ? (
                  <IconButton size="icon-sm" variant="ghost" label={t("skills.adopt")} icon={<PackagePlus />} busy={busy === s.path} disabled={locked} onClick={() => onAdopt(s)} />
                ) : s.location !== "agents" && s.description ? (
                  <IconButton size="icon-sm" variant="ghost" label={t("skills.import")} icon={<Download />} busy={busy === s.path} disabled={locked} onClick={() => onImport(s)} />
                ) : null}
              </div>
            );
          })}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

// ---------------------------------------------------------------- dialogs

/** The apps a new skill is turned on for: every app with a switch, preselected where it is installed. */
function useAppChoice(apps: SkillApp[]) {
  const { t } = useTranslation();
  const defaults = useMemo(() => apps.filter((a) => a.support !== "always-on" && a.detected).map((a) => a.id as string), [apps]);
  const [chosen, setChosen] = useState<string[]>();
  const picked = chosen ?? defaults;
  const toggles: Toggle[] = apps.map((a) => {
    const always = a.support === "always-on";
    return {
      app: a.id,
      enabled: (always && a.detected) || picked.includes(a.id),
      disabled: always || !a.detected,
      reason: always ? t("skills.state.always-on") : !a.detected ? t("skills.notInstalled") : undefined,
    };
  });
  const toggle = (app: string, next: boolean) => setChosen(next ? [...picked, app] : picked.filter((x) => x !== app));
  // `null` keeps the CLI default (on everywhere); a list turns the rest off.
  const arg = defaults.every((a) => picked.includes(a)) ? null : picked;
  return { toggles, toggle, arg };
}

function AppChoice({ toggles, onToggle }: { toggles: Toggle[]; onToggle: (app: string, next: boolean) => void }) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-2">
      <span className="flex items-center text-sm text-muted-foreground">
        {t("skills.turnOnFor")}
        <Hint text={t("skills.turnOnForHint")} />
      </span>
      <AppToggles apps={toggles} onToggle={onToggle} />
    </div>
  );
}

function InstallDialog({ open, onClose, apps, onDone }: { open: boolean; onClose: () => void; apps: SkillApp[]; onDone: () => Promise<void> }) {
  const { t } = useTranslation();
  const failed = useFailed();
  const choice = useAppChoice(apps);
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);

  const pick = async (kind: "folder" | "zip") => {
    try {
      const p = await skillsApi.pick(kind);
      if (p) setPath(p);
    } catch (e) {
      failed(e);
    }
  };

  const install = async () => {
    const source = path.trim();
    if (!source || busy) return;
    setBusy(true);
    try {
      const result = await skillsApi.install(source, { apps: choice.arg, all: true });
      toastResult(result, t("skills.toast.installed"));
      if (result.ok) {
        setPath("");
        onClose();
        await onDone();
      }
    } catch (e) {
      failed(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center">
            {t("skills.install")}
            <Hint text={t("skills.sourceHint")} />
          </DialogTitle>
        </DialogHeader>
        <InputGroup>
          <InputGroupInput
            className="font-mono"
            value={path}
            aria-label={t("skills.source")}
            placeholder={t("skills.sourcePlaceholder")}
            autoComplete="off"
            spellCheck={false}
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void install()}
          />
          <InputGroupAddon align="inline-end">
            <Tooltip>
              <TooltipTrigger asChild>
                <InputGroupButton size="icon-xs" aria-label={t("skills.pickFolder")} onClick={() => void pick("folder")}>
                  <Folder />
                </InputGroupButton>
              </TooltipTrigger>
              <TooltipContent>{t("skills.pickFolder")}</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <InputGroupButton size="icon-xs" aria-label={t("skills.pickZip")} onClick={() => void pick("zip")}>
                  <FolderZip />
                </InputGroupButton>
              </TooltipTrigger>
              <TooltipContent>{t("skills.pickZip")}</TooltipContent>
            </Tooltip>
          </InputGroupAddon>
        </InputGroup>
        <DialogFooter className="sm:items-center sm:justify-between">
          <AppChoice toggles={choice.toggles} onToggle={choice.toggle} />
          <Button disabled={busy || !path.trim()} onClick={() => void install()}>
            {busy ? <Spinner /> : <Download />} {t("skills.installOne")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function DiscoverRow({ skill, busy, locked, onInstall, onUpdate }: { skill: DiscoveredSkill; busy: boolean; locked: boolean; onInstall: () => void; onUpdate: () => void }) {
  const { t } = useTranslation();
  const status = (icon: ReactNode, tip: string) => (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="flex size-7 cursor-help items-center justify-center text-muted-foreground" aria-label={tip}>
          {icon}
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-80">{tip}</TooltipContent>
    </Tooltip>
  );
  const action = skill.update_available ? (
    <IconButton size="icon-sm" variant="ghost" label={t("skills.update")} icon={<CircleArrowUp className="text-amber-500" />} busy={busy} disabled={locked} onClick={onUpdate} />
  ) : skill.installed ? (
    status(<Check className="size-4 text-emerald-500" />, t("skills.installed"))
  ) : skill.problem ? (
    status(<CircleSlash className="size-4" />, skill.problem)
  ) : skill.conflict ? (
    status(<TriangleAlert className="size-4" />, t("skills.nameTaken"))
  ) : (
    <IconButton size="icon-sm" variant="ghost" label={t("skills.installOne")} icon={<Download />} busy={busy} disabled={locked} onClick={onInstall} />
  );
  return (
    <div className="flex h-12 items-center gap-3 rounded-xl px-3 hover:bg-muted/50">
      <div className="min-w-0 flex-1">
        <div className="truncate font-mono text-sm font-medium">{skill.name}</div>
        {skill.description && (
          <Tooltip>
            <TooltipTrigger asChild>
              <p className="truncate text-xs text-muted-foreground">{skill.description}</p>
            </TooltipTrigger>
            <TooltipContent className="max-w-96">{skill.description}</TooltipContent>
          </Tooltip>
        )}
      </div>
      {action}
    </div>
  );
}

function DiscoverDialog({ open, onClose, apps, onDone }: { open: boolean; onClose: () => void; apps: SkillApp[]; onDone: () => Promise<void> }) {
  const { t } = useTranslation();
  const failed = useFailed();
  const choice = useAppChoice(apps);
  const [repos, setRepos] = useState<SkillRepo[]>();
  const [fetching, setFetching] = useState(false);
  const [error, setError] = useState<string>();
  const [repo, setRepo] = useState(ALL);
  const [query, setQuery] = useState("");
  const [managing, setManaging] = useState(false);
  const [newRepo, setNewRepo] = useState("");
  /** The install source (or `repos`) being worked on. */
  const [busy, setBusy] = useState<string>();

  const cached = async () => {
    try {
      setRepos(await skillsApi.discovered());
    } catch (e) {
      setError(String(e));
    }
  };

  const fetchRepos = async (refresh: boolean) => {
    setFetching(true);
    setError(undefined);
    try {
      setRepos(await skillsApi.discover(refresh));
    } catch (e) {
      setError(String(e));
    } finally {
      setFetching(false);
    }
  };

  // The last listing shows at once; missing or stale repositories are fetched behind it.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void (async () => {
      const list = await skillsApi.discovered().catch((e) => {
        setError(String(e));
        return undefined;
      });
      if (cancelled || !list) return;
      setRepos(list);
      const now = Date.now() / 1000;
      if (list.some((r) => r.fetched_at === undefined || now - r.fetched_at > STALE_SECS)) await fetchRepos(false);
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const run = async (key: string, action: () => Promise<SkillsResult>, success: string, after: () => Promise<void>) => {
    setBusy(key);
    try {
      const result = await action();
      toastResult(result, success);
      if (result.ok) await after();
    } catch (e) {
      failed(e);
    } finally {
      setBusy(undefined);
    }
  };
  const changed = async () => {
    await Promise.all([onDone(), cached()]);
  };
  const addRepo = () => {
    const r = newRepo.trim();
    if (!r) return;
    void run("repos", () => skillsApi.addRepo(r), t("skills.toast.repoAdded"), async () => {
      setNewRepo("");
      await fetchRepos(false);
    });
  };

  const q = query.trim().toLowerCase();
  const shown = (repos ?? [])
    .filter((r) => repo === ALL || r.repo === repo)
    .map((r) => ({ ...r, skills: r.skills.filter((s) => !q || s.name.includes(q) || s.description.toLowerCase().includes(q)) }))
    .filter((r) => r.skills.length > 0 || r.error);

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("skills.discover")}</DialogTitle>
        </DialogHeader>
        <div className="flex items-center gap-2">
          <Select value={repo} onValueChange={setRepo}>
            <SelectTrigger className="w-44 shrink-0" aria-label={t("skills.repos")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>{t("skills.allRepos")}</SelectItem>
              {(repos ?? []).map((r) => (
                <SelectItem key={r.repo} value={r.repo} className="font-mono">
                  {r.repo.split("/").slice(0, 2).join("/")}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <InputGroup className="flex-1">
            <InputGroupAddon>
              <Search />
            </InputGroupAddon>
            <InputGroupInput value={query} aria-label={t("skills.search")} placeholder={t("skills.search")} autoComplete="off" spellCheck={false} onChange={(e) => setQuery(e.target.value)} />
          </InputGroup>
          <IconButton label={t("skills.fetchAgain")} icon={<RefreshCw className={fetching ? "animate-spin" : ""} />} disabled={fetching} onClick={() => void fetchRepos(true)} />
          <IconButton variant={managing ? "secondary" : "outline"} label={t("skills.repos")} icon={<Settings />} onClick={() => setManaging(!managing)} />
        </div>

        {managing && (
          <div className="grid gap-2 rounded-xl border p-3">
            <div className="flex flex-wrap gap-1.5">
              {(repos ?? []).map((r) => (
                <Badge key={r.repo} variant="secondary" className="h-6 gap-1 pr-1 font-mono font-normal">
                  {r.repo}
                  <button
                    type="button"
                    aria-label={t("skills.removeRepo", { repo: r.repo })}
                    disabled={!!busy}
                    className="rounded-full p-0.5 hover:bg-foreground/10"
                    onClick={() => void run("repos", () => skillsApi.removeRepo(r.repo), t("skills.toast.repoRemoved", { repo: r.repo }), cached)}
                  >
                    <X className="size-3" />
                  </button>
                </Badge>
              ))}
            </div>
            <div className="flex items-center gap-2">
              <Input
                className="flex-1 font-mono"
                value={newRepo}
                aria-label={t("skills.addRepo")}
                placeholder="owner/repo[/subdir][@ref]"
                autoComplete="off"
                spellCheck={false}
                onChange={(e) => setNewRepo(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && addRepo()}
              />
              <IconButton label={t("skills.addRepo")} icon={<Plus />} disabled={!!busy || !newRepo.trim()} onClick={addRepo} />
              <IconButton
                variant="ghost"
                label={t("skills.resetRepos")}
                icon={<RotateCcw />}
                disabled={!!busy}
                onClick={() => void run("repos", skillsApi.resetRepos, t("skills.toast.reposReset"), () => fetchRepos(false))}
              />
            </div>
          </div>
        )}

        {error && <ErrorAlert error={error} />}
        <div className="-mx-3 h-[50vh] overflow-y-auto">
          {!repos &&
            repeat(6, (i) => (
              <div key={i} className="flex h-12 flex-col justify-center gap-1.5 px-3">
                <Skeleton className="h-3.5 w-40 rounded-md" />
                <Skeleton className="h-2.5 w-4/5 rounded-md" />
              </div>
            ))}
          {shown.map((r) => (
            <section key={r.repo}>
              {repo === ALL && (
                <div className="flex items-center gap-1.5 px-3 pt-3 pb-1 font-mono text-xs text-muted-foreground">
                  <GitBranch className="size-3" />
                  {r.repo}
                </div>
              )}
              {r.error && <p className="px-3 pb-1 text-xs text-destructive">{r.error}</p>}
              {r.skills.map((s) => (
                <DiscoverRow
                  key={s.dir}
                  skill={s}
                  busy={busy === s.install}
                  locked={!!busy}
                  onInstall={() => void run(s.install, () => skillsApi.install(s.install, { apps: choice.arg }), t("skills.toast.installed"), changed)}
                  onUpdate={() => void run(s.install, () => skillsApi.update([s.name]), t("skills.toast.updated", { name: s.name }), changed)}
                />
              ))}
            </section>
          ))}
          {repos && shown.length === 0 && !fetching && <p className="py-10 text-center text-sm text-muted-foreground">{t("skills.noMatch")}</p>}
        </div>
        <DialogFooter className="sm:justify-start">
          <AppChoice toggles={choice.toggles} onToggle={choice.toggle} />
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---------------------------------------------------------------- editor

/** Mirrors `owo_skills::frontmatter::valid_name`. */
const NAME = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const FRONTMATTER = /^\uFEFF?\s*---\r?\n([\s\S]*?)\r?\n---[ \t]*(?:\r?\n|$)/;

/**
 * The shape of `owo_skills::author::template` (the CLI's English one), in the interface language; the frontmatter keys
 * stay English because the spec defines them.
 */
const template = (name: string, t: TFunction) =>
  `---\nname: ${name}\ndescription: ${t("skills.template.description")}\n---\n\n# ${name}\n\n## ${t("skills.template.whenToUse")}\n\n- \n\n## ${t("skills.template.steps")}\n\n1. \n`;

/** Agent Skills limits and guidance: the spec caps `description` at 1024 characters; keep SKILL.md under ~500 lines. */
const DESCRIPTION_MAX = 1024;
const ENTRY_LINES_MAX = 500;

/** The `name` and `description` from the frontmatter; `undefined` without one. */
function readFrontmatter(text: string): { name?: string; description?: string } | undefined {
  const block = FRONTMATTER.exec(text)?.[1];
  if (block === undefined) return undefined;
  const lines = block.split(/\r?\n/);
  // Mirrors the flat reading in `owo_skills::frontmatter`: top-level `key: value` (the space is optional), quotes
  // stripped, an unquoted value's ` # comment` dropped, `>` / `|` blocks continuing on the indented lines below.
  const value = (key: string) => {
    const i = lines.findIndex((l) => new RegExp(`^${key}\\s*:`).test(l));
    if (i < 0) return undefined;
    const v = lines[i].slice(lines[i].indexOf(":") + 1).trim();
    if (/^[>|][+-]?$/.test(v)) return lines.slice(i + 1).find((l) => /^\s+\S/.test(l))?.trim() ?? "";
    const quoted = /^(["'])(.*)\1$/.exec(v);
    return (quoted ? quoted[2] : v.replace(/\s+#.*$/, "")).trim();
  };
  return { name: value("name"), description: value("description") || undefined };
}

/** `text` with the frontmatter's `name:` line set to `name`. */
function withName(text: string, name: string): string {
  const m = FRONTMATTER.exec(text);
  if (!m) return text;
  const block = /^name:.*$/m.test(m[1]) ? m[1].replace(/^name:.*$/m, `name: ${name}`) : `name: ${name}\n${m[1]}`;
  return text.replace(m[1], () => block);
}

type EditTarget = { create: true } | { create: false; skill: Skill };

const ENTRY = "SKILL.md";
const isMd = (p: string) => p.toLowerCase().endsWith(".md");
const isEntry = (p: string) => p.toLowerCase() === ENTRY.toLowerCase();
const parentOf = (p: string) => (p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : "");
const leafOf = (p: string) => p.slice(p.lastIndexOf("/") + 1);
const inside = (p: string, dir: string) => p === dir || p.startsWith(`${dir}/`);
const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/i;

/** Mirrors `owo_skills::author::rel_path`: a `/`-separated path that stays inside the skill; `undefined` when it would not. */
function relPath(raw: string): string | undefined {
  const s = raw.trim().replace(/\\/g, "/");
  if (!s || s.startsWith("/") || /^[a-z]:/i.test(s)) return undefined;
  const parts = s.split("/").filter((p) => p && p !== ".");
  const bad = (p: string) => p === ".." || p.includes(":") || /[. ]$/.test(p) || RESERVED.test(p.split(".")[0]) || /[<>"|?*\u0000-\u001f]/.test(p);
  return parts.length === 0 || parts.some(bad) ? undefined : parts.join("/");
}

/** A file or folder in the editor's working copy of a skill. */
type Node = { path: string; dir: boolean; link: boolean };
/** An opened Markdown file; `original` is what is on disk (`null`: not written yet). */
type Buffer = { content: string; original: string | null };
type Prompt = { kind: "file" | "folder" | "rename"; from?: string; value: string };

/** Folders (and links to folders) first, then files, by name; the entry point on top. */
function childrenOf(nodes: Node[], dir: string): Node[] {
  return nodes
    .filter((n) => parentOf(n.path) === dir)
    .sort((a, b) => Number(isEntry(b.path)) - Number(isEntry(a.path)) || Number(b.dir) - Number(a.dir) || leafOf(a.path).localeCompare(leafOf(b.path)));
}

/** `nodes` plus any missing parent folders of `path`. */
function withParents(nodes: Node[], path: string): Node[] {
  const out = [...nodes];
  for (let p = parentOf(path); p; p = parentOf(p)) {
    if (!out.some((n) => n.path === p)) out.push({ path: p, dir: true, link: false });
  }
  return out;
}

/**
 * The skill editor: every Markdown file of the skill is editable (SKILL.md is the entry point), folders can be made,
 * renamed (when empty) and deleted; other files are the file manager's. Changes stay in memory and one Save writes them
 * all (the CLI backs the folder up first).
 */
function SkillEditor({
  target,
  apps,
  taken,
  store,
  onClose,
  onDone,
}: {
  target?: EditTarget;
  apps: SkillApp[];
  taken: Set<string>;
  /** `~/.agents/skills`, where a new skill's folder goes. */
  store?: string;
  onClose: () => void;
  onDone: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const failed = useFailed();
  const choice = useAppChoice(apps);
  const create = target?.create ?? true;
  const skillName = target && !target.create ? target.skill.name : undefined;
  const [name, setName] = useState("");
  const [nodes, setNodes] = useState<Node[]>([]);
  /** Paths that exist on disk (so a rename or delete of them becomes a change). */
  const [onDisk, setOnDisk] = useState<Set<string>>(new Set());
  const [buffers, setBuffers] = useState<Record<string, Buffer>>({});
  /** Folder and rename/delete changes, in order; file contents are added on save. */
  const [ops, setOps] = useState<SkillChange[]>([]);
  const [selected, setSelected] = useState(ENTRY);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [info, setInfo] = useState<SkillFile>();
  const [more, setMore] = useState(0);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [prompt, setPrompt] = useState<Prompt>();
  const [deleting, setDeleting] = useState<string>();
  const [confirm, setConfirm] = useState<"detach" | "discard">();

  const load = async (skill: string) => {
    setLoading(true);
    try {
      const [tree, file] = await Promise.all([skillsApi.tree(skill), skillsApi.read(skill)]);
      setNodes(tree.entries.map((e) => ({ path: e.path, dir: e.dir, link: e.link })));
      setOnDisk(new Set(tree.entries.map((e) => e.path)));
      setMore(tree.more);
      setBuffers({ [ENTRY]: { content: file.content, original: file.content } });
      setInfo(file);
      setOps([]);
    } catch (e) {
      failed(e);
      onClose();
    } finally {
      setLoading(false);
    }
  };

  // A fresh working copy per opening: the template for a new skill, the folder on disk for an edit.
  useEffect(() => {
    setConfirm(undefined);
    setPrompt(undefined);
    setDeleting(undefined);
    setSelected(ENTRY);
    setCollapsed(new Set());
    setInfo(undefined);
    setMore(0);
    setOps([]);
    if (!target) return;
    if (target.create) {
      setName("my-skill");
      setNodes([{ path: ENTRY, dir: false, link: false }]);
      setOnDisk(new Set());
      setBuffers({ [ENTRY]: { content: template("my-skill", t), original: null } });
      return;
    }
    setName(target.skill.name);
    setNodes([]);
    setBuffers({});
    void load(target.skill.name);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target]);

  const entry = buffers[ENTRY]?.content ?? "";
  const nameError = !name ? undefined : name.length > 64 || !NAME.test(name) ? t("skills.editor.badName") : create && taken.has(name) ? t("skills.nameTaken") : undefined;
  const fm = readFrontmatter(entry);
  // `name` is the one skill name: the field for a new skill (kept in step with the frontmatter both ways), the
  // skill's own name for an edit. The check compares the frontmatter against it.
  const found = fm?.name;
  const nameTip = !found
    ? t("skills.editor.needsName", { name: name || "my-skill" })
    : !name
      ? t("skills.editor.nameEmpty")
      : found !== name
        ? t("skills.editor.nameMismatch", { found, expected: name })
        : `name: ${found}`;
  const body = entry.replace(FRONTMATTER, "");
  const lineCount = entry.split("\n").length;
  const issues: Diagnostic[] = [];
  if (!fm) issues.push({ level: "error", text: t("skills.editor.needsFrontmatter") });
  else {
    if (!found || !name || found !== name) issues.push({ level: "error", text: nameTip });
    if (!fm.description) issues.push({ level: "error", text: t("skills.editor.needsDescription") });
    else if (fm.description.length > DESCRIPTION_MAX) issues.push({ level: "warning", text: t("skills.editor.descriptionLong", { count: fm.description.length, max: DESCRIPTION_MAX }) });
  }
  if (fm && !body.trim()) issues.push({ level: "warning", text: t("skills.editor.bodyEmpty") });
  if (lineCount > ENTRY_LINES_MAX) issues.push({ level: "warning", text: t("skills.editor.entryLong", { count: lineCount, max: ENTRY_LINES_MAX }) });
  // Nothing wrong: the panel still says what was read.
  const findings: Diagnostic[] = issues.length
    ? issues
    : [
        { level: "info", text: t("skills.editor.checksOk") },
        { level: "info", text: `name: ${found}` },
        { level: "info", text: t("skills.editor.descriptionChars", { count: fm?.description?.length ?? 0 }) },
      ];
  const dirtyFile = (p: string) => !!buffers[p] && buffers[p].content !== buffers[p].original;
  const dirty = ops.length > 0 || Object.keys(buffers).some(dirtyFile);
  const ready = create || !!info;
  const valid = ready && !!name && !nameError && !issues.some((d) => d.level === "error") && (create || !!info?.editable);
  const detaches = !create && !!info && ["github", "local", "zip"].includes(info.source);
  const current = buffers[selected];

  /** The name field drives the frontmatter `name` and a heading that still repeats the old name (an empty field changes neither). */
  const rename = (next: string) => {
    const clean = next.trim().toLowerCase();
    const before = `# ${name}`;
    setName(clean);
    if (!clean) return;
    setBuffers((b) => ({ ...b, [ENTRY]: { ...b[ENTRY], content: withName(b[ENTRY].content, clean).split("\n").map((l) => (l === before ? `# ${clean}` : l)).join("\n") } }));
  };

  /** An edit of a new skill's SKILL.md that changes the frontmatter `name` changes the name field too. */
  const edit = (content: string) => {
    setBuffers((b) => ({ ...b, [selected]: { ...b[selected], content } }));
    if (create && selected === ENTRY) {
      const next = readFrontmatter(content)?.name;
      if (next && next !== name) setName(next);
    }
  };

  const open = async (path: string) => {
    setSelected(path);
    if (buffers[path] || !skillName) return;
    try {
      const f = await skillsApi.read(skillName, path);
      setBuffers((b) => ({ ...b, [path]: b[path] ?? { content: f.content, original: f.content } }));
    } catch (e) {
      failed(e);
      setSelected(ENTRY);
    }
  };

  const exists = (p: string) => nodes.some((n) => n.path.toLowerCase() === p.toLowerCase());

  /** Why the prompt's value cannot be used, if it cannot. */
  const promptError = (p: Prompt): string | undefined => {
    const rel = relPath(p.value);
    if (!rel) return p.value.trim() ? t("skills.editor.badPath") : undefined;
    const from = p.from ? nodes.find((n) => n.path === p.from) : undefined;
    const wantsMd = p.kind === "file" || (p.kind === "rename" && !from?.dir);
    if (wantsMd && !isMd(rel)) return t("skills.editor.notMd");
    if (isEntry(rel) || (rel.toLowerCase() !== p.from?.toLowerCase() && exists(rel))) return t("skills.editor.exists");
    return undefined;
  };

  const submitPrompt = () => {
    if (!prompt) return;
    const rel = relPath(prompt.value);
    if (!rel || promptError(prompt)) return;
    if (prompt.kind === "file") {
      setNodes((n) => [...withParents(n, rel), { path: rel, dir: false, link: false }]);
      setBuffers((b) => ({ ...b, [rel]: { content: `# ${leafOf(rel).replace(/\.md$/i, "")}\n`, original: null } }));
      setSelected(rel);
    } else if (prompt.kind === "folder") {
      setNodes((n) => [...withParents(n, rel), { path: rel, dir: true, link: false }]);
      setOps((o) => [...o, { op: "mkdir", path: rel }]);
    } else if (prompt.from && rel !== prompt.from) {
      const from = prompt.from;
      setNodes((n) => withParents(n.map((x) => (x.path === from ? { ...x, path: rel } : x)), rel));
      if (onDisk.has(from)) {
        setOps((o) => [...o, { op: "rename", from, to: rel }]);
        setOnDisk((d) => new Set([...[...d].filter((p) => p !== from), rel]));
      } else {
        setOps((o) => o.map((c) => (c.op === "mkdir" && c.path === from ? { ...c, path: rel } : c)));
      }
      setBuffers((b) => {
        if (!b[from]) return b;
        const { [from]: moved, ...rest } = b;
        return { ...rest, [rel]: moved };
      });
      if (selected === from) setSelected(rel);
    }
    setPrompt(undefined);
  };

  const remove = (path: string) => {
    const onDiskInside = [...onDisk].some((p) => inside(p, path));
    setNodes((n) => n.filter((x) => !inside(x.path, path)));
    setBuffers((b) => Object.fromEntries(Object.entries(b).filter(([p]) => !inside(p, path))));
    setOps((o) => [...o.filter((c) => !(c.op === "mkdir" && inside(c.path, path))), ...(onDiskInside ? [{ op: "delete" as const, path }] : [])]);
    setOnDisk((d) => new Set([...d].filter((p) => !inside(p, path))));
    if (inside(selected, path)) setSelected(ENTRY);
    setDeleting(undefined);
  };

  const save = async (confirmed = false) => {
    if (!valid || !dirty || saving) return;
    if (detaches && !confirmed) {
      setConfirm("detach");
      return;
    }
    setConfirm(undefined);
    const writes: SkillChange[] = Object.entries(buffers)
      .filter(([p]) => dirtyFile(p))
      .map(([path, b]) => ({ op: "write", path, content: b.content }));
    setSaving(true);
    try {
      const result = await skillsApi.apply(name, create, [...ops, ...writes], create ? choice.arg : null);
      if (result.ok && create && store) {
        // Other files go in with the file manager; the toast leads there.
        const dir = `${store}${store.includes("\\") ? "\\" : "/"}${name}`;
        toast({
          title: t("skills.toast.created", { name }),
          action: (
            <ToastAction altText={t("skills.openFolder")} onClick={() => void skillsApi.open(dir).catch(failed)}>
              {t("skills.openFolder")}
            </ToastAction>
          ),
        });
      } else {
        toastResult(result, t(create ? "skills.toast.created" : "skills.toast.saved", { name }));
      }
      if (result.ok) {
        await onDone();
        if (create) onClose();
        else await load(name);
      }
    } catch (e) {
      failed(e);
    } finally {
      setSaving(false);
    }
  };

  /** Re-reads the folder after changes in the file manager; a hand-written skill's copies follow. */
  const reread = async () => {
    if (!skillName) return;
    try {
      if (info?.source === "authored") {
        const r = await skillsApi.sync([skillName]);
        if (!r.ok) toastResult(r, "");
      }
      await load(skillName);
    } catch (e) {
      failed(e);
    }
  };

  const close = () => (dirty ? setConfirm("discard") : onClose());

  const counts = (dir: string) => {
    const under = nodes.filter((n) => n.path !== dir && inside(n.path, dir));
    return { files: under.filter((n) => !n.dir).length, folders: under.filter((n) => n.dir).length };
  };

  const row = (n: Node, depth: number): ReactNode => {
    const leaf = leafOf(n.path);
    const entryRow = isEntry(n.path) && depth === 0;
    const md = !n.dir && !n.link && isMd(n.path);
    const open_ = n.dir && !n.link && !collapsed.has(n.path);
    const empty = n.dir && !nodes.some((x) => x.path !== n.path && inside(x.path, n.path));
    const Icon = n.link ? Link2 : n.dir ? (open_ ? FolderOpen : Folder) : entryRow ? Flag : md ? FileText : File;
    const tip = entryRow ? t("skills.editor.entry") : n.link ? t("skills.editor.link") : !n.dir && !md ? t("skills.editor.explorerOnly") : undefined;
    const actions = !entryRow && (md || n.dir || n.link);
    const click = () => {
      if (n.dir && !n.link) setCollapsed((c) => (c.has(n.path) ? new Set([...c].filter((p) => p !== n.path)) : new Set([...c, n.path])));
      else if (md) void open(n.path);
    };
    const line = (
      <div
        role={md || n.dir ? "button" : undefined}
        tabIndex={md || n.dir ? 0 : -1}
        onClick={click}
        onKeyDown={(e) => e.key === "Enter" && click()}
        className={cn(
          "group flex h-7 items-center gap-1.5 rounded-lg pr-1 outline-none focus-visible:ring-2 focus-visible:ring-ring/30",
          md || n.dir ? "cursor-pointer hover:bg-muted/60" : "cursor-default text-muted-foreground/60",
          selected === n.path && "bg-muted font-medium text-foreground",
        )}
        style={{ paddingLeft: `${8 + depth * 14}px` }}
      >
        {n.dir && !n.link ? <ChevronRight className={cn("size-3 shrink-0 text-muted-foreground transition-transform", open_ && "rotate-90")} /> : <span className="w-3 shrink-0" />}
        <Icon className={cn("size-3.5 shrink-0", entryRow ? "text-primary" : "text-muted-foreground")} />
        <span className="min-w-0 flex-1 truncate font-mono">{leaf}</span>
        {dirtyFile(n.path) && <span className="size-1.5 shrink-0 rounded-full bg-amber-500" aria-label={t("skills.editor.unsaved")} />}
        {actions && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon-xs" className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100" aria-label={t("skills.actions")} onClick={(e) => e.stopPropagation()}>
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" onClick={(e) => e.stopPropagation()}>
              {(md || (n.dir && !n.link && empty)) && (
                <DropdownMenuItem onClick={() => setPrompt({ kind: "rename", from: n.path, value: n.path })}>
                  <Pencil /> {t("skills.editor.rename")}
                </DropdownMenuItem>
              )}
              <DropdownMenuItem variant="destructive" onClick={() => (n.dir && !n.link && !empty ? setDeleting(n.path) : remove(n.path))}>
                <Trash2 /> {t("skills.editor.delete")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>
    );
    return (
      <div key={n.path}>
        {tip ? (
          <Tooltip>
            <TooltipTrigger asChild>{line}</TooltipTrigger>
            <TooltipContent side="right">{tip}</TooltipContent>
          </Tooltip>
        ) : (
          line
        )}
        {open_ && childrenOf(nodes, n.path).map((c) => row(c, depth + 1))}
      </div>
    );
  };

  const deletion = deleting ? counts(deleting) : undefined;
  const pe = prompt ? promptError(prompt) : undefined;

  return (
    <>
      <Dialog open={!!target} onOpenChange={(o) => !o && close()}>
        <DialogContent className="sm:max-w-5xl">
          <DialogHeader>
            <DialogTitle className="flex items-center">
              {skillName ? <span className="font-mono">{skillName}</span> : t("skills.editor.newTitle")}
              <Hint text={t("skills.editor.hint")} />
            </DialogTitle>
          </DialogHeader>
          {create && (
            <div className="flex items-center gap-3">
              <Input
                className="max-w-64 font-mono"
                value={name}
                aria-label={t("skills.editor.name")}
                aria-invalid={!!nameError}
                placeholder="my-skill"
                autoComplete="off"
                spellCheck={false}
                autoFocus
                onFocus={(e) => e.target.select()}
                onChange={(e) => rename(e.target.value)}
              />
              {nameError && <span className="text-xs text-destructive">{nameError}</span>}
            </div>
          )}
          <div className="grid gap-3 sm:grid-cols-[15rem_minmax(0,1fr)]">
            <div className="flex h-[60vh] min-w-0 flex-col overflow-hidden rounded-2xl border">
              <div className="flex h-10 shrink-0 items-center gap-0.5 border-b pr-1 pl-3 text-xs text-muted-foreground">
                <FolderTree className="mr-1 size-3.5" />
                <span className="flex-1">{t("skills.editor.files")}</span>
                <IconButton size="icon-sm" variant="ghost" label={t("skills.editor.newFile")} icon={<FilePlus />} disabled={!ready} onClick={() => setPrompt({ kind: "file", value: "" })} />
                <IconButton size="icon-sm" variant="ghost" label={t("skills.editor.newFolder")} icon={<FolderPlus />} disabled={!ready} onClick={() => setPrompt({ kind: "folder", value: "" })} />
                <IconButton
                  size="icon-sm"
                  variant="ghost"
                  label={create ? t("skills.editor.afterSave") : dirty ? t("skills.editor.saveFirst") : t("skills.editor.reread")}
                  icon={<RefreshCw className={loading ? "animate-spin" : ""} />}
                  disabled={create || dirty || loading}
                  onClick={() => void reread()}
                />
                <IconButton
                  size="icon-sm"
                  variant="ghost"
                  label={create ? t("skills.editor.afterSave") : t("skills.openFolder")}
                  icon={<FolderOpen />}
                  disabled={create || !info}
                  onClick={() => info && void skillsApi.open(info.path).catch(failed)}
                />
              </div>
              <div className="min-h-0 flex-1 overflow-y-auto p-1 text-xs">
                {!ready && repeat(6, (i) => <Skeleton key={i} className="mx-2 my-2 h-3 rounded-md" style={{ width: `${75 - i * 8}%` }} />)}
                {ready && childrenOf(nodes, "").map((n) => row(n, 0))}
                {more > 0 && <div className="px-2 py-1 tabular-nums text-muted-foreground/70">+{more}</div>}
              </div>
            </div>
            <div className="flex min-w-0 flex-col gap-1.5">
              <div className="flex h-5 items-center gap-1.5 font-mono text-xs text-muted-foreground">
                {isEntry(selected) && <Flag className="size-3 text-primary" />}
                <span className="truncate">{selected}</span>
                {dirtyFile(selected) && <span className="size-1.5 rounded-full bg-amber-500" aria-label={t("skills.editor.unsaved")} />}
              </div>
              {ready && current ? (
                <Suspense fallback={<Skeleton className="h-[calc(60vh-1.625rem)] rounded-2xl" />}>
                  <CodeEditor key={selected} language="markdown" minHeight="calc(60vh - 1.625rem)" maxHeight="calc(60vh - 1.625rem)" value={current.content} onChange={edit} onSave={() => void save()} />
                </Suspense>
              ) : (
                <Skeleton className="h-[calc(60vh-1.625rem)] rounded-2xl" />
              )}
            </div>
          </div>
          <DialogFooter className="sm:items-center sm:justify-between">
            <div className="flex flex-wrap items-center gap-3">
              <Diagnostics items={findings} label={t("skills.editor.checks")} />
              {create && <AppChoice toggles={choice.toggles} onToggle={choice.toggle} />}
              {info && !info.editable && <span className="text-xs text-destructive">{t("skills.editor.notEditable")}</span>}
            </div>
            <Button disabled={!valid || !dirty || saving} onClick={() => void save()}>
              {saving ? <Spinner /> : <Check />} {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={!!prompt} onOpenChange={(o) => !o && setPrompt(undefined)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{prompt?.kind === "file" ? t("skills.editor.newFile") : prompt?.kind === "folder" ? t("skills.editor.newFolder") : t("skills.editor.rename")}</DialogTitle>
          </DialogHeader>
          <Input
            className="font-mono"
            value={prompt?.value ?? ""}
            aria-invalid={!!pe}
            aria-label={t("skills.editor.path")}
            placeholder={prompt?.kind === "folder" ? "references" : "references/api.md"}
            autoComplete="off"
            spellCheck={false}
            autoFocus
            onChange={(e) => prompt && setPrompt({ ...prompt, value: e.target.value })}
            onKeyDown={(e) => e.key === "Enter" && submitPrompt()}
          />
          <p className={cn("text-xs", pe ? "text-destructive" : "text-muted-foreground")}>{pe ?? t(prompt?.kind === "folder" ? "skills.editor.folderHint" : "skills.editor.fileHint")}</p>
          <DialogFooter>
            <Button disabled={!prompt || !relPath(prompt.value) || !!pe} onClick={submitPrompt}>
              <Check /> {t("common.ok")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog open={!!deleting} onOpenChange={(o) => !o && setDeleting(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("skills.editor.deleteTitle", { path: `${deleting ?? ""}/` })}</AlertDialogTitle>
            <AlertDialogDescription>{t("skills.editor.deleteFolder", { files: deletion?.files ?? 0, folders: deletion?.folders ?? 0 })}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => deleting && remove(deleting)}>
              {t("skills.editor.delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={!!confirm} onOpenChange={(o) => !o && setConfirm(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{confirm === "detach" ? t("skills.editor.detachTitle") : t("skills.editor.discardTitle")}</AlertDialogTitle>
            <AlertDialogDescription>{confirm === "detach" ? t("skills.editor.detachDescription") : t("skills.editor.discardDescription")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            {confirm === "detach" ? (
              <AlertDialogAction onClick={() => void save(true)}>{t("common.save")}</AlertDialogAction>
            ) : (
              <AlertDialogAction
                variant="destructive"
                onClick={() => {
                  setConfirm(undefined);
                  onClose();
                }}
              >
                {t("skills.editor.discard")}
              </AlertDialogAction>
            )}
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

// ---------------------------------------------------------------- page

export function SkillsPage() {
  const { t } = useTranslation();
  const failed = useFailed();
  const list = useQuery(skillsApi.list, [], { refreshInterval: REFRESH.config, empty: (l) => l.skills.length === 0 });
  const [mode, setMode] = useState<Mode>();
  /** The skill (or unmanaged path) being changed, and what: an app id, `update`, `remove`, `adopt`. */
  const [busy, setBusy] = useState<{ key: string; what: string }>();
  const [checking, setChecking] = useState(false);
  const [removing, setRemoving] = useState<Skill>();

  const apps = (list.data?.apps ?? []).filter((a) => a.support !== "unsupported");
  const skills = list.data?.skills ?? [];
  const others = list.data?.unmanaged ?? [];
  const [editing, setEditing] = useState<EditTarget>();
  /** Names a new skill cannot take: everything already in the skills folder. */
  const taken = useMemo(() => new Set([...skills.map((s) => s.name), ...others.filter((s) => s.location === "agents").map((s) => s.path.split(/[\\/]/).pop() ?? s.name)]), [skills, others]);

  const act = async (key: string, what: string, action: () => Promise<SkillsResult>, success: string) => {
    setBusy({ key, what });
    try {
      toastResult(await action(), success);
      await list.reload();
    } catch (e) {
      failed(e);
    } finally {
      setBusy(undefined);
    }
  };

  /** Reloads the list; with GitHub skills installed it also asks GitHub for updates (silently, unless it fails). */
  const refresh = async () => {
    setChecking(true);
    try {
      if (skills.some((s) => s.source?.kind === "github")) {
        const r = await skillsApi.update([], true);
        if (!r.ok) toastResult(r, "");
      }
      // Hand-written skills pick up files the user changed in the file manager (their copies follow).
      if (skills.some((s) => s.source?.kind === "authored")) {
        const r = await skillsApi.sync();
        if (!r.ok) toastResult(r, "");
      }
      await list.refresh();
    } catch (e) {
      failed(e);
    } finally {
      setChecking(false);
    }
  };

  const remove = async () => {
    const s = removing;
    setRemoving(undefined);
    if (s) await act(s.name, "remove", () => skillsApi.remove(s.name), t("toast.removed", { id: s.name }));
  };

  const loading = list.loading;
  const locked = !!busy;
  const spinning = checking || list.refreshing;

  return (
    <div className="space-y-6">
      <PageHeader
        title={t("skills.title")}
        actions={
          <>
            <IconButton variant="default" label={t("common.refresh")} icon={<RefreshCw className={spinning ? "animate-spin" : ""} />} disabled={spinning} onClick={() => void refresh()} />
            <IconButton variant="default" label={t("skills.discover")} icon={<Compass />} onClick={() => setMode("discover")} />
            <IconButton variant="default" label={t("skills.write")} icon={<SquarePen />} onClick={() => setEditing({ create: true })} />
            <IconButton variant="default" label={t("skills.install")} icon={<Plus />} onClick={() => setMode("install")} />
          </>
        }
      />
      {list.error && <ErrorAlert title={t("skills.cannotList")} error={list.error} />}

      {!loading && !list.error && skills.length === 0 && (
        <Empty className="border">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <WandSparkles />
            </EmptyMedia>
            <EmptyTitle>{t("skills.empty")}</EmptyTitle>
          </EmptyHeader>
          <EmptyContent className="flex-row justify-center">
            <Button onClick={() => setMode("discover")}>
              <Compass /> {t("skills.discover")}
            </Button>
            <Button variant="outline" onClick={() => setMode("install")}>
              <Plus /> {t("skills.install")}
            </Button>
          </EmptyContent>
        </Empty>
      )}

      <div className="grid gap-4 md:grid-cols-2 2xl:grid-cols-3">
        {loading && repeat(3, (i) => <SkillCardSkeleton key={i} />)}
        {!loading &&
          skills.map((s) => (
            <SkillCard
              key={s.name}
              skill={s}
              apps={apps}
              busy={busy?.key === s.name ? busy.what : undefined}
              locked={locked}
              onToggle={(app, enabled) =>
                void act(s.name, app, () => skillsApi.toggle(s.name, app, enabled), t(enabled ? "skills.toast.on" : "skills.toast.off", { name: s.name, app: appTitle(app) }))
              }
              onEdit={() => setEditing({ create: false, skill: s })}
              onUpdate={() => void act(s.name, "update", () => skillsApi.update([s.name]), t("skills.toast.updated", { name: s.name }))}
              onOpen={() => void skillsApi.open(s.path).catch(failed)}
              onRemove={() => setRemoving(s)}
            />
          ))}
      </div>

      {!loading && others.length > 0 && (
        <OtherSkills
          skills={others}
          busy={busy?.key}
          locked={locked}
          onAdopt={(s) => void act(s.path, "adopt", () => skillsApi.adopt(s.name), t("skills.toast.adopted", { name: s.name }))}
          onImport={(s) => void act(s.path, "import", () => skillsApi.install(s.path, { all: true }), t("skills.toast.installed"))}
        />
      )}

      <InstallDialog open={mode === "install"} onClose={() => setMode(undefined)} apps={apps} onDone={list.reload} />
      <DiscoverDialog open={mode === "discover"} onClose={() => setMode(undefined)} apps={apps} onDone={list.reload} />
      <SkillEditor target={editing} apps={apps} taken={taken} store={list.data?.store} onClose={() => setEditing(undefined)} onDone={list.reload} />

      <AlertDialog open={!!removing} onOpenChange={(o) => !o && setRemoving(undefined)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("skills.removeTitle", { name: removing?.name })}</AlertDialogTitle>
            <AlertDialogDescription>{t("skills.removeDescription")}</AlertDialogDescription>
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
