import * as React from "react"
import { ChevronRight } from "lucide-react"
import { cn } from "@/lib/utils"
import type { FlatCase } from "@/data"
import type { Case, Relation } from "@/data/types"
import type { Verdict } from "@/lib/run-state"
import { GutterSegment, VerdictControl } from "@/components/verdict"
import { Terminal } from "@/components/terminal"
import { Prose } from "@/components/prose"
import { Badge } from "@/components/ui/badge"

const surfaceLabel: Record<Case["surface"], string> = {
  terminal: "Terminal",
  claude: "Claude",
  dashboard: "Dashboard",
  editor: "Editor",
  files: "Files",
}

const flagLabel: Record<NonNullable<Case["flag"]>, string> = {
  divergence: "Divergence",
  gap: "Gap",
  trap: "Trap",
}

const relationLabel: Record<Relation, string> = {
  depends: "depends on",
  diverges: "diverges from",
  contrasts: "contrasts with",
}

/**
 * One labelled section of the expanded panel.
 *
 * The label sits right-aligned in a 3.5rem gutter with content starting at
 * 5.5rem, so nine possible sections share one hard vertical rule rather than
 * nine headings interrupting the prose. Below 640px the grid collapses and the
 * label stacks above its content.
 */
function Row({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <>
      <dt className="label mt-3.5 text-muted-foreground first:mt-0 sm:mt-0 sm:text-right">
        {label}
      </dt>
      <dd className="min-w-0">{children}</dd>
    </>
  )
}

/** Prose shared by Why, Before, After, Note and the Verify commentary. */
function Text({ children }: { children: string }) {
  return (
    <p className="max-w-[68ch] text-[13px] leading-relaxed text-muted-foreground">
      <Prose>{children}</Prose>
    </p>
  )
}

/**
 * One check.
 *
 * Memoised, and the two callbacks hand the row's own id back rather than
 * closing over it, so App can pass every row the same pair. A closure per row
 * would be a fresh prop on each App render and this memo would never hold --
 * and App re-renders on every keystroke, every j/k move, and every scroll into
 * a new journey.
 */
export const CaseRow = React.memo(function CaseRow({
  item,
  verdict,
  onVerdict,
  open,
  onToggle,
  focused,
}: {
  item: FlatCase
  verdict: Verdict
  onVerdict: (id: string, v: Verdict) => void
  open: boolean
  onToggle: (id: string) => void
  focused: boolean
}) {
  const panelId = `case-panel-${item.id}`
  // Only a verdict recorded in this session animates. Re-rendering a restored
  // run should not replay 150 animations on load.
  const first = React.useRef(true)
  React.useEffect(() => {
    first.current = false
  }, [])

  const toggle = React.useCallback(() => onToggle(item.id), [onToggle, item.id])
  const record = React.useCallback(
    (v: Verdict) => onVerdict(item.id, v),
    [onVerdict, item.id]
  )

  return (
    <li
      id={`case-${item.id}`}
      data-case={item.id}
      className={cn(
        "relative border-b border-border/70 last:border-b-0",
        focused && "bg-accent/40"
      )}
    >
      {/* The gutter. One segment per case, running the full height of the row
          with no gap above or below, so the rail down the page is a single
          unbroken readout of the whole run rather than a column of marks. */}
      <GutterSegment
        verdict={verdict}
        animate={!first.current}
        className="absolute inset-y-0 left-0 w-[4px]"
      />

      <div className="flex flex-col gap-1.5 py-2.5 pr-3 pl-5 sm:flex-row sm:items-start sm:gap-3">
        <button
          type="button"
          onClick={toggle}
          aria-expanded={open}
          aria-controls={panelId}
          className="focus-ring group flex min-w-0 flex-1 items-start gap-3 text-left"
        >
          <ChevronRight
            aria-hidden="true"
            className={cn(
              "mt-[3px] size-3.5 shrink-0 text-muted-foreground transition-transform",
              open && "rotate-90"
            )}
          />
          <span className="mt-px w-9 shrink-0 font-mono text-[12px] text-muted-foreground tabular-nums">
            {item.id}
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-[14px] leading-snug font-medium group-hover:text-primary">
              {item.title}
            </span>
            <span className="mt-0.5 block text-[13px] leading-snug text-muted-foreground">
              {item.assert}
            </span>
          </span>
        </button>

        <div className="flex shrink-0 items-center gap-2 pl-[1.6rem] sm:pt-px sm:pl-0">
          {item.flag ? (
            <Badge variant="outline">{flagLabel[item.flag]}</Badge>
          ) : null}
          <span className="label shrink-0 text-muted-foreground sm:w-16 sm:text-right">
            {surfaceLabel[item.surface]}
          </span>
          <VerdictControl verdict={verdict} onChange={record} size="sm" />
        </div>
      </div>

      {open ? (
        <div id={panelId} className="pt-1 pr-3 pb-5 pl-5">
          <dl className="grid grid-cols-1 gap-y-1 sm:grid-cols-[3.5rem_minmax(0,1fr)] sm:gap-x-3 sm:gap-y-4">
            {item.why ? (
              <Row label="Why">
                <Text>{item.why}</Text>
              </Row>
            ) : null}

            {item.precondition ? (
              <Row label="Before">
                <Text>{item.precondition}</Text>
              </Row>
            ) : null}

            <Row label="Run">
              <Terminal copy>{item.command}</Terminal>
            </Row>

            <Row label="Expected">
              <Terminal tone="output">{item.expected}</Terminal>
            </Row>

            {item.verify ? (
              <Row label="Verify">
                <div className="space-y-2">
                  {item.verify.command ? (
                    <Terminal copy>{item.verify.command}</Terminal>
                  ) : null}
                  <Text>{item.verify.look}</Text>
                </div>
              </Row>
            ) : null}

            {item.after ? (
              <Row label="After">
                <Text>{item.after}</Text>
              </Row>
            ) : null}

            {item.notes ? (
              <Row label="Note">
                <Text>{item.notes}</Text>
              </Row>
            ) : null}

            {item.related?.length ? (
              <Row label="Related">
                <ul className="flex flex-wrap gap-x-4 gap-y-1.5">
                  {item.related.map((r) => (
                    <li key={r.id} className="text-[13px] leading-relaxed">
                      <a
                        href={`#case-${r.id}`}
                        className="focus-ring rounded-sm font-mono text-foreground underline decoration-border underline-offset-2 hover:decoration-foreground"
                      >
                        {r.id}
                      </a>
                      <span className="ml-1.5 text-muted-foreground">
                        {relationLabel[r.rel]}
                      </span>
                    </li>
                  ))}
                </ul>
              </Row>
            ) : null}

            {item.source ? (
              <Row label="Source">
                <p className="max-w-[68ch] text-[12.5px] leading-relaxed text-muted-foreground/80">
                  {item.source}
                </p>
              </Row>
            ) : null}
          </dl>
        </div>
      ) : null}
    </li>
  )
})
