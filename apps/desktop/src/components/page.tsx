import type { ComponentProps, ReactNode } from "react";
import { cn } from "cn";
import { useTranslation } from "react-i18next";

import { Info } from "@/components/icons";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { FieldDescription } from "@/components/ui/field";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

/**
 * An icon-only button; the tooltip (and `aria-label`) carry the label. `busy` swaps the icon for a spinner.
 * Defaults to `outline` for in-card use; `PageHeader` actions pass `variant="default"` (filled primary).
 */
export function IconButton({
  label,
  icon,
  busy = false,
  variant = "outline",
  size = "icon",
  ...props
}: Omit<ComponentProps<typeof Button>, "children" | "aria-label"> & { label: string; icon: ReactNode; busy?: boolean }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button size={size} variant={variant} aria-label={label} {...props}>
          {busy ? <Spinner /> : icon}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

/**
 * Edge inset for a `Table` that fills a `CardContent className="px-0"`: the first and last column
 * get the card's horizontal spacing, whichever columns those turn out to be (some are conditional).
 */
export const TABLE_EDGE_INSET = "[&_td:first-child]:pl-6 [&_th:first-child]:pl-6 [&_td:last-child]:pr-6 [&_th:last-child]:pr-6";

/** An info icon whose tooltip holds the explanation, instead of a line of text. */
export function Hint({ text }: { text: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Info className="ml-1 inline size-3.5 cursor-help align-[-2px] text-muted-foreground" />
      </TooltipTrigger>
      <TooltipContent className="max-w-64">{text}</TooltipContent>
    </Tooltip>
  );
}

/** The page title, with the sidebar toggle in front of it; the only title on a page. */
export function PageHeader({ title, actions }: { title: string; actions?: ReactNode }) {
  return (
    <div className="flex min-h-9 flex-wrap items-center justify-between gap-3">
      <div className="flex items-center gap-2">
        <SidebarTrigger className="-ml-2 text-muted-foreground" />
        <h1 className="text-xl font-semibold tracking-tight">{title}</h1>
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

/** A titled group of fields in a dialog; put a `Separator` between sections. */
export function FormSection({ title, description, children }: { title: string; description?: ReactNode; children: ReactNode }) {
  return (
    <section className="grid gap-4">
      <div className="grid gap-1">
        <h3 className="text-sm font-medium leading-none">{title}</h3>
        {description && <FieldDescription>{description}</FieldDescription>}
      </div>
      {children}
    </section>
  );
}

/**
 * A large "pick one" tile for wizard steps: big icon and a one-line label (the tooltip carries the full name). Group them in a grid with `role="group"`.
 * A double-click confirms the choice (`onConfirm`), so a preset can be picked and the wizard advanced in one go.
 */
export function ChoiceTile({
  selected,
  icon,
  label,
  onSelect,
  onConfirm,
  className,
}: {
  selected: boolean;
  icon: ReactNode;
  label: string;
  onSelect: () => void;
  onConfirm?: () => void;
  className?: string;
}) {
  return (
    <button
      type="button"
      aria-pressed={selected}
      title={label}
      onClick={onSelect}
      onDoubleClick={onConfirm}
      className={cn(
        // `dark:bg-transparent` outranks a plain `hover:` variant, so the dark hover must be spelled out explicitly.
        "flex min-w-0 flex-col items-center gap-2 rounded-xl border bg-background p-3 text-center outline-none transition-colors hover:border-foreground/25 hover:bg-accent focus-visible:ring-3 focus-visible:ring-ring/30 dark:bg-transparent dark:hover:bg-accent",
        selected && "border-primary ring-2 ring-primary/30 hover:border-primary hover:bg-primary/5 dark:hover:bg-primary/10",
        className,
      )}
    >
      <span className="flex size-10 shrink-0 items-center justify-center">{icon}</span>
      <span className="w-full truncate text-[13px] font-medium leading-tight">{label}</span>
    </button>
  );
}

export function ErrorAlert({ title, error }: { title?: string; error: string }) {
  const { t } = useTranslation();
  return (
    <Alert variant="destructive">
      <AlertTitle>{title ?? t("errors.somethingWentWrong")}</AlertTitle>
      <AlertDescription className="whitespace-pre-wrap font-mono text-xs">{error}</AlertDescription>
    </Alert>
  );
}

/** A label/value row for detail views (not a form field; see ui/field for those). */
/** A settings line: title and description on the left, its control on the right. Stack several in a `divide-y` container. */
export function SettingRow({ title, description, htmlFor, children }: { title: ReactNode; description?: ReactNode; htmlFor?: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3 py-3.5 first:pt-0 last:pb-0">
      <div className="min-w-0 flex-1 space-y-0.5">
        <label htmlFor={htmlFor} className="block text-sm font-medium">
          {title}
        </label>
        {description && <p className="text-sm text-muted-foreground">{description}</p>}
      </div>
      <div className="flex shrink-0 items-center">{children}</div>
    </div>
  );
}

export function DetailRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[7rem_1fr] gap-3 py-2 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className="min-w-0 break-words">{children}</span>
    </div>
  );
}
