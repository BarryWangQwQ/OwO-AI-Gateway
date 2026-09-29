"use client"

// Toast on Base UI (https://base-ui.com/react/components/toast).
//
// Mechanics come from Base UI, looks come from the rhea tokens the rest of the
// app uses (bg-popover / rounded-2xl / shadow-lg / ring-foreground/5), so
// toasts stay in the same visual family as dialog, popover and sheet.
//
// The stack is why this is not on Radix: Base UI publishes `--toast-index` /
// `--toast-height` / `--toast-swipe-movement-*` as CSS variables plus
// `data-expanded` / `data-behind` state, which is what makes the peeking,
// scaling stack and the hover-to-expand possible. Radix's toast has no
// equivalent, so it could only ever render a flat list.

import * as React from "react"
import { Toast as ToastPrimitive } from "@base-ui/react/toast"
import { cn } from "cn"
import { useTranslation } from "react-i18next"

import { buttonVariants } from "@/components/ui/button"
import {
  CircleCheck as CircleCheckIcon,
  Info as InfoIcon,
  LoaderCircle as LoaderCircleIcon,
  OctagonX as OctagonXIcon,
  TriangleAlert as TriangleAlertIcon,
  X as XIcon,
} from "@/components/icons"

/**
 * Module-level toast manager, so code outside the provider tree can fire a
 * toast. `Toaster` below hands this same manager to the provider — otherwise
 * `toast` and `useToastManager()` would be two independent stores and
 * `toast.add()` would render nothing.
 */
const toast = ToastPrimitive.createToastManager()

function ToastProvider({ ...props }: ToastPrimitive.Provider.Props) {
  return <ToastPrimitive.Provider {...props} />
}

function ToastPortal({ ...props }: ToastPrimitive.Portal.Props) {
  return <ToastPrimitive.Portal data-slot="toast-portal" {...props} />
}

function ToastViewport({ className, ...props }: ToastPrimitive.Viewport.Props) {
  return (
    <ToastPrimitive.Viewport
      data-slot="toast-viewport"
      className={cn(
        // Mobile: full-width bar pinned to the bottom. sm+: bottom-right card,
        // capped at the 420px the old Radix viewport used.
        "pointer-events-none fixed bottom-4 left-4 z-100 w-[calc(100vw-2rem)] max-w-sm outline-none sm:right-4 sm:left-auto sm:w-[420px] sm:max-w-[calc(100vw-2rem)]",
        className
      )}
      {...props}
    />
  )
}

function Toast({ className, ...props }: ToastPrimitive.Root.Props) {
  return (
    <ToastPrimitive.Root
      data-slot="toast"
      className={cn(
        "group/toast pointer-events-auto absolute right-0 bottom-0 z-[calc(1000-var(--toast-index))] w-full origin-bottom rounded-2xl border bg-popover text-popover-foreground shadow-lg ring-1 ring-foreground/5 will-change-transform outline-none select-none dark:ring-foreground/10",
        // Stack geometry. `--height` follows the measured height so a toast can
        // animate its own height while the ones behind it shift up.
        "[--gap:0.75rem] [--height:var(--toast-frontmost-height,var(--toast-height))] [--offset-y:calc(var(--toast-offset-y)*-1+calc(var(--toast-index)*var(--gap)*-1)+var(--toast-swipe-movement-y))] [--peek:0.75rem] [--scale:calc(max(0,1-(var(--toast-index)*0.1)))] [--shrink:calc(1-var(--scale))]",
        // Collapsed: each toast peeks out from under the one in front and is
        // scaled down by its index. Expanded (hover / focus): full height, laid
        // out with a gap.
        "h-(--height) [transform:translateX(var(--toast-swipe-movement-x))_translateY(calc(var(--toast-swipe-movement-y)-(var(--toast-index)*var(--peek))-(var(--shrink)*var(--height))))_scale(var(--scale))] [transition:transform_500ms_cubic-bezier(0.22,1,0.36,1),opacity_500ms,height_150ms]",
        "after:absolute after:top-full after:left-0 after:h-[calc(var(--gap)+1px)] after:w-full after:content-['']",
        "data-expanded:h-(--toast-height) data-expanded:[transform:translateX(var(--toast-swipe-movement-x))_translateY(var(--offset-y))]",
        // `data-limited` is the toast pushed off the back of the stack.
        "data-limited:opacity-0 data-starting-style:[transform:translateY(150%)]",
        // Enter from below; leave in whichever direction it was swiped.
        "[&[data-ending-style]:not([data-limited]):not([data-swipe-direction])]:[transform:translateY(150%)]",
        "data-ending-style:data-[swipe-direction=down]:[transform:translateY(calc(var(--toast-swipe-movement-y)+150%))]",
        "data-ending-style:data-[swipe-direction=left]:[transform:translateX(calc(var(--toast-swipe-movement-x)-150%))_translateY(var(--offset-y))]",
        "data-ending-style:data-[swipe-direction=right]:[transform:translateX(calc(var(--toast-swipe-movement-x)+150%))_translateY(var(--offset-y))]",
        "data-ending-style:data-[swipe-direction=up]:[transform:translateY(calc(var(--toast-swipe-movement-y)-150%))]",
        "data-expanded:data-ending-style:data-[swipe-direction=down]:[transform:translateY(calc(var(--toast-swipe-movement-y)+150%))]",
        "data-expanded:data-ending-style:data-[swipe-direction=left]:[transform:translateX(calc(var(--toast-swipe-movement-x)-150%))_translateY(var(--offset-y))]",
        "data-expanded:data-ending-style:data-[swipe-direction=right]:[transform:translateX(calc(var(--toast-swipe-movement-x)+150%))_translateY(var(--offset-y))]",
        "data-expanded:data-ending-style:data-[swipe-direction=up]:[transform:translateY(calc(var(--toast-swipe-movement-y)-150%))]",
        className
      )}
      {...props}
    />
  )
}

