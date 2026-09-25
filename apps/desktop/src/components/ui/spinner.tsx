import { cn } from "cn"
import { LoaderCircle } from "@/components/icons"
import { useTranslation } from "react-i18next"

function Spinner({ className, ...props }: React.ComponentProps<"svg">) {
  const { t } = useTranslation()
  return (
    <LoaderCircle data-slot="spinner" role="status" aria-hidden={false} aria-label={t("common.loading")} className={cn("size-4 animate-spin", className)} {...props} />
  )
}

export { Spinner }
