import { CircleCheck as CircleCheckIcon, OctagonX as OctagonXIcon } from "@/components/icons"

import { useToast } from "@/hooks/use-toast"
import {
  Toast,
  ToastClose,
  ToastDescription,
  ToastProvider,
  ToastTitle,
  ToastViewport,
} from "@/components/ui/toast"

export function Toaster() {
  const { toasts } = useToast()

  return (
    <ToastProvider duration={4000}>
      {toasts.map(function ({ id, title, description, action, icon, variant, ...props }) {
        return (
          <Toast key={id} variant={variant} {...props}>
            <span data-slot="toast-icon" className="shrink-0">
              {icon ?? (variant === "destructive" ? <OctagonXIcon aria-hidden="true" /> : <CircleCheckIcon aria-hidden="true" />)}
            </span>
            <div className="grid min-w-0 flex-1 gap-1">
              {title && <ToastTitle>{title}</ToastTitle>}
              {description && (
                <ToastDescription className="break-words">{description}</ToastDescription>
              )}
            </div>
            {action}
            <ToastClose />
          </Toast>
        )
      })}
      <ToastViewport />
    </ToastProvider>
  )
}
