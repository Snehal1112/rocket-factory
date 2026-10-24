import { pipeline } from "@/data/reference"
import { Prose } from "@/components/prose"

/**
 * The page opens on this rather than a headline number, because it is the one
 * thing a new user has to hold in their head to read any result correctly: the
 * factory is a fixed line of stages, and a job only ever stops in one of two
 * places -- merged, or waiting for you.
 *
 * The numbering is earned: the stages genuinely run in this order, and a job
 * that stops at stage 3 never reaches stage 5.
 */
export function PipelineDiagram() {
  return (
    <section
      aria-labelledby="pipeline-heading"
      className="rounded-lg border border-border bg-card"
    >
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 border-b border-border px-5 py-3.5">
        <h2 id="pipeline-heading" className="text-[15px] font-semibold">
          The pipeline, in order
        </h2>
        <p className="text-[13px] text-muted-foreground">
          A job ends in one of two places: merged, or waiting for you. The
          factory never pushes.
        </p>
      </div>

      {/* gap-px over a --border ground is what draws the dividers between the
          stages, so a hairline lands in exactly the places a gap exists and
          nowhere else -- no special case at the first or last panel. */}
      <ol className="grid gap-px overflow-hidden rounded-b-lg bg-border md:grid-cols-3">
        {pipeline.map((s) => (
          <li key={s.key} className="flex flex-col gap-2.5 bg-card px-5 py-4">
            <div className="flex items-baseline gap-2.5">
              <span className="font-mono text-[13px] text-muted-foreground tabular-nums">
                {s.n}
              </span>
              <h3 className="text-[15px] leading-none font-semibold">
                {s.name}
              </h3>
            </div>
            <p className="text-[13px] leading-snug text-muted-foreground">
              {s.scope}
            </p>
            {/* The hanging indent lives on the inner span, never on the
                padded box -- see the note on .transcript-line. */}
            <p className="transcript rounded border border-term-border bg-term px-2.5 py-1.5 text-term-foreground">
              <span className="transcript-line">{s.signature}</span>
            </p>
            <p className="text-[13px] leading-relaxed text-muted-foreground">
              <Prose>{s.detail}</Prose>
            </p>
          </li>
        ))}
      </ol>
    </section>
  )
}
