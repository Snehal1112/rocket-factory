import { memo } from "react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import type { Verdict } from "@/lib/run-state"
import type { SuiteTally } from "@/lib/summary"

/**
 * The one thing the rail's strip and the masthead's strip actually share.
 * Everything else about them differs -- height, shape, semantics and scale --
 * so they stay two components rather than one with two modes.
 *
 * Spelled out as whole class names on purpose: Tailwind reads source as plain
 * text and never sees a class assembled by concatenation.
 */
const fill: Record<Verdict, string> = {
  pass: "bg-success",
  fail: "bg-destructive",
  todo: "bg-untested",
}

/** How a verdict is said in a tooltip. */
const said: Record<Verdict, string> = {
  pass: "Worked",
  fail: "Stuck",
  todo: "To do",
}

/**
 * A single segment of the verdict gutter -- the unbroken rail running down the
 * left of the case list. Segments touch, so the rail reads as one continuous
 * readout of the run rather than a column of separate marks.
 *
 * Width carries the state before colour does: untested is a hairline track,
 * a recorded verdict is the full width. That ordering is deliberate, so a
 * half-finished run stays legible in greyscale and to a colourblind reader.
 *
 * This is also the only place --success and --destructive appear at full
 * strength. Everywhere else they are tinted, which is what keeps the rail the
 * thing your eye lands on.
 */
export function GutterSegment({
  verdict,
  animate,
  className,
}: {
  verdict: Verdict
  animate?: boolean
  className?: string
}) {
  return (
    <span
      aria-hidden="true"
      className={cn("flex justify-center transition-colors", className)}
    >
      <span
        className={cn(
          "block h-full origin-center",
          verdict === "pass" && "w-full bg-success",
          verdict === "fail" && "w-full bg-destructive",
          verdict === "todo" && "w-px bg-untested",
          animate && verdict !== "todo" && "verdict-settle"
        )}
      />
    </span>
  )
}

/** The compact per-suite strip in the rail: one tick per case, in order. */
export function TickStrip({
  ticks,
  className,
}: {
  ticks: Array<{ id: string; verdict: Verdict }>
  className?: string
}) {
  return (
    <span
      aria-hidden="true"
      className={cn("flex items-stretch gap-px", className)}
    >
      {ticks.map((t) => (
        <span
          key={t.id}
          className={cn("h-1.5 flex-1 rounded-full", fill[t.verdict])}
        />
      ))}
    </span>
  )
}

/**
 * The whole run as one strip: a tick per check, grouped into the journeys,
 * each group as wide as its share of the checks. Where the rail's TickStrip
 * answers "how is this journey going", this answers "where in the run are the
 * failures" -- the question a flat percentage bar cannot answer at all.
 *
 * A failure is drawn taller as well as red, so it reads as a shape. That
 * survives greyscale, a colourblind reader, and a printer dropping
 * backgrounds, which is the same ordering GutterSegment above is built on.
 *
 * The group is the link; ticks are not individually clickable. At four pixels
 * wide they would be a target nobody can hit, and clicking anywhere in a
 * journey's span jumps to the section that holds all of them. That also makes
 * the whole strip a mouse shortcut for something the rail already offers with
 * a real name, which is why it is aria-hidden and out of the tab order -- a
 * screen reader gets the figures below it as plain text instead of a second
 * set of twenty-one identical links.
 *
 * `activeKey` marks the journey the reader is currently scrolled into. It
 * matters because the strip is sticky: once it is on screen for the whole run
 * rather than only at the top of the page, "where in the run are the failures"
 * gains a second half -- "and where am I relative to them". The mark is the
 * same underline hover already uses, at full strength, so the strip grows no
 * new visual device to say it.
 */
export function RunStrip({
  groups,
  activeKey,
  className,
}: {
  groups: SuiteTally[]
  activeKey?: string | null
  className?: string
}) {
  return (
    // Fixed height, not intrinsic. A group is 29px tall until it holds a
    // failure and 36px afterwards, and sizing the strip to its tallest child
    // would shove the whole page down the first time something fails.
    <div
      aria-hidden="true"
      className={cn(
        "flex h-9 items-end gap-1 overflow-hidden border-b border-border xl:gap-[7px]",
        className
      )}
    >
      {groups.map((g) => (
        <RunStripGroup key={g.key} group={g} active={g.key === activeKey} />
      ))}
    </div>
  )
}

