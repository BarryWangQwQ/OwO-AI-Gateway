import type { ReactNode } from "react";
import { cn } from "cn";
import { useTranslation } from "react-i18next";

import { AppVendorIcon } from "@/components/vendor-icon";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { appTitle } from "@/lib/format";

export type AppToggleItem = {
  app: string;
  enabled: boolean;
  /** Its change is being saved: a spinner takes the icon's place. */
  busy?: boolean;
  disabled?: boolean;
  /** Second tooltip line; replaces the plain Enabled / Disabled. */
  reason?: string;
  /** Tooltip title; defaults to `appTitle(app)`. */
  label?: string;
  /** A small amber dot: the app's copy needs a look (the reason says why). */
  attention?: boolean;
};

/**
 * A row of round per-app on/off chips: the app logo only, in full colour with a soft ring when enabled, dimmed and grey
 * when not; the tooltip names the app and its state. `empty` is shown instead when there are no apps.
 */
export function AppToggles({
  apps,
  onToggle,
  empty,
  className,
}: {
  apps: AppToggleItem[];
  onToggle: (app: string, next: boolean) => void;
  empty?: ReactNode;
  className?: string;
}) {
  const { t } = useTranslation();
  if (apps.length === 0) return empty ? <span className={cn("text-xs text-muted-foreground/70", className)}>{empty}</span> : null;
  return (
    <div className={cn("flex flex-wrap items-center gap-1.5", className)}>
      {apps.map((a) => {
        const label = a.label ?? appTitle(a.app);
        return (
          <Tooltip key={a.app}>
            <TooltipTrigger asChild>
              {/* The span carries the tooltip, so it still shows while the button is disabled. */}
              <span className="inline-flex">
                <button
                  type="button"
                  aria-pressed={a.enabled}
                  aria-label={label}
                  disabled={a.disabled || a.busy}
                  onClick={() => onToggle(a.app, !a.enabled)}
                  className={cn(
                    "relative flex size-7 items-center justify-center rounded-full outline-none transition-all focus-visible:ring-3 focus-visible:ring-ring/30 disabled:cursor-not-allowed",
                    a.enabled
                      ? "bg-primary/10 ring-1 ring-primary/30"
                      : "opacity-35 grayscale not-disabled:hover:bg-muted not-disabled:hover:opacity-80 not-disabled:hover:grayscale-0",
                  )}
                >
                  {a.busy ? <Spinner className="size-3.5" /> : <AppVendorIcon app={a.app} className="size-4" />}
                  {a.attention && !a.busy && <span className="absolute -top-0.5 -right-0.5 size-2 rounded-full bg-amber-500 ring-2 ring-card" />}
                </button>
              </span>
            </TooltipTrigger>
            <TooltipContent className="max-w-72 flex-col items-start gap-0.5">
              <span className="font-medium">{label}</span>
              <span className="opacity-80">{a.reason ?? (a.enabled ? t("common.enabled") : t("common.disabled"))}</span>
            </TooltipContent>
          </Tooltip>
        );
      })}
    </div>
  );
}
