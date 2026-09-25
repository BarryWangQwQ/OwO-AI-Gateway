import { Suspense, lazy, useEffect, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { useTheme, type Theme } from "@/components/theme";

import { useApp } from "@/components/app-context";
import { CircleCheck as CheckCircle2, Languages, Monitor, Moon, RotateCcw, Save, Sun } from "@/components/icons";
import { DangerZone } from "@/components/danger-zone";
import { ErrorBoundary } from "@/components/error-boundary";
import { DetailRow, ErrorAlert, Hint, IconButton, PageHeader, SettingRow } from "@/components/page";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { useAutoStartGateway } from "@/hooks/use-auto-start-gateway";
import { useQuery } from "@/hooks/use-query";
import { toast } from "@/hooks/use-toast";
import { LANGUAGES, currentLanguage, setLanguage, type Language } from "@/i18n";
import { api } from "@/lib/api";

// CodeMirror is heavy; load it only once the Settings page is opened.
const CodeEditor = lazy(() => import("@/components/code-editor"));

/** Pull a 1-based line number out of a TOML parse error such as `... at line 12 column 5` or `12:5`. */
const problemLineOf = (problem: string | null): number | null => {
  if (!problem) return null;
  const m = /\bline\s+(\d+)/i.exec(problem) ?? /(?:^|\D)(\d+):\d+(?!\d)/.exec(problem);
  return m ? Number(m[1]) : null;
};

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const { status, saved } = useApp();
  const { theme, setTheme } = useTheme();
  // Desktop-local; persisted the moment it is toggled, independent of the card's Save button.
  const [autoStart, setAutoStart] = useAutoStartGateway();
  // Deliberately not polled: both feed editable fields (name/listen, the config.toml text) that a reload would overwrite.
  const general = useQuery(api.general);
  const config = useQuery(api.configText);
  const [name, setName] = useState("");
  const [listen, setListen] = useState("");
  const [text, setText] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [checked, setChecked] = useState(false);

  useEffect(() => {
    if (general.data) {
      setName(general.data.name);
      setListen(general.data.listen);
    }
  }, [general.data]);

  useEffect(() => {
    if (config.data) {
      setText(config.data.text);
      setProblem(null);
      setChecked(false);
    }
  }, [config.data]);

  const saveGeneral = async () => {
    try {
      await api.saveGeneral(name, listen);
      saved(t("settings.settingsSaved"));
      await Promise.all([general.reload(), config.reload()]);
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.notSaved"), description: String(e) });
    }
  };

  const check = async () => {
    const result = await api.checkConfigText(text);
    setProblem(result);
    setChecked(true);
    return result === null;
  };

  const saveText = async () => {
    if (!(await check())) return;
    try {
      await api.saveConfigText(text);
      saved(t("settings.configSaved"));
      await Promise.all([general.reload(), config.reload()]);
    } catch (e) {
      toast({ variant: "destructive", title: t("toast.notSaved"), description: String(e) });
    }
  };

  const generalDirty = general.data !== undefined && (name !== general.data.name || listen !== general.data.listen);
  const saveOnEnter = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && !e.nativeEvent.isComposing && generalDirty) void saveGeneral();
  };
  const dirty = config.data !== undefined && text !== config.data.text;
  // `i18n.language` is read so the Select re-renders with the new value after a switch.
  const language = i18n.language ? currentLanguage() : "en";

  return (
    <div className="space-y-4">
      <PageHeader title={t("settings.title")} />
      <Card>
        <CardHeader className="items-center">
          <CardTitle className="font-semibold">{t("settings.general")}</CardTitle>
          {/* Name and address are applied together (the gateway re-reads them); the launch toggle saves on its own. */}
          <CardAction>
            <IconButton label={t("common.save")} icon={<Save />} variant="default" onClick={() => void saveGeneral()} disabled={!generalDirty} />
          </CardAction>
        </CardHeader>
        <CardContent className="divide-y">
          {general.error && <ErrorAlert error={general.error} />}
          <SettingRow title={t("settings.displayName")} description={t("settings.displayNameHint")} htmlFor="settings-name">
            {general.loading ? (
              <Skeleton className="h-8 w-64 rounded-2xl" />
            ) : (
              <Input id="settings-name" value={name} placeholder="OwO" className="w-64" onChange={(e) => setName(e.target.value)} onKeyDown={saveOnEnter} />
            )}
          </SettingRow>
          <SettingRow title={t("settings.listenAddress")} description={t("settings.listenHint")} htmlFor="settings-listen">
            {general.loading ? (
              <Skeleton className="h-8 w-64 rounded-2xl" />
            ) : (
              <Input id="settings-listen" value={listen} placeholder="127.0.0.1:8787" className="w-64 font-mono" onChange={(e) => setListen(e.target.value)} onKeyDown={saveOnEnter} />
            )}
          </SettingRow>
          {/* Not wrapped in a TooltipTrigger: its data-state would override the switch's checked/unchecked styling. */}
          <SettingRow title={t("settings.autoStart.label")} description={t("settings.autoStart.description")} htmlFor="autostart-gateway">
            <Switch id="autostart-gateway" checked={autoStart} onCheckedChange={setAutoStart} />
          </SettingRow>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="font-semibold">{t("settings.appearance")}</CardTitle>
        </CardHeader>
        <CardContent className="divide-y">
          <SettingRow title={t("settings.theme")} description={t("settings.themeHint")}>
            <ToggleGroup type="single" variant="outline" value={theme} onValueChange={(v) => v && setTheme(v as Theme)}>
              <ToggleGroupItem value="light" aria-label={t("settings.light")} title={t("settings.light")}>
                <Sun />
              </ToggleGroupItem>
              <ToggleGroupItem value="dark" aria-label={t("settings.dark")} title={t("settings.dark")}>
                <Moon />
              </ToggleGroupItem>
              <ToggleGroupItem value="system" aria-label={t("settings.system")} title={t("settings.system")}>
                <Monitor />
              </ToggleGroupItem>
            </ToggleGroup>
          </SettingRow>
          <SettingRow title={t("settings.language")} description={t("settings.languageHint")}>
            <Select value={language} onValueChange={(v) => setLanguage(v as Language)}>
              <SelectTrigger aria-label={t("settings.language")} className="w-64">
                <Languages className="text-muted-foreground" />
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {LANGUAGES.map((l) => (
                  <SelectItem key={l.code} value={l.code} lang={l.code}>
                    {l.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingRow>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="font-semibold">{t("settings.about")}</CardTitle>
        </CardHeader>
        <CardContent className="divide-y">
          <DetailRow label={t("settings.version")}>{status?.version ?? "…"}</DetailRow>
          <DetailRow label={t("settings.config")}>
            <span className="font-mono text-xs">{status?.configPath}</span>
          </DetailRow>
          <DetailRow label={t("settings.owoCli")}>
            <span className="font-mono text-xs">{status?.owoBinary ?? t("settings.notFound")}</span>
          </DetailRow>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="items-center">
          <CardTitle className="flex items-center gap-1 font-semibold">
            config.toml <Hint text={t("settings.configHint")} />
          </CardTitle>
          <CardAction className="flex items-center gap-1">
            <IconButton label={t("common.save")} icon={<Save />} variant="default" onClick={() => void saveText()} disabled={!dirty} />
            <IconButton label={t("settings.check")} icon={<CheckCircle2 />} onClick={() => void check()} />
            <IconButton label={t("settings.discardChanges")} icon={<RotateCcw />} variant="ghost" onClick={() => void config.reload()} disabled={!dirty} />
          </CardAction>
        </CardHeader>
        <CardContent className="grid gap-3">
          <ErrorBoundary title={t("settings.editorFailed")}>
            {config.loading ? (
              <Skeleton className="min-h-[360px]" />
            ) : (
              <Suspense fallback={<Skeleton className="min-h-[360px]" />}>
                <CodeEditor
                  value={text}
                  onChange={(v) => {
                    setText(v);
                    setChecked(false);
                  }}
                  onSave={() => void saveText()}
                  problemLine={problemLineOf(problem)}
                />
              </Suspense>
            )}
          </ErrorBoundary>
          {problem && <ErrorAlert title={t("settings.configInvalid")} error={problem} />}
          {checked && !problem && (
            <p className="flex items-center gap-1.5 text-sm text-emerald-600 dark:text-emerald-400">
              <CheckCircle2 className="size-4" /> {t("settings.valid")}
            </p>
          )}
        </CardContent>
      </Card>

      <DangerZone onChanged={() => Promise.all([general.reload(), config.reload()])} />
    </div>
  );
}
