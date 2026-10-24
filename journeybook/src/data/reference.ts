/**
 * The material a reader needs beside the steps: the setup to run once, the
 * pipeline to hold in mind, the words the factory uses, and the flags that do
 * less than they look. All transcribed from docs/guides/factory-user-guide.md.
 */

export const setupScript = `# 1. Build the programs from the rocket-factory repository root.
cargo build -j4 -p factory-cli -p factory-supervisor
cargo build -j4 --manifest-path dashboard/Cargo.toml

# 2. Put them on your PATH, then open a new terminal.
mkdir -p ~/.local/bin
ln -sf "$PWD/target/debug/factory"  ~/.local/bin/factory
ln -sf "$PWD/target/debug/factoryd" ~/.local/bin/factoryd
which factory factoryd

# 3. Install the /factory command, then register your project.
factory install
factory project add ~/code/my-app --preset rust
factory project show my-app     # must end with: problems: none`

export const globalFlags =
  "--project <name>  --root <path>  --help  (--version at the top level)"

export interface GlossaryRow {
  term: string
  meaning: string
}

export const glossary: GlossaryRow[] = [
  {
    term: "job",
    meaning: "One unit of work the factory builds and merges. It has an id like `028`. `1`, `01` and `001` all mean job `001`.",
  },
  {
    term: "spec",
    meaning: "The approved design document. It needs a `## Goal` of at least 8 words and a `## Acceptance criteria` list.",
  },
  {
    term: "plan",
    meaning: "A markdown file with 1 to 3 tasks. A feature can chain up to 6 plans with a `Next plan:` last line.",
  },
  {
    term: "series",
    meaning: "The chain of jobs made from a multi-plan feature. Plan 2 starts only after plan 1 merges. Series ids look like `S1`.",
  },
  {
    term: "worktree",
    meaning: "A second working copy of your repository on the job's own branch, so your own checkout stays clean while a job builds.",
  },
  {
    term: "stage",
    meaning: "Where a job is: `spec`, `build`, `review`, `approve`, `merged` or `needs-human`.",
  },
  {
    term: "needs-human",
    meaning: "The job stopped and wants you. The reason is printed with the commands you can run next. Also called parked.",
  },
  {
    term: "risky diff",
    meaning: "A change that touches a sensitive path such as `Cargo.toml`, authentication or secrets. It **always** needs your approval.",
  },
  {
    term: "preset",
    meaning: "A starter rule set for a language: `rust`, `go`, `ruby`, `csharp` or `node`.",
  },
  {
    term: "project",
    meaning: "A repository, or a folder inside one, registered with the factory.",
  },
  {
    term: "factory home",
    meaning: "Where the factory keeps its data: `$FACTORY_HOME`, else `~/.factory`.",
  },
  {
    term: "lease",
    meaning: "A lock saying which process owns a project or job, so two drivers never work on the same project at once.",
  },
  {
    term: "weighted tokens",
    meaning: "The factory's cost unit. Cache reads count one tenth. Each job has a budget, 1,000,000 by default.",
  },
  {
    term: "draft",
    meaning: "A feature that is still being specified, before it has a job.",
  },
]

export interface PipelineStage {
  key: string
  n: number
  name: string
  scope: string
  /** The command or line that shows this stage, set in the terminal face. */
  signature: string
  detail: string
}

export const pipeline: PipelineStage[] = [
  {
    key: "spec",
    n: 1,
    name: "Spec and plans",
    scope: "You and Claude agree what to build, and it is written down.",
    signature: `factory spec start login-fix "Fix the login redirect"`,
    detail:
      "The spec needs a **Goal** and **Acceptance criteria**, and no `TBD`, `TODO`, `FIXME` or `???` outside code blocks. Commit the spec and the plans.",
  },
  {
    key: "new",
    n: 2,
    name: "Create the job",
    scope: "The factory checks the spec and plans before anything exists.",
    signature: `factory new login-fix "Fix the login redirect" --spec <spec> --plans <folder>`,
    detail:
      "One job per plan, or one job for the whole chain with `--one-job`. A bad plan is refused here, before any job is created.",
  },
  {
    key: "build",
    n: 3,
    name: "Build",
    scope: "A builder agent works one task at a time in its own worktree.",
    signature: "factory run 028 --series",
    detail:
      "The builder never commits. Your checkout is not touched, and nothing is pushed.",
  },
  {
    key: "review",
    n: 4,
    name: "Checks and review",
    scope: "The checks for the changed files run, then a reviewer reads the change.",
    signature: "reviewer verdict: APPROVE or CHANGES",
    detail:
      "If the checks fail or the reviewer asks for changes, the builder gets up to **2 fix rounds**.",
  },
  {
    key: "decide",
    n: 5,
    name: "Decide",
    scope: "The factory merges only when every condition holds.",
    signature: "approved, small, no risky path, in budget",
    detail:
      "Approved by the reviewer, under the line cap, no sensitive path touched and inside the token budget. Anything else stops for you.",
  },
  {
    key: "end",
    n: 6,
    name: "Merged, or waiting for you",
    scope: "The last line tells you which.",
    signature: "merged | needs-human: <reason>",
    detail:
      "A merge goes into your **local** base branch only. For a stopped job, run `factory status`, then `approve`, `rework` or `cancel`.",
  },
]

