import {
  commandGroups,
  corrections,
  flagTraps,
  globalFlags,
  glossary,
  messageTable,
  programMatrix,
  setupScript,
} from "@/data/reference"
import * as React from "react"
import { ChevronDown } from "lucide-react"
import { Prose } from "@/components/prose"
import { Terminal } from "@/components/terminal"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import { cn } from "@/lib/utils"

/** Remembers which prep panels a reader has folded away. */
const OPEN_KEY = "journeybook-panels-v1"

function readOpen(id: string, fallback: boolean): boolean {
  try {
    const raw = localStorage.getItem(OPEN_KEY)
    if (!raw) return fallback
    const map = JSON.parse(raw) as Record<string, boolean>
    return map[id] ?? fallback
  } catch {
    return fallback
  }
}

function writeOpen(id: string, open: boolean) {
  try {
    const raw = localStorage.getItem(OPEN_KEY)
    const map = raw ? (JSON.parse(raw) as Record<string, boolean>) : {}
    map[id] = open
    localStorage.setItem(OPEN_KEY, JSON.stringify(map))
  } catch {
    // Storage unavailable. The panel still opens and closes for this session.
  }
}

function Panel({
  id,
  title,
  lede,
  children,
  collapsible,
}: {
  id: string
  title: string
  lede?: string
  children: React.ReactNode
  /**
   * Set on the two panels that sit above the checks. They are prerequisites,
   * so they open by default -- but a reader on their third session has
   * already run setup and wants the checks nearer the top, so the choice is
   * remembered rather than reset on every load.
   */
  collapsible?: boolean
}) {
  const [open, setOpen] = React.useState(true)

  // Read persisted state after mount: reading localStorage during the first
  // render would make the markup depend on storage that can throw.
  React.useEffect(() => {
    if (collapsible) setOpen(readOpen(id, true))
  }, [collapsible, id])

  const onOpenChange = (next: boolean) => {
    setOpen(next)
    writeOpen(id, next)
  }

  const heading = (
    <>
      <h2 id={`${id}-heading`} className="text-[15px] font-semibold">
        {title}
      </h2>
      {lede ? (
        <p className="mt-1 max-w-[74ch] text-[13px] leading-relaxed text-muted-foreground">
          <Prose>{lede}</Prose>
        </p>
      ) : null}
    </>
  )

  return (
    <section
      id={id}
      aria-labelledby={`${id}-heading`}
      // Same clearance the journey sections use: two standing bands overhead,
      // the masthead's readout and the checks toolbar.
      className="scroll-mt-[calc(var(--pin-h)+4.5rem)] rounded-lg border border-border bg-card"
    >
      {/* The same registry Collapsible the rail's panels use, rather than the
          hand-wired aria-expanded / aria-controls pair this carried before.
          base-ui derives both from the trigger and panel it owns, so the ids
          cannot fall out of step, and the two panel implementations on this
          page stop diverging for no reason.

          It ships no classes of its own -- collapsible.tsx is a pass-through
          that adds only a data-slot -- so the trigger keeps the exact class
          string the button had, .focus-ring included. The unstyled root div
          between the section and its children is inert in block flow. */}
      {collapsible ? (
        <Collapsible open={open} onOpenChange={onOpenChange}>
          <CollapsibleTrigger
            className={cn(
              "focus-ring flex w-full items-start gap-3 px-5 py-3.5 text-left",
              open && "border-b border-border"
            )}
          >
            <span className="min-w-0 flex-1">{heading}</span>
            <ChevronDown
              aria-hidden="true"
              className={cn(
                "mt-1 size-4 shrink-0 text-muted-foreground transition-transform",
                !open && "-rotate-90"
              )}
            />
          </CollapsibleTrigger>
          <CollapsibleContent>
            <div className="px-5 py-4">{children}</div>
          </CollapsibleContent>
        </Collapsible>
      ) : (
        <>
          <div className="border-b border-border px-5 py-3.5">{heading}</div>
          <div className="px-5 py-4">{children}</div>
        </>
      )}
    </section>
  )
}

export function SetupPanel() {
  return (
    <Panel
      id="setup"
      collapsible
      title="Before you run anything"
      lede="Use a scratch repository you do not mind a few AI-written commits landing in. Every journey below assumes this has already run."
    >
      <Terminal label="One-time setup" copy>
        {setupScript}
      </Terminal>
      <p className="mt-4 text-[13px] leading-relaxed text-muted-foreground">
        <span className="font-medium text-foreground">
          Available on every factory command:{" "}
        </span>
        <code className="rounded-[3px] bg-muted px-1 py-px font-mono text-[0.9em]">
          {globalFlags}
        </code>
      </p>
    </Panel>
  )
}