function ToastContent({ className, ...props }: ToastPrimitive.Content.Props) {
  return (
    <ToastPrimitive.Content
      data-slot="toast-content"
      className={cn(
        // Match the reference stack: hide the contents of cards behind the front one.
        "flex h-full items-center gap-3 overflow-hidden p-4 transition-opacity duration-250 ease-[cubic-bezier(0.22,1,0.36,1)] data-behind:opacity-0 data-expanded:opacity-100",
        className
      )}
      {...props}
    />
  )
}

function ToastTitle({ className, ...props }: ToastPrimitive.Title.Props) {
  return (
    <ToastPrimitive.Title
      data-slot="toast-title"
      className={cn("text-sm font-medium", className)}
      {...props}
    />
  )
}

function ToastDescription({
  className,
  ...props
}: ToastPrimitive.Description.Props) {
  return (
    <ToastPrimitive.Description
      data-slot="toast-description"
      className={cn("text-sm text-muted-foreground break-words", className)}
      {...props}
    />
  )
}

/** Base UI's `render` may be an element or a function; either way, put `content` inside it. */
function withContent(
  render: NonNullable<ToastPrimitive.Action.Props["render"]>,
  content: React.ReactNode
) {
  if (typeof render === "function") {
    return (props: unknown, state: unknown) =>
      React.cloneElement(
        render(props as never, state as never) as React.ReactElement,
        undefined,
        content
      )
  }
  return React.cloneElement(render as React.ReactElement, undefined, content)
}

/** `altText` is the Radix-era prop; call sites still pass it, so allow it here. */
type ToastActionProps = ToastPrimitive.Action.Props & { altText?: string }

function ToastAction({
  className,
  render = <button className={buttonVariants({ variant: "outline", size: "sm" })} />,
  children,
  altText: _altText,
  ...props
}: ToastActionProps) {
  // Base UI renders through `render`, so the label has to be placed on the
  // element rather than passed as a child of the primitive.
  const content = children ?? "​"
  return (
    <ToastPrimitive.Action
      data-slot="toast-action"
      render={withContent(render, content)}
      className={cn("shrink-0", className)}
      {...props}
    />
  )
}

function ToastClose({
  className,
  children,
  render = <button className={buttonVariants({ variant: "ghost", size: "icon-xs" })} />,
  ...props
}: ToastPrimitive.Close.Props) {
  const { t } = useTranslation()
  return (
    <ToastPrimitive.Close
      data-slot="toast-close"
      aria-label={t("common.closeToast")}
      render={withContent(render, children ?? <XIcon aria-hidden="true" />)}
      className={cn(
        // Always visible; `::after` pads the hit area past the icon.
        "relative shrink-0 text-muted-foreground after:absolute after:-inset-2 after:content-[''] hover:text-foreground",
        className
      )}
      {...props}
    />
  )
}

/** Status icon for a toast `type`. Unknown types render nothing. */
function ToastIcon({ type }: { type: string | undefined }) {
  let icon: React.ReactNode = null

  if (type === "success") {
    icon = <CircleCheckIcon aria-hidden="true" />
  } else if (type === "info") {
    icon = <InfoIcon aria-hidden="true" />
  } else if (type === "warning") {
    icon = <TriangleAlertIcon aria-hidden="true" />
  } else if (type === "error") {
    icon = <OctagonXIcon aria-hidden="true" className="text-destructive" />
  } else if (type === "loading") {
    icon = <LoaderCircleIcon aria-hidden="true" className="animate-spin" />
  }

  if (!icon) return null

  return (
    <span
      data-slot="toast-icon"
      className="shrink-0 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4"
    >
      {icon}
    </span>
  )
}

function ToastList() {
  const { toasts } = ToastPrimitive.useToastManager()

  return toasts.map((toastItem) => (
    <Toast key={toastItem.id} toast={toastItem}>
      <ToastContent>
        <ToastIcon type={toastItem.type} />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <ToastTitle />
          <ToastDescription />
        </div>
        {toastItem.actionProps ? <ToastAction /> : null}
        <ToastClose />
      </ToastContent>
    </Toast>
  ))
}

/**
 * Mount once, near the app root. The module-level manager goes in via
 * `toastManager` so `toast.add()` and `useToastManager()` share one store.
 */
function Toaster({ ...props }: ToastPrimitive.Provider.Props) {
  return (
    <ToastProvider toastManager={toast} {...props}>
      <ToastPortal>
        <ToastViewport>
          <ToastList />
        </ToastViewport>
      </ToastPortal>
    </ToastProvider>
  )
}

const createToastManager = ToastPrimitive.createToastManager
const useToastManager = ToastPrimitive.useToastManager

export {
  Toaster,
  Toast,
  ToastAction,
  ToastClose,
  ToastContent,
  ToastDescription,
  ToastIcon,
  ToastPortal,
  ToastProvider,
  ToastTitle,
  ToastViewport,
  createToastManager,
  toast,
  useToastManager,
}
