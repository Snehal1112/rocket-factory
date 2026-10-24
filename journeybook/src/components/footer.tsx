import { ArrowUp } from "lucide-react"
import { caseCount, source, suites } from "@/data"
import { BrandMark } from "@/components/brand"

/**
 * The page's closing tier. Three bands, in decreasing weight: who this is,
 * where it came from, and what to do when it disagrees with the factory.
 *
 * It used to be a single run-on sentence with "Back to top" floated opposite
 * it on `items-baseline`. That had three faults worth naming, because they are
 * easy to reintroduce:
 *
 *   1. `items-baseline` aligns to the *first* line of a wrapping paragraph, so
 *      the button drifted level with line one of three and read as unplaced.
 *   2. The provenance chips could break mid-token -- `v-4.0.0` wrapped as
 *      "v-" / "4.0.0" across two lines, which reads as a broken build rather
 *      than a branch name. Hence `whitespace-nowrap` on every chip here.
 *   3. Provenance and instruction were the same sentence. A reader filing a
 *      problem wants the first as fields they can copy; the second is advice
 *      and belongs on its own line.
 */
export function Footer({ onBackToTop }: { onBackToTop: () => void }) {
  return (
    <footer className="mt-16 border-t border-border bg-card">
      <div className="mx-auto max-w-[92rem] px-5 py-7 md:px-8">
        {/* Identity. items-center, not items-baseline -- see the note above. */}
        <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-4">
          {/* items-start, not items-center: below sm the descriptor wraps to
              two lines, and centring against a three-line block drops the mark
              into the gap between them. The small top margin anchors it to the
              cap height of the name instead, at every width. */}
          <div className="flex items-start gap-3">
            {/* Decorative: the name is spelled out immediately to its right. */}
            <BrandMark
              aria-hidden="true"
              className="mt-px size-7 shrink-0 rounded-[6px]"
            />
            <div className="min-w-0">
              <p className="text-[14px] leading-tight font-semibold">
                <span className="text-primary">Rocket Factory</span> Journeybook
              </p>
              <p className="label mt-1 text-muted-foreground">
                Offline walkthrough · {suites.length} journeys · {caseCount}{" "}
                steps
              </p>
            </div>
          </div>

          {/* Promoted from bare text to a real control. border-input rather
              than border-border: this bounds something clickable, and that is
              the token that clears the 3:1 WCAG asks of one. */}
          <button
            type="button"
            onClick={onBackToTop}
            className="focus-ring inline-flex items-center gap-1.5 rounded-md border border-input px-3 py-1.5 text-[12.5px] text-muted-foreground hover:border-ring hover:text-foreground"
          >
            <ArrowUp className="size-3.5" aria-hidden="true" />
            Back to top
          </button>
        </div>

        {/* Provenance, as fields rather than prose: a reader filing a problem
            quotes these three verbatim. */}
        <dl className="mt-6 flex flex-wrap gap-x-10 gap-y-4 border-t border-border pt-5">
          <Field label="Source" value={source.doc} />
          <Field label="Branch" value={source.branch} />
          <Field label="As of" value={source.asOf} />
        </dl>

        <p className="mt-5 max-w-[70ch] text-[12.5px] leading-relaxed text-muted-foreground">
          When a step and the factory disagree, run the command with --help and
          trust what it prints — then fix whichever one is wrong.
        </p>
      </div>
    </footer>
  )
}

/**
 * `whitespace-nowrap` is load-bearing, not cosmetic: without it a chip breaks
 * at the hyphen in `v-4.0.0` and at every `/` and `_` in the document path.
 * `overflow-x-auto` is what keeps that safe on a narrow viewport -- the chip
 * scrolls rather than forcing the page to.
 */
function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <dt className="label text-muted-foreground">{label}</dt>
      <dd className="mt-1.5 max-w-full overflow-x-auto">
        <code className="rounded-[3px] bg-muted px-1.5 py-0.5 font-mono text-[12.5px] whitespace-nowrap text-foreground">
          {value}
        </code>
      </dd>
    </div>
  )
}
