import { Check, ChevronRight, Copy, RotateCcw, SkipForward } from "lucide-react"
import * as React from "react"
import type { ComponentType, ReactNode } from "react"
import { cn } from "@/lib/utils"
import type { SuiteTally } from "@/lib/summary"
import { useStoredFlag } from "@/lib/stored-flag"
import { TickStrip } from "@/components/verdict"
import { Button } from "@/components/ui/button"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import { Kbd } from "@/components/ui/kbd"
import { Switch } from "@/components/ui/switch"

const shortcutKeys: Array<[string[], string]> = [
  [["j", "k"], "Move between steps"],
  [["n"], "Jump to the next step to do"],
  [["p", "f"], "Mark worked or stuck"],
  [["x"], "Clear the verdict"],
  [["o"], "Open or close the focused step"],
  [["e"], "Expand every step"],
  [["/"], "Search"],
]

/**
 * A word and a switch, for the two settings that sit in a rail panel's header.
 *
 * Both are settings that stay on rather than actions you fire, which is what a
 * switch means and an icon button or a text link does not. The word is inside
 * the label so it is part of the hit target and part of the accessible name --
 * a switch whose only label is its own on/off state tells a screen reader
 * nothing about what it switches.
 *
 * The explicit `htmlFor`/`id` pair is not redundant with wrapping. base-ui
 * associates the two itself at runtime, but biome's noLabelWithoutControl
 * cannot see through the component boundary to the input inside `Switch`, and
 * an explicit association is a better answer to that than a suppression.
 *
 * The id lands on the hidden `<input type="checkbox">` that `Switch` renders,
 * not on its `role="switch"` button -- base-ui gives that button an id of its
 * own making (`base-ui-*`). So `htmlFor` points at a genuine labelable control
 * rather than at nothing, which is the failure mode a suppression would have
 * hidden. Verified in the browser: `input.labels.length === 1`, and base-ui
 * separately derives `aria-labelledby` on the switch from this same label.
 */
function SwitchField({
  label,
  checked,
  onCheckedChange,
}: {
  label: string
  checked: boolean
  onCheckedChange: () => void
}) {
  const id = React.useId()
  return (
    <label
      htmlFor={id}
      className="flex shrink-0 cursor-pointer items-center gap-2"
    >
      <span className="label text-muted-foreground">{label}</span>
      <Switch
        id={id}
        size="sm"
        checked={checked}
        onCheckedChange={onCheckedChange}
      />
    </label>
  )
}

/**
 * A rail panel whose body folds away, with its heading as the trigger.
 *
 * The chevron sits on the left of the heading rather than the right, so the
 * three panel headings still start on one vertical line and the rail does not
 * grow a second ragged edge. `aside` carries anything that must stay reachable
 * while the panel is shut -- see the Keyboard panel, whose WCAG 2.1.4 switch
 * cannot be folded out of reach.
 *
 * No open/close transition. Tailwind's animation utilities sit in a later
 * cascade layer than the reduced-motion reset in index.css, and a height
 * animation here would buy a 200ms slide in exchange for having to fight that
 * -- the panel snapping is honest and costs the reader nothing.
 */
function CollapsiblePanel({
  title,
  open,
  onToggle,
  aside,
  children,
}: {
  title: string
  open: boolean
  onToggle: () => void
  aside?: ReactNode
  children: ReactNode
}) {
  return (
    <Collapsible
      open={open}
      onOpenChange={onToggle}
      className="rounded-lg border border-border bg-card"
    >
      <div className="flex items-center justify-between gap-2 px-3.5 py-2.5">
        <CollapsibleTrigger className="focus-ring label -my-1 flex min-w-0 items-center gap-1.5 rounded-sm py-1 text-muted-foreground hover:text-foreground">
          <ChevronRight
            aria-hidden="true"
            className={cn(
              "size-3.5 shrink-0 transition-transform",
              open && "rotate-90"
            )}
          />
          {title}
        </CollapsibleTrigger>
        {aside}
      </div>
      <CollapsibleContent>
        <div className="border-t border-border p-3.5">{children}</div>
      </CollapsibleContent>
    </Collapsible>
  )
}

