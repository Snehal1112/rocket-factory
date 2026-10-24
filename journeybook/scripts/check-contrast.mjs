/**
 * Contrast gate for both themes.
 *
 *   bun scripts/check-contrast.mjs        # or: node scripts/check-contrast.mjs
 *
 * Parses the token blocks straight out of src/index.css, converts oklch to
 * sRGB, and checks every foreground/background pairing the components
 * actually use: 4.5:1 for body text, 3:1 for control boundaries, focus
 * indicators and the tick states that carry meaning.
 *
 * Run it after touching any colour token. The light scale was rebuilt against
 * these numbers -- eight pairings were failing before, including the Pass and
 * Fail labels on all 208 rows -- so a regression here is a real one.
 */
import fs from "node:fs"
import { oklch2lin, contrast, hex, over } from "./oklch.mjs"

const css = fs.readFileSync(
  process.argv[2] ?? new URL("../src/index.css", import.meta.url),
  "utf8"
)
function block(sel) {
  const i = css.indexOf(sel + " {")
  const body = css.slice(i, css.indexOf("\n}", i))
  const t = {}
  for (const m of body.matchAll(/--([\w-]+):\s*oklch\(([\d.]+) ([\d.]+) ([\d.]+)(?: \/ ([\d.]+)%)?\)/g))
    t[m[1]] = { lin: oklch2lin(+m[2], +m[3], +m[4]), alpha: m[5] ? +m[5] / 100 : 1 }
  return t
}
const themes = { LIGHT: block(":root"), DARK: block(".dark") }

for (const [name, t] of Object.entries(themes)) {
  const g = (k) => t[k].lin
  // In dark, --border/--input are white-alpha over the card; composite them.
  const solid = (k, on) => (t[k].alpha < 1 ? over(t[k].lin, g(on), t[k].alpha) : t[k].lin)
  let fail = 0
  const row = (l, f, b, n) => {
    const r = contrast(f, b); const ok = r >= n; if (!ok) fail++
    console.log(`  ${ok ? "ok  " : "FAIL"} ${r.toFixed(2).padStart(5)}:1 (${n})  ${l}`)
  }
  console.log(`\n########## ${name} ##########`)
  console.log("-- body text (4.5) --")
  row("foreground / background", g("foreground"), g("background"), 4.5)
  row("foreground / card", g("foreground"), g("card"), 4.5)
  row("muted-foreground / background", g("muted-foreground"), g("background"), 4.5)
  row("muted-foreground / card", g("muted-foreground"), g("card"), 4.5)
  row("muted-foreground / muted  (Kbd)", g("muted-foreground"), g("muted"), 4.5)
  row("muted-foreground / accent", g("muted-foreground"), g("accent"), 4.5)
  row("foreground / muted  (code chip)", g("foreground"), g("muted"), 4.5)
  row("primary / card", g("primary"), g("card"), 4.5)
  row("success / card  (figure)", g("success"), g("card"), 4.5)
  row("destructive / card", g("destructive"), g("card"), 4.5)
  row("primary-foreground / primary", g("primary-foreground"), g("primary"), 4.5)
  row("term-foreground / term", g("term-foreground"), g("term"), 4.5)
  row("term-muted / term", g("term-muted"), g("term"), 4.5)

  console.log("-- verdict pills --")
  const active = name === "LIGHT" ? 0.12 : 0.08
  const hov = name === "LIGHT" ? 0.20 : 0.08
  const bord = name === "LIGHT" ? 1.0 : 0.60
  for (const tok of ["success", "destructive"]) {
    row(`${tok} label on active /${active * 100}`, g(tok), over(g(tok), g("card"), active), 4.5)
    row(`${tok} label on active-hover /${hov * 100}`, g(tok), over(g(tok), g("card"), hov), 4.5)
    row(`${tok} label on idle-hover /8`, g(tok), over(g(tok), g("card"), 0.08), 4.5)
    row(`${tok} active border /${bord * 100} vs card`, over(g(tok), g("card"), bord), g("card"), 3.0)
  }

  console.log("-- non-text / state (3.0) --")
  row("ring / card  (focus)", g("ring"), g("card"), 3.0)
  row("ring / background", g("ring"), g("background"), 3.0)
  row("input / card  (control edge)", solid("input", "card"), g("card"), 3.0)
  row("input / background", solid("input", "background"), g("background"), 3.0)
  row("success vs untested  (pass vs todo tick)", g("success"), g("untested"), 3.0)
  row("destructive vs untested (fail vs todo)", g("destructive"), g("untested"), 3.0)

  console.log("-- surface hierarchy (informational) --")
  for (const [a, b, l] of [["card", "background", "panel vs ground"], ["border", "card", "panel edge"],
    ["untested", "card", "untested tick on panel"], ["accent", "card", "focused row"],
    ["term", "card", "transcript well vs panel"], ["term-border", "term", "transcript edge"]])
    console.log(`       ${contrast(solid(a, b), g(b)).toFixed(2).padStart(5)}:1  ${l}`)
  console.log(`\n  ==> ${fail} failing in ${name}`)
}
