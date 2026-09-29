"use client"

// Compatibility layer between the app's existing call sites and Base UI's toast
// manager (see components/ui/toast.tsx).
//
// The app was written against the Radix toast API: `toast({ variant, title,
// description, action })`, where `action` is a `<ToastAction>` element. Base UI
// takes `type` plus a plain `actionProps` object, and owns the toast list
// itself instead of going through a reducer. Translating here keeps every call
// site reading the way it already does while the rendering, stacking and
// dismissal are all Base UI's.
//
// New code should import `toast` from "@/components/ui/toast" and call
// `toast.add({ type: "success", ... })` directly; this file exists for the
// existing call sites and can be retired once they are migrated.

import type * as React from "react"

import { toast as manager, useToastManager } from "@/components/ui/toast"

/**
 * A `<ToastAction>` element; its `children` and `onClick` become `actionProps`.
 * `altText` is accepted for call-site compatibility but ignored — Base UI's
 * action is a plain button whose label already comes from `children`.
 */
type ToastActionElement = React.ReactElement<{
  children?: React.ReactNode
  onClick?: React.MouseEventHandler<HTMLButtonElement>
  altText?: string
}>

type ToastOptions = {
  title?: React.ReactNode
  description?: React.ReactNode
  /** `destructive` maps to Base UI's `error` type, `default` to `success`. */
  variant?: "default" | "destructive" | null
  action?: ToastActionElement
}

/**
 * Maps a legacy toast onto Base UI's options.
 *
 * A non-destructive toast used to render a check mark, so it maps to `success`;
 * `destructive` maps to `error`. Callers wanting `info`, `warning` or `loading`
 * should use `toast.add` from "@/components/ui/toast" directly.
 */
function toOptions({ variant, action, ...rest }: ToastOptions) {
  return {
    ...rest,
    type: variant === "destructive" ? "error" : "success",
    ...(action
      ? {
          actionProps: {
            children: action.props.children,
            onClick: action.props.onClick,
          },
        }
      : {}),
  }
}

/** Legacy-compatible `toast()`; returns the same handle the old API returned. */
function toast({ ...props }: ToastOptions) {
  const id = manager.add(toOptions(props))

  return {
    id,
    dismiss: () => manager.close(id),
    update: (next: ToastOptions) => manager.update(id, toOptions(next)),
  }
}

/** Legacy-compatible `useToast()`. */
function useToast() {
  const { toasts } = useToastManager()
  return {
    toasts,
    toast,
    dismiss: (toastId?: string) => manager.close(toastId),
  }
}

export { useToast, toast }
export type { ToastActionElement, ToastOptions }
