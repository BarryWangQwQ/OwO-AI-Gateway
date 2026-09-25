import { useTranslation } from "react-i18next";

import { CircleCheck, CircleSlash, CircleX } from "@/components/icons";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

/**
 * A status icon; the tooltip carries the words. `detail` (e.g. the error message) goes into the tooltip too, so list
 * rows can stay one line high and still surface why a call failed on hover.
 */
export function CallStatusIcon({ status, upstream, detail }: { status: string; upstream?: number | null; detail?: string | null }) {
  const { t } = useTranslation();
  const [Icon, color, label] =
    status === "ok"
      ? [CircleCheck, "text-emerald-500", t("history.status.ok")]
      : status === "cancelled"
        ? [CircleSlash, "text-muted-foreground", t("history.status.cancelled")]
        : [CircleX, "text-destructive", upstream ? t("history.status.errorHttp", { status: upstream }) : t("history.status.error")];
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Icon className={`size-4 shrink-0 ${color}`} aria-label={label} />
      </TooltipTrigger>
      <TooltipContent className={detail ? "flex-col items-start gap-0.5" : undefined}>
        {label}
        {detail && <span className="line-clamp-3 font-mono break-all opacity-80">{detail}</span>}
      </TooltipContent>
    </Tooltip>
  );
}

export function Dot({ on }: { on: boolean }) {
  return <span className={`inline-block size-2 shrink-0 rounded-full ${on ? "bg-emerald-500" : "bg-muted-foreground/40"}`} />;
}