/**
 * One journey's link and ticks, split out of RunStrip so a scroll-driven
 * activeKey change only re-renders the two groups whose active state
 * actually flipped. `groups` is a stable-identity array from the caller's
 * useMemo, so memo's shallow prop check holds for every group but those two
 * -- turning "recreate 245 ticks" into "recreate ~2 groups' worth" on the
 * other 21.
 */
const RunStripGroup = memo(function RunStripGroup({
  group,
  active,
}: {
  group: SuiteTally
  active: boolean
}) {
  return (
    <a
      href={`#suite-${group.key}`}
      // Focusable children inside an aria-hidden subtree are an
      // accessibility fault. `inert` would fix that too, but it also kills
      // the click this exists for.
      tabIndex={-1}
      // Grow by case count, so a 23-case journey takes 23/208 of the
      // strip. Inline because the value is data, and Tailwind can only
      // emit class names it literally sees in the source.
      style={{ flex: `${group.count} 1 0` }}
      // border-b-transparent is load-bearing: the base layer applies
      // border-border to *, so a bare border-b-2 draws a grey rule under
      // every group. The clip is per-group as well as on the strip --
      // ticks have a 1px floor, so a squeezed group whose content will not
      // fit paints over its neighbour rather than losing its own tail.
      className={cn(
        "flex min-w-0 items-end gap-px overflow-hidden rounded-[2px] border-b-2 border-b-transparent pb-[5px] transition-colors hover:border-b-primary xl:gap-[2px]",
        active && "border-b-primary"
      )}
    >
      {group.ticks.map((t) => (
        <span
          key={t.id}
          title={`${t.id} — ${t.title} · ${said[t.verdict]}`}
          className={cn(
            "min-w-px flex-1 rounded-[1.5px] transition-colors",
            t.verdict === "fail" ? "h-[29px]" : "h-[22px]",
            fill[t.verdict]
          )}
        />
      ))}
    </a>
  )
})

/**
 * The three-state control, repeated on every one of 200-odd rows. Both states
 * use the registry's own tinted treatment -- a light wash behind coloured text
 * -- rather than a solid fill, so 200 rows of controls never shout over the
 * case titles. The gutter is what carries the run at full strength.
 *
 * The active state is drawn by an outline, not by a deeper wash. That is a
 * contrast requirement rather than a preference: the label is the same hue as
 * the wash behind it, so every step of extra fill eats the contrast between
 * them. In dark the old 20%/30% washes put the label at 2.5-3.6:1, well under
 * AA. Fill is now fixed per theme at a level the label clears comfortably, and
 * hover moves the border instead.
 *
 * Clicking the active verdict clears it, which is what a reader expects from a
 * toggle and saves a third button.
 */
export function VerdictControl({
  verdict,
  onChange,
  size = "xs",
}: {
  verdict: Verdict
  onChange: (v: Verdict) => void
  size?: "xs" | "sm"
}) {
  return (
    <div className="flex items-center gap-0.5">
      <Button
        variant="ghost"
        size={size}
        aria-pressed={verdict === "pass"}
        onClick={() => onChange(verdict === "pass" ? "todo" : "pass")}
        className={cn(
          verdict === "pass"
            ? "border-success bg-success/12 text-success hover:bg-success/20 dark:border-success/60 dark:bg-success/8 dark:hover:bg-success/8"
            : "text-muted-foreground hover:bg-success/8 hover:text-success"
        )}
      >
        Worked
      </Button>
      <Button
        variant="ghost"
        size={size}
        aria-pressed={verdict === "fail"}
        onClick={() => onChange(verdict === "fail" ? "todo" : "fail")}
        className={cn(
          verdict === "fail"
            ? "border-destructive bg-destructive/12 text-destructive hover:bg-destructive/20 dark:border-destructive/60 dark:bg-destructive/8 dark:hover:bg-destructive/8"
            : "text-muted-foreground hover:bg-destructive/8 hover:text-destructive"
        )}
      >
        Stuck
      </Button>
    </div>
  )
}