export function WordsPanel() {
  return (
    <Panel
      id="words"
      collapsible
      title="Words you will meet"
      lede="The factory uses a few terms the same way every time. Skim these once and the messages make sense."
    >
      <div className="overflow-x-auto">
        <table className="w-full text-left text-[13px]">
          <thead className="text-muted-foreground">
            <tr className="border-b border-border">
              <th scope="col" className="py-2 pr-4 font-medium">
                Term
              </th>
              <th scope="col" className="py-2 font-medium">
                What it means
              </th>
            </tr>
          </thead>
          <tbody>
            {glossary.map((g) => (
              <tr
                key={g.term}
                className="border-b border-border/60 last:border-0"
              >
                <td className="py-2 pr-4 align-top font-mono whitespace-nowrap">
                  {g.term}
                </td>
                <td className="py-2 leading-snug text-muted-foreground">
                  <Prose>{g.meaning}</Prose>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Panel>
  )
}

export function MessagePanel() {
  return (
    <Panel
      id="messages"
      title="What a message means"
      lede="When a job stops, the reason is printed. Find it here, then see what to do about it."
    >
      <dl className="space-y-3.5">
        {messageTable.map((e) => (
          <div
            key={e.message}
            className="grid gap-1.5 md:grid-cols-[1.2fr_1fr] md:gap-6"
          >
            <dt className="transcript rounded border border-term-border bg-term px-2.5 py-1.5 text-term-foreground">
              <span className="transcript-line">{e.message}</span>
            </dt>
            <dd className="self-center text-[13px] leading-snug text-muted-foreground">
              <Prose>{e.action}</Prose>
            </dd>
          </div>
        ))}
      </dl>
    </Panel>
  )
}

export function ProgramPanel() {
  return (
    <Panel
      id="programs"
      title="factory or factoryd?"
      lede="`factory` does the work for one project. `factoryd` is an optional background supervisor that queues and runs `factory` for you. Stopping `factoryd` never stops a running job."
    >
      <div className="overflow-x-auto">
        <table className="w-full text-left text-[13px]">
          <thead className="text-muted-foreground">
            <tr className="border-b border-border">
              <th scope="col" className="py-2 pr-4 font-medium">
                You want to
              </th>
              <th scope="col" className="py-2 pr-4 font-medium">
                factory
              </th>
              <th scope="col" className="py-2 font-medium">
                factoryd
              </th>
            </tr>
          </thead>
          <tbody>
            {programMatrix.map((r) => (
              <tr
                key={r.want}
                className="border-b border-border/60 last:border-0"
              >
                <td className="py-2 pr-4">{r.want}</td>
                <td className="py-2 pr-4 font-mono whitespace-nowrap">
                  {r.factory === "none" ? (
                    <span className="text-muted-foreground">none</span>
                  ) : (
                    r.factory
                  )}
                </td>
                <td className="py-2 font-mono">
                  {r.factoryd === "none" ? (
                    <span className="text-muted-foreground">none</span>
                  ) : (
                    r.factoryd
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Panel>
  )
}

export function CommandPanel() {
  return (
    <Panel
      id="commands"
      title="Command groups and flag traps"
      lede="Every command `factory` and `factoryd` register, then the flags that do less, or something other, than you would guess."
    >
      <div className="overflow-x-auto">
        <table className="w-full text-left text-[13px]">
          <tbody>
            {commandGroups.map((g) => (
              <tr key={g.group} className="border-b border-border/60">
                <td className="py-1.5 pr-5 font-mono whitespace-nowrap">
                  {g.group}
                </td>
                <td className="py-1.5 font-mono text-muted-foreground">
                  {g.subcommands}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <dl className="mt-6 space-y-3">
        {flagTraps.map((t) => (
          <div
            key={t.command}
            className="grid gap-1 md:grid-cols-[minmax(0,17rem)_1fr] md:gap-6"
          >
            <dt className="font-mono text-[13px] leading-snug">{t.command}</dt>
            <dd className="text-[13px] leading-relaxed text-muted-foreground">
              {t.gotcha}
            </dd>
          </div>
        ))}
      </dl>
    </Panel>
  )
}

export function CorrectionsPanel() {
  return (
    <Panel
      id="corrections"
      title="Older docs that are wrong"
      lede="Each of these appears in an older doc and was disproved against the programs. If a note in your team still says the left-hand column, it needs fixing."
    >
      <ol className="space-y-4">
        {corrections.map((c) => (
          <li
            key={c.n}
            className="grid gap-1.5 md:grid-cols-[1fr_1.3fr] md:gap-6"
          >
            <p className="text-[13px] leading-relaxed text-muted-foreground line-through decoration-1">
              {c.claimed}
            </p>
            <p className="text-[13px] leading-relaxed">
              <Prose>{c.actual}</Prose>
            </p>
          </li>
        ))}
      </ol>
    </Panel>
  )
}
