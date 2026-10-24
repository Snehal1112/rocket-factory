import { allCases, source, suites } from "@/data"
import type { Verdict } from "@/lib/run-state"

/**
 * A plain-text summary a reader can paste straight into a support ticket or a
 * standup message. Failures come first and carry the expected output, the
 * mechanism and the verification command, because "A7 failed, expected 403,
 * got 200, and here is why it should have been 403" is one somebody else can
 * act on.
 */
export function buildReport(verdictOf: (id: string) => Verdict): string {
  const pass = allCases.filter((c) => verdictOf(c.id) === "pass").length
  const fail = allCases.filter((c) => verdictOf(c.id) === "fail")
  const todo = allCases.length - pass - fail.length

  const lines: string[] = [
    "Rocket Factory Journeybook — progress summary",
    `Source: ${source.doc} (${source.branch}, as of ${source.asOf})`,
    `Recorded: ${new Date().toISOString()}`,
    "",
    `Worked:   ${pass}`,
    `Stuck:    ${fail.length}`,
    `To do:    ${todo}`,
    `Total:    ${allCases.length}`,
    "",
  ]

  if (fail.length) {
    lines.push("STUCK", "=====", "")
    for (const c of fail) {
      lines.push(`${c.id}  ${c.suiteKey} — ${c.title}`)
      lines.push(`  surface:  ${c.surface}`)
      lines.push(`  asserts:  ${c.assert}`)
      lines.push(`  command:  ${c.command.split("\n")[0]}`)
      lines.push(
        `  expected: ${c.expected.split("\n").slice(0, 3).join(" / ")}`
      )
      if (c.why) {
        lines.push(`  why:      ${c.why.replace(/\n/g, " ")}`)
      }
      if (c.verify?.command) {
        lines.push(`  verify:   ${c.verify.command.split("\n")[0]}`)
      }
      lines.push("")
    }
  }

  lines.push("BY JOURNEY", "==========", "")
  for (const s of suites) {
    const p = s.cases.filter((c) => verdictOf(c.id) === "pass").length
    const f = s.cases.filter((c) => verdictOf(c.id) === "fail").length
    const suffix = f ? `  (${f} stuck)` : ""
    lines.push(`${s.key}  ${p}/${s.cases.length}${suffix}  ${s.title}`)
  }

  return lines.join("\n")
}
