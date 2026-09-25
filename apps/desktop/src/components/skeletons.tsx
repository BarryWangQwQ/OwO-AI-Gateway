import type { ReactNode } from "react";
import { cn } from "cn";

import { Card, CardContent, CardFooter, CardHeader } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { TableBody, TableCell, TableRow } from "@/components/ui/table";

/**
 * First-load placeholders that take exactly the room of what they stand in for, so the page lays out once and the
 * content just fades in over it. Each one mirrors the real component's box model class for class (sizes, gaps, paddings);
 * when the real one changes shape, change its skeleton with it. They are for `loading` (no data yet) only — a
 * `refreshing` query keeps its content mounted instead.
 */

/** `n` copies of `render(i)`, for grids and lists of skeletons. */
export function repeat(n: number, render: (i: number) => ReactNode): ReactNode[] {
  return Array.from({ length: n }, (_, i) => render(i));
}

/** A one-line text placeholder: `line` is the line height the text takes, the bar itself is a bit shorter and centered in it. */
export function TextSkeleton({ line = "h-5", bar = "h-3.5", width, className }: { line?: string; bar?: string; width: string; className?: string }) {
  return (
    <div className={cn("flex items-center", line, className)}>
      <Skeleton className={cn("rounded-md", bar, width)} />
    </div>
  );
}

/**
 * The Apps page card: round icon `size-9`, title (`text-sm leading-tight`) over a subtitle (`text-xs`), the `h-9` status
 * slot on the right; footer with the `size="sm"` select (`h-7`, fills) and one `size-8` icon button. The header is 36px
 * tall either way (icon / status slot), the footer 32px (the button).
 */
export function AppCardSkeleton() {
  return (
    <Card size="sm" className="gap-3" aria-hidden>
      <CardHeader className="flex items-center gap-3">
        <Skeleton className="size-9 shrink-0 rounded-full" />
        <div className="min-w-0 flex-1">
          <TextSkeleton line="h-[17.5px]" bar="h-3" width="w-24" />
          <TextSkeleton line="h-4" bar="h-2.5" width="w-36" className="mt-0.5" />
        </div>
        <div className="flex h-9 shrink-0 items-center self-start">
          <Skeleton className="h-2.5 w-20 rounded-md" />
        </div>
      </CardHeader>
      <CardFooter className="gap-2">
        <Skeleton className="h-7 min-w-0 flex-1 rounded-2xl" />
        <Skeleton className="size-8 rounded-2xl" />
      </CardFooter>
    </Card>
  );
}

/**
 * The Providers page card: `size-8` icon, title (`text-base leading-tight`) with a key icon beside it and a detail line
 * under it, the `size-8` ⋯ button top-right; bottom row of `h-5` badges with the `h-5 w-8` switch on the right.
 */
export function ProviderCardSkeleton() {
  return (
    <Card className="flex flex-col" aria-hidden>
      <CardHeader>
        <div className="flex min-w-0 items-center gap-3">
          <Skeleton className="size-8 shrink-0 rounded-full" />
          <div className="min-w-0">
            <div className="flex h-5 items-center gap-2">
              <Skeleton className="h-3.5 w-28 rounded-md" />
              <Skeleton className="size-3.5 rounded-full" />
            </div>
            <TextSkeleton line="h-4" bar="h-2.5" width="w-32" className="mt-0.5" />
          </div>
        </div>
        <Skeleton className="col-start-2 row-span-2 row-start-1 size-8 self-start justify-self-end rounded-2xl" data-slot="card-action" />
      </CardHeader>
      <CardContent className="mt-auto flex items-end gap-3">
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
          <Skeleton className="h-5 w-24 rounded-2xl" />
          <Skeleton className="h-5 w-20 rounded-2xl" />
          <Skeleton className="h-5 w-10 rounded-2xl" />
        </div>
        <Skeleton className="h-5 w-8 shrink-0 rounded-2xl" />
      </CardContent>
    </Card>
  );
}

/**
 * The MCP page card: `size-8` transport icon, the name (`text-base leading-tight`) with a key icon beside it and the
 * mono command line under it, the `size-8` ⋯ button top-right; a bottom row of `size-7` app chips (`AppToggles`).
 */
export function McpCardSkeleton() {
  return (
    <Card className="flex flex-col" aria-hidden>
      <CardHeader>
        <div className="flex min-w-0 items-center gap-3">
          <Skeleton className="size-8 shrink-0 rounded-full" />
          <div className="min-w-0">
            <div className="flex h-5 items-center gap-2">
              <Skeleton className="h-3.5 w-24 rounded-md" />
              <Skeleton className="size-3.5 rounded-full" />
            </div>
            <TextSkeleton line="h-4" bar="h-2.5" width="w-44" className="mt-0.5" />
          </div>
        </div>
        <Skeleton className="col-start-2 row-span-2 row-start-1 size-8 self-start justify-self-end rounded-2xl" data-slot="card-action" />
      </CardHeader>
      <CardContent className="mt-auto flex min-h-7 flex-wrap items-center gap-1.5">
        {repeat(5, (i) => (
          <Skeleton key={i} className="size-7 rounded-full" />
        ))}
      </CardContent>
    </Card>
  );
}

