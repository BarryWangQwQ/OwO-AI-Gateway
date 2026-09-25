import { useRef, useState, type ReactNode } from "react";
import { cn } from "cn";

import { CircleCheck, CircleX, Info, TriangleAlert } from "@/components/icons";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";

export type DiagnosticLevel = "error" | "warning" | "info";
export type Diagnostic = { level: DiagnosticLevel; text: ReactNode };

const LEVELS: Record<DiagnosticLevel, { icon: typeof Info; className: string }> = {
  error: { icon: CircleX, className: "text-destructive" },
  warning: { icon: TriangleAlert, className: "text-amber-500" },
  info: { icon: Info, className: "text-muted-foreground" },
};

/** Hover grace period, so the pointer can travel from the icon to the panel. */
const CLOSE_DELAY_MS = 120;

/**
 * One status icon for a list of findings: red when anything is an error, amber for warnings, a green check otherwise.
 * Hovering shows the list; clicking pins it open until clicked again or dismissed. It never takes focus, so it can sit
 * next to an editor.
 */
export function Diagnostics({ items, label, className }: { items: Diagnostic[]; label: string; className?: string }) {
  const [open, setOpen] = useState(false);
  const pinned = useRef(false);
  const closing = useRef<number | undefined>(undefined);

  const errors = items.filter((d) => d.level === "error").length;
  const warnings = items.filter((d) => d.level === "warning").length;
  const worst = errors ? "error" : warnings ? "warning" : null;
  const Icon = worst ? LEVELS[worst].icon : CircleCheck;
  // The badge counts what the icon's colour stands for: errors when there are any, otherwise warnings.
  const count = errors || warnings;

  const show = () => {
    window.clearTimeout(closing.current);
    setOpen(true);
  };
  const hide = () => {
    if (pinned.current) return;
    window.clearTimeout(closing.current);
    closing.current = window.setTimeout(() => setOpen(false), CLOSE_DELAY_MS);
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        if (!next) pinned.current = false;
        setOpen(next);
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={label}
          className={cn(
            "relative inline-flex size-7 items-center justify-center rounded-full outline-none transition-colors hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/30",
            worst ? LEVELS[worst].className : "text-emerald-500",
            className,
          )}
          onMouseEnter={show}
          onMouseLeave={hide}
          onClick={(e) => {
            e.preventDefault();
            pinned.current = !pinned.current;
            setOpen(pinned.current);
          }}
        >
          <Icon className="size-4" />
          {count > 0 && (
            <span
              className={cn(
                "absolute -top-0.5 -right-0.5 flex h-3.5 min-w-3.5 items-center justify-center rounded-full px-1 text-[10px] leading-none font-semibold tabular-nums text-white ring-2 ring-popover",
                errors ? "bg-destructive" : "bg-amber-500",
              )}
            >
              {count > 9 ? "9+" : count}
            </span>
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        side="top"
        className="w-80 gap-2 p-3"
        onOpenAutoFocus={(e) => e.preventDefault()}
        onMouseEnter={show}
        onMouseLeave={hide}
      >
        {items.map((d, i) => {
          const { icon: LevelIcon, className: tone } = LEVELS[d.level];
          return (
            <div key={i} className="flex items-start gap-2 text-xs leading-relaxed">
              <LevelIcon className={cn("mt-0.5 size-3.5 shrink-0", tone)} />
              <span className="min-w-0 break-words">{d.text}</span>
            </div>
          );
        })}
      </PopoverContent>
    </Popover>
  );
}
