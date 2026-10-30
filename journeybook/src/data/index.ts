import { journeysAC } from "./journeys-a-c"
import { journeysDE } from "./journeys-d-e"
import { journeysFH } from "./journeys-f-h"
import type { Case, Suite } from "./types"

export const suites: Suite[] = [...journeysAC, ...journeysDE, ...journeysFH]

/** Every case, flattened, with its suite key attached for search and report. */
export interface FlatCase extends Case {
  suiteKey: string
  suiteTitle: string
}

export const allCases: FlatCase[] = suites.flatMap((s) =>
  s.cases.map((c) => ({ ...c, suiteKey: s.key, suiteTitle: s.title }))
)

export const caseCount = allCases.length

/**
 * The source this journeybook is transcribed from. Shown in the footer so a
 * reader reporting a problem can quote the same document the step came from.
 */
export const source = {
  doc: "docs/guides/factory-user-guide.md",
  branch: "main",
  asOf: "2026-10-30",
}