/**
 * The Skills page card: `size-8` source icon and the one-line name, the `size-8` ⋯ button top-right; two `text-xs`
 * (`leading-relaxed`) description lines, then seven `size-7` app toggles.
 */
export function SkillCardSkeleton() {
  return (
    <Card className="flex flex-col" aria-hidden>
      <CardHeader>
        <div className="flex min-w-0 items-center gap-3">
          <Skeleton className="size-8 shrink-0 rounded-full" />
          <Skeleton className="h-3.5 w-28 rounded-md" />
        </div>
        <Skeleton className="col-start-2 row-span-2 row-start-1 size-8 self-start justify-self-end rounded-2xl" data-slot="card-action" />
      </CardHeader>
      <CardContent className="flex flex-1 flex-col gap-4">
        <div>
          <TextSkeleton line="h-[19.5px]" bar="h-2.5" width="w-full" />
          <TextSkeleton line="h-[19.5px]" bar="h-2.5" width="w-3/5" />
        </div>
        <div className="mt-auto flex flex-wrap items-center gap-1.5">
          {repeat(7, (i) => (
            <Skeleton key={i} className="size-7 rounded-full" />
          ))}
        </div>
      </CardContent>
    </Card>
  );
}

export type SkeletonColumn = {
  /** Bar width class (`w-24`). */
  w: string;
  /** Right-aligned (numeric) columns. */
  right?: boolean;
  /** Bar height class; default `h-3.5` (a `text-sm` line). */
  bar?: string;
};

/**
 * Table rows for a first load, under the real `TableHeader`. Every cell holds a `line`-tall box (default `h-5`, a
 * `text-sm` line; `p-2` makes the row 36px like a text-only row) with a bar of the column's width in it. Put it in the
 * `Table` in place of the `TableBody`; the header's edge inset applies to it as to any body.
 */
export function TableRowsSkeleton({ rows, columns, line = "h-5" }: { rows: number; columns: SkeletonColumn[]; line?: string }) {
  return (
    <TableBody aria-hidden>
      {repeat(rows, (r) => (
        <TableRow key={r} className="hover:bg-transparent">
          {columns.map((c, i) => (
            <TableCell key={i}>
              <div className={cn("flex items-center", line, c.right && "justify-end")}>
                <Skeleton className={cn("rounded-md", c.bar ?? "h-3.5", c.w)} />
              </div>
            </TableCell>
          ))}
        </TableRow>
      ))}
    </TableBody>
  );
}

/** A `Donut` / `Gauge` stand-in: a pulsing ring of the same box (`aspect-square`; `className` sets the height). */
export function RingSkeleton({ className, thickness = "border-[10px]" }: { className?: string; thickness?: string }) {
  return <div aria-hidden className={cn("aspect-square animate-pulse rounded-full border-muted", thickness, className)} />;
}

/** `RankBars` rows: the same `1.5rem` grid rows with a label bar, an empty track (a track is `bg-muted` anyway) and a value bar. */
export function RankBarsSkeleton({ rows, className }: { rows: number; className?: string }) {
  return (
    <div aria-hidden className={cn("grid auto-rows-[1.5rem] grid-cols-[minmax(0,3fr)_minmax(0,2fr)_auto] content-start items-center gap-x-3 text-xs", className)}>
      {repeat(rows, (i) => (
        <div key={i} className="contents">
          <Skeleton className="h-2.5 rounded-md" style={{ width: `${70 - i * 8}%` }} />
          <Skeleton className="h-3 rounded-[3px]" />
          <Skeleton className="h-2.5 w-9 rounded-md" />
        </div>
      ))}
    </div>
  );
}

/** The dashboard's recent-calls rows: status icon, vendor icon, two-line text (`leading-5` + `leading-4`), two-line numbers on the right. */
export function RecentRowsSkeleton({ rows }: { rows: number }) {
  return (
    <div aria-hidden className="space-y-1.5 pr-3">
      {repeat(rows, (i) => (
        <div key={i} className="flex items-center gap-2.5 text-sm">
          <Skeleton className="size-4 shrink-0 rounded-full" />
          <Skeleton className="size-4 shrink-0 rounded-full" />
          <div className="min-w-0 flex-1">
            <TextSkeleton line="h-5" bar="h-3" width={i % 2 ? "w-28" : "w-36"} />
            <TextSkeleton line="h-4" bar="h-2.5" width="w-24" />
          </div>
          <div className="flex flex-col items-end">
            <TextSkeleton line="h-4" bar="h-2.5" width="w-10" />
            <TextSkeleton line="h-4" bar="h-2.5" width="w-12" />
          </div>
        </div>
      ))}
    </div>
  );
}