function RailButton({
  children,
  onClick,
  icon: Icon,
}: {
  children: ReactNode
  onClick: () => void
  icon: ComponentType<{ className?: string }>
}) {
  return (
    <Button
      variant="outline"
      size="sm"
      onClick={onClick}
      className="w-full justify-start font-normal"
    >
      <Icon className="size-3.5 shrink-0 text-muted-foreground" />
      {children}
    </Button>
  )
}

/**
 * The run controls, the journey list, and the two reference panels.
 *
 * Takes the tallies `summarise` already produced rather than a `verdictOf` it
 * would have to re-tally 245 cases through. Memoised on top of that: nothing
 * here reads the search box, the filter, or which row holds focus, so App's
 * most frequent re-renders no longer redraw 23 tick strips.
 */
export const Rail = React.memo(function Rail({
  groups,
  activeSuite,
  onNextTodo,
  onReset,
  onCopyReport,
  copied,
  shortcuts,
  onToggleShortcuts,
}: {
  groups: SuiteTally[]
  activeSuite: string | null
  onNextTodo: () => void
  onReset: () => void
  onCopyReport: () => void
  copied: boolean
  shortcuts: boolean
  onToggleShortcuts: () => void
}) {
  /*
   * These three stay here rather than being lifted into App the way `theme`,
   * `shortcuts` and the run are. Each of those is lifted for a reason -- theme
   * lands on <html>, shortcuts gates App's own keydown handler, the run is read
   * by every row on the page. Nothing outside this component reads a rail view
   * preference, so lifting them would copy the shape of that pattern without
   * the reason for it, for six more props.
   */
  const [keyboardOpen, onToggleKeyboard] = useStoredFlag(
    "journeybook-rail-keyboard",
    false
  )
  const [sectionsOpen, onToggleSections] = useStoredFlag(
    "journeybook-rail-sections",
    true
  )
  const [compact, onToggleCompact] = useStoredFlag(
    "journeybook-journeys-compact",
    false
  )

  return (
    <nav
      aria-label="Journeys and run controls"
      // The masthead's standing height plus the 16px the rail always kept
      // above itself; the max-height gives that same total back at the bottom.
      className="flex flex-col gap-5 lg:sticky lg:top-[calc(var(--pin-h)+1rem)] lg:max-h-[calc(100vh-var(--pin-h)-2rem)] lg:overflow-y-auto lg:pb-4"
    >
      <div className="rounded-lg border border-border bg-card p-3.5">
        <h2 className="label text-muted-foreground">Your progress</h2>
        <p className="mt-1.5 text-[12.5px] leading-relaxed text-muted-foreground">
          Saved in this browser only. Nothing is sent anywhere, and clearing
          site data clears your progress.
        </p>
        <div className="mt-3 space-y-1.5">
          <RailButton onClick={onNextTodo} icon={SkipForward}>
            Go to the next step to do
          </RailButton>
          <RailButton onClick={onCopyReport} icon={copied ? Check : Copy}>
            {copied ? "Summary copied" : "Copy a progress summary"}
          </RailButton>
          <RailButton onClick={onReset} icon={RotateCcw}>
            Clear all progress
          </RailButton>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-card">
        <div className="flex items-center justify-between gap-2 border-b border-border px-3.5 py-2">
          <h2 className="label text-muted-foreground">Journeys</h2>
          {/* Density, not a filter: every journey stays in the list either
              way. Compact drops the per-journey tick strip, which the masthead
              already draws for all 208 checks, and holds each title to one
              line -- 21 rows go from ~1274px to ~597px, which is most of the
              reason this rail needed its own scrollbar.

              A switch rather than an icon button because this is a setting
              that stays on, not an action you fire. The word carries the
              meaning; a lone icon here was a guess the reader had to make. */}
          <SwitchField
            label="Compact"
            checked={compact}
            onCheckedChange={onToggleCompact}
          />
        </div>
        <ul className={cn(compact ? "p-1" : "p-1.5")}>
          {groups.map((g) => (
            <li key={g.key}>
              <a
                href={`#suite-${g.key}`}
                aria-current={activeSuite === g.key ? "location" : undefined}
                className={cn(
                  "block rounded-md px-2 transition-colors hover:bg-accent",
                  compact ? "py-1" : "py-1.5",
                  activeSuite === g.key && "bg-accent"
                )}
              >
                <span className="flex items-baseline gap-2">
                  <span className="w-3 shrink-0 font-mono text-[12px] text-muted-foreground">
                    {g.key}
                  </span>
                  {/* truncate, not line-clamp-1: a single-line clamp still
                      reserves the taller line box of a block clamp, which
                      would give back a third of what compact just saved. */}
                  <span
                    className={cn(
                      "min-w-0 flex-1 text-[13px] leading-snug",
                      compact ? "truncate" : "line-clamp-2"
                    )}
                    title={compact ? g.title : undefined}
                  >
                    {g.title}
                  </span>
                  {/* The ratio counts passes, so it stays neutral even when
                      the journey has failures -- tinting it red would read
                      as "8 of 12 is bad". The strip below carries the red. */}
                  <span className="shrink-0 font-mono text-[11.5px] text-muted-foreground tabular-nums">
                    {g.fail > 0 ? (
                      <span
                        className="text-destructive"
                        title={`${g.fail} stuck`}
                      >
                        ✕{g.fail}{" "}
                      </span>
                    ) : null}
                    {g.pass}/{g.count}
                  </span>
                </span>
                {/* Dropped in compact: the masthead's run strip already
                    draws all 208 ticks grouped by journey, and the ✕n and
                    ratio above state the same thing in figures. */}
                {compact ? null : (
                  <TickStrip ticks={g.ticks} className="mt-1.5 ml-5" />
                )}
              </a>
            </li>
          ))}
        </ul>
      </div>

      {/* Collapsed by default: a reader reads the keymap once and then works
          from muscle memory, so it does not earn 270px of standing rail.
          The On/Off switch stays in the header as `aside` rather than inside
          the fold -- it is the WCAG 2.1.4 control, and a shortcut kill-switch
          you have to open a panel to reach is not switchable off in any sense
          that helps someone whose screen reader is fighting for f and e. */}
      <div className="hidden lg:block">
        <CollapsiblePanel
          title="Keyboard"
          open={keyboardOpen}
          onToggle={onToggleKeyboard}
          aside={
            /* Labelled "Shortcuts", not "On"/"Off": the switch already shows
               its own state, so spending the word on that would leave nothing
               saying what is being switched -- which is the whole question a
               screen-reader user has when their f and e keys stop working. */
            <SwitchField
              label="Shortcuts"
              checked={shortcuts}
              onCheckedChange={onToggleShortcuts}
            />
          }
        >
          <dl
            className={cn("space-y-1.5", !shortcuts && "opacity-45 grayscale")}
          >
            {shortcutKeys.map(([keys, label]) => (
              <div key={label} className="flex items-center gap-2">
                <dt className="flex w-14 shrink-0 gap-1">
                  {keys.map((k) => (
                    <Kbd key={k}>{k}</Kbd>
                  ))}
                </dt>
                <dd className="text-[12.5px] leading-snug text-muted-foreground">
                  {label}
                </dd>
              </div>
            ))}
          </dl>
        </CollapsiblePanel>
      </div>

      {/* Two groups because the targets live in two places now: prep sits
          above the checks, reference below them. Open by default -- this is
          navigation, not reference, and a shut nav panel is a dead end. */}
      <CollapsiblePanel
        title="Jump to a section"
        open={sectionsOpen}
        onToggle={onToggleSections}
      >
        <h3 className="label text-muted-foreground">Before you start</h3>
        <ul className="mt-2 space-y-1">
          {[
            ["setup", "Before you run anything"],
            ["words", "Words you will meet"],
          ].map(([id, label]) => (
            <li key={id}>
              <a
                href={`#${id}`}
                className="block text-[13px] leading-snug text-muted-foreground hover:text-foreground"
              >
                {label}
              </a>
            </li>
          ))}
        </ul>

        <h3 className="label mt-4 text-muted-foreground">Look up mid-run</h3>
        <ul className="mt-2 space-y-1">
          {[
            ["messages", "What a message means"],
            ["programs", "factory or factoryd?"],
            ["commands", "Commands and flag traps"],
            ["corrections", "Older docs that are wrong"],
          ].map(([id, label]) => (
            <li key={id}>
              <a
                href={`#${id}`}
                className="block text-[13px] leading-snug text-muted-foreground hover:text-foreground"
              >
                {label}
              </a>
            </li>
          ))}
        </ul>
      </CollapsiblePanel>
    </nav>
  )
})
