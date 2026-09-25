import type { ComponentProps } from "react"
import { cn } from "cn"

import { Check } from "@/components/icons"

// shadcn/ui ships no Stepper (only the choice-driven Questionnaire), so this is
// a small local one: numbered circles, labels and connector lines, styled like
// the other primitives. State lives in the caller (`value` / `onValueChange`).

export type StepperStep = { label: string }

type StepState = "completed" | "active" | "upcoming"

function Stepper({
  steps,
  value,
  onValueChange,
  isNavigable,
  className,
  ...props
}: Omit<ComponentProps<"ol">, "onChange"> & {
  steps: StepperStep[]
  /** Index of the active step. */
  value: number
  /** Called when a navigable step is clicked. */
  onValueChange?: (index: number) => void
  /** Which steps may be jumped to by clicking; defaults to the completed ones. */
  isNavigable?: (index: number) => boolean
}) {
  return (
    <ol data-slot="stepper" className={cn("flex w-full items-center gap-2", className)} {...props}>
      {steps.map((step, i) => {
        const state: StepState = i < value ? "completed" : i === value ? "active" : "upcoming"
        const navigable = i !== value && !!onValueChange && (isNavigable ? isNavigable(i) : i < value)
        return (
          <li
            key={i}
            data-slot="stepper-item"
            data-state={state}
            className="flex min-w-0 flex-1 items-center gap-2 last:flex-none"
          >
            <button
              type="button"
              disabled={!navigable}
              aria-current={state === "active" ? "step" : undefined}
              onClick={() => onValueChange?.(i)}
              className={cn(
                "flex min-w-0 items-center gap-2 rounded-full text-sm outline-none transition-colors focus-visible:ring-3 focus-visible:ring-ring/30",
                navigable ? "cursor-pointer hover:text-foreground" : "cursor-default"
              )}
            >
              <span
                data-slot="stepper-indicator"
                className={cn(
                  "flex size-6 shrink-0 items-center justify-center rounded-full border text-xs font-medium tabular-nums transition-colors",
                  state === "active" && "border-primary bg-primary text-primary-foreground",
                  state === "completed" && "border-primary/40 bg-primary/10 text-primary",
                  state === "upcoming" && "border-border text-muted-foreground"
                )}
              >
                {state === "completed" ? <Check className="size-3.5" /> : i + 1}
              </span>
              <span
                data-slot="stepper-title"
                className={cn("truncate font-medium", state === "upcoming" ? "text-muted-foreground" : "text-foreground")}
              >
                {step.label}
              </span>
            </button>
            {i < steps.length - 1 && (
              <span
                aria-hidden
                data-slot="stepper-separator"
                className={cn("h-px min-w-4 flex-1 rounded-full transition-colors", state === "completed" ? "bg-primary/60" : "bg-border")}
              />
            )}
          </li>
        )
      })}
    </ol>
  )
}

export { Stepper }