export interface MessageRow {
  message: string
  action: string
}

export const messageTable: MessageRow[] = [
  {
    message: "risky diff: risky paths (K): ...",
    action: "A sensitive file changed. Read the diff, then `approve`, or `rework` with a note.",
  },
  {
    message: "checks failed after N fix rounds",
    action: "The tests still fail. `rework` with a hint.",
  },
  {
    message: "review asked for changes after N fix rounds",
    action: "`approve` if you disagree with the reviewer, or `rework`.",
  },
  {
    message: "diff has N changed lines, the cap is C",
    action: "Too big to auto-merge. `approve` after reading it, or `rework` to split it up.",
  },
  {
    message: "budget is stopped",
    action: "The job used its token budget. `approve` or `cancel`. Raise `budget_weighted` if the job is legitimately large.",
  },
  {
    message: "builder blocked/needs context ...",
    action: "The builder needs information. `rework` and give it in the note.",
  },
  {
    message: "builder made no changes",
    action: "Nothing was produced. `rework`, or `cancel`.",
  },
  {
    message: "no check ran for N changed code path(s)",
    action: "No check covers the changed files. `approve` if you accept that, or `rework`.",
  },
  {
    message: "main busy: the main checkout has tracked changes",
    action: "Your own checkout has uncommitted edits to tracked files. Commit or stash them, then `approve` again. Untracked files are fine.",
  },
  {
    message: "main busy: the main checkout is not on <base>",
    action: "Switch your checkout to the base branch, then `approve` again.",
  },
  {
    message: "merge conflict in <paths>",
    action: "The base branch moved and the change conflicts. Look at the job's worktree, or `rework` with a note.",
  },
  {
    message: "no plan.md in the job folder",
    action: "Put `plan.md` in the job folder, then `run` again.",
  },
  {
    message: "project is busy: job N is running ...",
    action: "Another run holds the project (exit code 75). Wait. If that process is gone, `factory cancel N --force`.",
  },
  {
    message: "this home was last used by factory X, this binary is Y",
    action: "Install a newer binary, or delete the `VERSION` file in the factory home to ignore the check.",
  },
]

export interface ProgramRow {
  want: string
  factory: string
  factoryd: string
}

export const programMatrix: ProgramRow[] = [
  {
    want: "Build one feature now",
    factory: "factory run <id>",
    factoryd: "factoryd enqueue --project my-app --job <id>",
  },
  {
    want: "Keep a run going after you close the terminal",
    factory: "none (use nohup or tmux)",
    factoryd: "factoryd enqueue ...",
  },
  {
    want: "See what is happening",
    factory: "factory status",
    factoryd: "factoryd status",
  },
  {
    want: "Approve a stopped job",
    factory: "factory approve <id>",
    factoryd: "factoryd enqueue ... --approve",
  },
  {
    want: "Rework or cancel a job",
    factory: "factory rework / factory cancel",
    factoryd: "none on the command line (the --home dashboard can rework and cancel)",
  },
  {
    want: "Run several projects at once",
    factory: "none",
    factoryd: "factoryd run",
  },
  {
    want: "Set a daily token limit",
    factory: "none",
    factoryd: "daily_budget_weighted in ~/.factory/config.toml",
  },
  {
    want: "Check that it is running",
    factory: "factory status (first line)",
    factoryd: "factoryd is-running",
  },
  {
    want: "Stop it",
    factory: "factory cancel <id>",
    factoryd: "factoryd stop (running jobs keep running)",
  },
]

export interface CommandGroup {
  group: string
  subcommands: string
}

