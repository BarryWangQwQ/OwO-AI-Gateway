"use client"

// Tooltip on Base UI (https://base-ui.com/react/components/tooltip).
//
// The public surface is deliberately unchanged: the same four exports, and the
// same props the app already passes (`asChild` on the trigger, `side` /
// `align` / `sideOffset` plus `hidden` on the content, `delayDuration` on the
// provider). Only the mechanism underneath is Base UI's.
//
// What Base UI buys over Radix here is the hover grouping on the provider: a
// neighbour opens instantly if the previous tooltip closed within `timeout`
// (400ms by default), which is what keeps tooltip-dense rows — status badges,
// header hints, icon buttons — from re-running the open delay for every item
// the cursor crosses. Radix only had Provider-level `skipDelayDuration` with
// no per-tooltip control; here `delay` / `closeDelay` / `timeout` are three
// independent knobs.
//
// Looks stay on the rhea surface tokens the other overlays use (bg-popover /
// text-popover-foreground / rounded-xl), so this matches popover and dropdown
// in both light and dark themes.

import * as React from "react"
import { cn } from "cn"
import { Tooltip as TooltipPrimitive } from "@base-ui/react/tooltip"

/** `asChild` is the Radix-era prop; Base UI spells the same idea `render`. */
type AsChildProp = { asChild?: boolean }

function TooltipProvider({
  // Radix's name for the same knob. Call sites pass neither, but keeping the
  // alias means an override in either dialect works.
  delayDuration,
  delay = delayDuration ?? 0,
  ...props
}: TooltipPrimitive.Provider.Props & {
  delayDuration?: number
}) {
  return (
    <TooltipPrimitive.Provider data-slot="tooltip-provider" delay={delay} {...props} />
  )
}

function Tooltip({ ...props }: TooltipPrimitive.Root.Props) {
  return <TooltipPrimitive.Root data-slot="tooltip" {...props} />
}

/**
 * `asChild` renders the single child in place of the trigger's own button,
 * which is what Base UI's `render` prop does. When `asChild` is set the child
 * element is handed to `render` and the child slot is left empty; Base UI
 * merges its own handlers, `data-*` state and className onto that element.
 */
function TooltipTrigger({
  asChild,
  children,
  render,
  ...props
}: TooltipPrimitive.Trigger.Props & AsChildProp) {
  const child = React.Children.toArray(children)
  const useChild = asChild && React.isValidElement(child[0])

  return (
    <TooltipPrimitive.Trigger
      data-slot="tooltip-trigger"
      render={useChild ? (child[0] as React.ReactElement) : render}
      {...props}
    >
      {useChild ? undefined : children}
    </TooltipPrimitive.Trigger>
  )
}

function TooltipContent({
  className,
  side = "top",
  sideOffset = 4,
  align = "center",
  alignOffset = 0,
  children,
  ...props
}: TooltipPrimitive.Popup.Props &
  Pick<
    TooltipPrimitive.Positioner.Props,
    "align" | "alignOffset" | "side" | "sideOffset"
  >) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Positioner
        align={align}
        alignOffset={alignOffset}
        side={side}
        sideOffset={sideOffset}
        className="isolate z-50"
      >
        <TooltipPrimitive.Popup
          data-slot="tooltip-content"
          className={cn(
            // `--transform-origin` is Base UI's variable (Radix published it
            // under `--radix-tooltip-content-transform-origin`).
            "z-50 inline-flex w-fit max-w-xs origin-(--transform-origin) items-center gap-1.5 rounded-xl bg-popover px-3 py-1.5 text-xs text-popover-foreground shadow-md ring-1 ring-border has-data-[slot=kbd]:pr-1.5 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 **:data-[slot=kbd]:relative **:data-[slot=kbd]:isolate **:data-[slot=kbd]:z-50 **:data-[slot=kbd]:rounded-lg data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95",
            className
          )}
          {...props}
        >
          {children}
        </TooltipPrimitive.Popup>
      </TooltipPrimitive.Positioner>
    </TooltipPrimitive.Portal>
  )
}

export { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger }
