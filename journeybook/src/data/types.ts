/**
 * The shape of the journeybook.
 *
 * Every field here is transcribed from docs/guides/factory-user-guide.md, which was
 * itself checked against `factory --help`, `factoryd --help` and the source.
 * Commands and expected output are copied, never paraphrased and never
 * invented. See .claude/authoring-steps.md before adding anything.
 */

/** Where in the factory pipeline a step happens. Drives the pipeline diagram. */
export type Phase =
  /** Install, PATH, register a project. Happens once. */
  | "setup"
  /** Interview, spec and plans, before any job exists. */
  | "spec"
  /** `factory new` and `factory run`: the builder at work. */
  | "build"
  /** Checks and the reviewer. */
  | "review"
  /** The factory decides: merge, fix round, or stop for you. */
  | "decide"
  /** The merge itself, and the clean-up after it. */
  | "merge"
  /** A job is in needs-human and you choose what happens next. */
  | "stuck"
  /** Reading status, logs, tokens and costs. Changes nothing. */
  | "observe"
  /** factoryd, the dashboard, upgrades and project upkeep. */
  | "operate"

/** Which door the reader walks through. */
export type Surface = "terminal" | "claude" | "dashboard" | "editor" | "files"

/**
 * How one step relates to another. Three values rather than a free string,
 * because a reader scanning a cross-reference needs to know which kind it is:
 * `depends` means this step is meaningless unless that one worked first,
 * `diverges` means the same words mean something different there, and
 * `contrasts` is the neighbouring step showing the opposite outcome.
 */
export type Relation = "depends" | "diverges" | "contrasts"

export interface Related {
  /** Must resolve to a real step id. scripts/check-links.mjs enforces it. */
  id: string
  rel: Relation
}

/**
 * How to settle worked from didn't-work when `expected` leaves a margin.
 *
 * One object, not an array. Rule 3 of .claude/authoring-steps.md is one
 * assertion per step, so a step needing two independent checks is two steps.
 */
export interface Verify {
  /**
   * Transcribed from the guide, or built only from flags that appear in
   * `--help`. Never inferred.
   */
  command?: string
  /** What in the result settles it. Required whenever `verify` is present. */
  look: string
}

export interface Case {
  /** Stable across edits -- progress is keyed on it in localStorage. */
  id: string
  title: string
  surface: Surface
  phase: Phase
  /** State the machine must be in. Omit when the journey preamble covers it. */
  precondition?: string
  /** Verbatim from the guide. Multi-line shell is fine. */
  command: string
  /** Verbatim expected output, including the exact message. */
  expected: string
  /**
   * The short assertion shown on the collapsed row -- what a reader scans for
   * when deciding whether their run matched. Keep it to a few words.
   */
  assert: string
  /** Why the step exists, or what is easy to get wrong. Light markdown. */
  notes?: string
  /** Marks a documented trap, gap or divergence rather than a happy path. */
  flag?: "divergence" | "gap" | "trap"
  /**
   * The mechanism. Why the factory behaves this way, not what the command
   * does. Requires `source`. An absent `why` is honest; an invented one is
   * the failure mode this whole structure exists to avoid.
   */
  why?: string
  /** How to settle worked from didn't-work. Requires `source`. */
  verify?: Verify
  /** What this leaves behind, and what to undo first. Requires `source`. */
  after?: string
  /** Other steps this one leans on or contradicts. */
  related?: Related[]
  /**
   * Provenance for `why`, `verify` and `after`. A guide section
   * (`factory-user-guide.md § 6.5 factory approve`) or a code location
   * (`crates/factory-cli/src/commands.rs:566`). Several citations are
   * separated by `; `.
   */
  source?: string
}

export interface Suite {
  /** The journey letter, A-H. */
  key: string
  title: string
  /** Who does the work in this journey: you, Claude, or factoryd. */
  actor: string
  /** One or two sentences: what this journey is actually about. */
  premise: string
  /**
   * The journey-level prose the guide carries between its command blocks, one
   * string per paragraph. `premise` stays the one-sentence summary shown on
   * the collapsed journey; this is what a reader reads before starting.
   */
  context?: string[]
  cases: Case[]
}