export const commandGroups: CommandGroup[] = [
  { group: "factory new", subcommands: "<slug> <feature> --plans --spec --model --one-job --accept-over-budget" },
  { group: "factory run", subcommands: "<id> --series" },
  { group: "factory status | show", subcommands: "status  |  show <id>" },
  { group: "factory approve", subcommands: "<id> --without-review" },
  { group: "factory rework", subcommands: "<id> <note>" },
  { group: "factory cancel", subcommands: "<id|draft> --reason --force" },
  { group: "factory cleanup", subcommands: "<id> --discard-commits" },
  { group: "factory spec", subcommands: "start | stage | resume" },
  { group: "factory tokens | costs", subcommands: "tokens <id> | --cache --days  |  costs" },
  { group: "factory log", subcommands: "--job --level --event --tail" },
  { group: "factory tickets", subcommands: "<id> --dry-run" },
  { group: "factory kb-refresh", subcommands: "--job <id>" },
  { group: "factory project", subcommands: "add | list | which | show | remove" },
  { group: "factory presets", subcommands: "refresh --force" },
  { group: "factory install | upgrade", subcommands: "install --dir --force  |  upgrade --dry-run" },
  { group: "factoryd", subcommands: "run | status | stop | is-running | enqueue" },
]

export interface FlagTrap {
  command: string
  gotcha: string
}

export const flagTraps: FlagTrap[] = [
  {
    command: "factory new --model <name>",
    gotcha:
      "Stored on the job card. Nothing makes the builder or reviewer use it: their models come from `builder_model` and `reviewer_model` in `project.toml`.",
  },
  {
    command: "factory cleanup --discard-commits",
    gotcha:
      "Deletes real work for good. Use it only on a job you are throwing away.",
  },
  {
    command: "factory project remove --force",
    gotcha:
      "Overrides open jobs and a broken project file. It does **not** override a held project lease.",
  },
  {
    command: "factory presets refresh --force",
    gotcha: "Overwrites files you edited by hand.",
  },
  {
    command: "factory cancel <id> --force",
    gotcha:
      "Only for a job whose owning process is gone. It also clears the project lease held for that job.",
  },
  {
    command: "factory log --job 28",
    gotcha:
      "Use the full `028`, not `28`.",
  },
  {
    command: "factory --root <path> project ...",
    gotcha:
      "`project`, `presets`, `install` and `upgrade` use only the factory home, so `--root` changes nothing for them.",
  },
  {
    command: "factoryd enqueue --approve --series",
    gotcha:
      "Refused: `series is only valid for a run request`. There is also no `--without-review` flag on `enqueue`.",
  },
  {
    command: "factoryd stop",
    gotcha:
      "Ends the supervisor after its current pass. Running jobs keep running, and the next `factoryd` adopts them.",
  },
  {
    command: "factory-server --root <repo>",
    gotcha:
      "Reads the old in-repo `.superpowers/` folder, so a registered project looks empty. Use `--home ~/.factory`.",
  },
]

export interface Correction {
  n: number
  claimed: string
  actual: string
}

export const corrections: Correction[] = [
  {
    n: 1,
    claimed:
      "Build with `cargo build -j4 --manifest-path tools/factory/Cargo.toml -p factory-cli`. The binary is `tools/factory/target/debug/factory`.",
    actual:
      "`cargo build -j4 -p factory-cli`. The binary is `target/debug/factory`.",
  },
  {
    n: 2,
    claimed:
      "Run the dashboard with `--manifest-path tools/factory-server/Cargo.toml`.",
    actual: "The dashboard lives in `dashboard/`: `--manifest-path dashboard/Cargo.toml`.",
  },
  {
    n: 3,
    claimed: "The board is in the old in-repository `.superpowers/` folder.",
    actual:
      "For a registered project it is at `~/.factory/projects/<name>/state/board.json`.",
  },
  {
    n: 4,
    claimed: "`factory install` writes only `~/.claude/commands/factory.md`.",
    actual:
      "It also installs a Codex skill at `~/.agents/skills/factory/SKILL.md`, unless you pass `--dir`.",
  },
  {
    n: 5,
    claimed: "`factory cleanup` removes the worktree and branch after a merge or a cancel.",
    actual:
      "It removes the branch only if its commits are in the base branch, unless you pass `--discard-commits`. A cancelled job's card can stay on the board.",
  },
  {
    n: 6,
    claimed: "Only the builder, reviewer and committer spend tokens.",
    actual: "The message-only committer run and the knowledge-base refresher spend tokens too.",
  },
  {
    n: 7,
    claimed: "`factory-server --root <repo>` shows a registered project.",
    actual: "`--root` reads the old layout. Use `--home ~/.factory` with `factoryd` running.",
  },
  {
    n: 8,
    claimed: "`factory new --model` chooses the model that builds the job.",
    actual: "The value is stored on the job and has no effect on which model runs.",
  },
  {
    n: 9,
    claimed: "`factory log` takes `--job`, `--level` and `--tail`.",
    actual: "It also takes `--event`, which matches a name or a dotted prefix such as `task`.",
  },
]
