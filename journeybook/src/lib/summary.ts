import { caseCount, suites } from "@/data"
import type { Verdict } from "@/lib/run-state"

/** One case's mark in a strip. The title rides along for the hover tooltip. */
export interface Tick {
  id: string
  title: string
  verdict: Verdict
}

/** One journey's share of the run. */
export interface SuiteTally {
  /** The journey letter, A-U. */
  key: string
  title: string
  /** Case count. This is also the group's flex-grow in the run strip. */
  count: number
  pass: number
  fail: number
  ticks: Tick[]
}

export interface RunSummary {
  groups: SuiteTally[]
  pass: number
  fail: number
  todo: number
  done: number
  /** Journey letters holding at least one failure, in document order. */
  failedSuites: string[]
}

/**
 * Every number the masthead needs, derived in one pass over the run.
 *
 * The counts used to be recomputed by whoever wanted them -- two filters in
 * App for the totals, two more per suite in the section renderer, another map
 * and two filters per suite in the rail. Memoise this on the run map instead
 * and the whole page stops re-tallying 208 cases on every search keystroke.
 */
export function summarise(run: Record<string, Verdict>): RunSummary {
  let pass = 0
  let fail = 0
  const failedSuites: string[] = []

  const groups = suites.map((s) => {
    let p = 0
    let f = 0
    const ticks = s.cases.map((c) => {
      // Same rule as run-state's verdictOf: "todo" is stored as absence.
      const verdict: Verdict = run[c.id] ?? "todo"
      if (verdict === "pass") p += 1
      else if (verdict === "fail") f += 1
      return { id: c.id, title: c.title, verdict }
    })

    pass += p
    fail += f
    if (f > 0) failedSuites.push(s.key)

    return {
      key: s.key,
      title: s.title,
      count: s.cases.length,
      pass: p,
      fail: f,
      ticks,
    }
  })

  return {
    groups,
    pass,
    fail,
    todo: caseCount - pass - fail,
    done: pass + fail,
    failedSuites,
  }
}
