/**
 * Fails if any Case.related points at a case id that does not exist, or at
 * itself.
 *
 * A dangling cross-reference renders as a plausible-looking id a tester will
 * go hunting for. That is the misleading outcome the enrichment spec exists to
 * prevent, so it is a build failure rather than a runtime shrug.
 *
 * Bun executes TypeScript directly, so this imports the data modules with no
 * build step -- the same reason check-contrast.mjs can parse index.css.
 */

import { allCases } from "../src/data/index.ts"

const ids = new Set(allCases.map((c) => c.id))
const problems = []

for (const c of allCases) {
  for (const r of c.related ?? []) {
    if (r.id === c.id) {
      problems.push(`${c.id}: related to itself`)
    } else if (!ids.has(r.id)) {
      problems.push(`${c.id}: related id "${r.id}" does not exist`)
    }
  }
}

// A `why`, `verify` or `after` with no `source` is a rule-1 violation: the
// claim cannot be traced, so a reader has no way to check it.
for (const c of allCases) {
  const claims = ["why", "verify", "after"].filter((f) => c[f] !== undefined)
  if (claims.length > 0 && !c.source) {
    problems.push(`${c.id}: has ${claims.join(", ")} but no source`)
  }
}

if (problems.length) {
  console.error(`check-links: ${problems.length} problem(s)\n`)
  for (const p of problems) console.error(`  ${p}`)
  process.exit(1)
}

const withDetail = allCases.filter((c) => c.why || c.verify || c.after).length
console.log(
  `check-links: ${allCases.length} cases, ${withDetail} carrying detail, 0 problems.`
)
