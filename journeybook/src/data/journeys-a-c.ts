import type { Suite } from "./types"

export const journeysAC: Suite[] = [
  {
    key: "A",
    title: "Get set up",
    actor: "You",
    premise:
      "You build the factory programs, put them on your PATH, install the /factory command and register one project. You do this once per machine and once per project.",
    context: [
      "The factory is two programs. `factory` is the command you use for everything and works on one project at a time. `factoryd` is an optional background supervisor that you only need for several projects at once, queued runs or a daily token limit.",
      "Setup has two halves. The first half (A1 to A5 and A10) is per machine: build, link the programs onto your PATH, and install the Claude command. The second half (A6 to A9) registers your repository as a project. Registering writes only into the factory home (`$FACTORY_HOME`, else `~/.factory`); it does not change your repository.",
      "You need Rust 1.82 or newer, a C compiler, git, a logged-in Claude Code, and the superpowers (7.0.0) and dev-workflow-skills plugins. Run the build steps from the rocket-factory repository root. The guide uses `my-app` as the project name throughout.",
    ],
    cases: [
      {
        id: "A1",
        title: "Build the programs",
        surface: "terminal",
        phase: "setup",
        precondition:
          "You are in the rocket-factory repository root, and Rust, a C compiler and git are installed.",
        command: `cargo build -j4 -p factory-cli -p factory-supervisor
cargo build -j4 --manifest-path dashboard/Cargo.toml`,
        expected: `(no build output is given in the guide; the programs are written to target/debug/factory, target/debug/factoryd and dashboard/target/debug/factory-server)`,
        assert: "The three program files exist at these paths",
        notes:
          "Always use `-j4`. Do not use `--workspace` or `--all`. The guide lists the three paths above as the result of the two builds and does not print the build output itself.",
        source: "factory-user-guide.md § 4.1 Build the programs",
      },
      {
        id: "A2",
        title: "Put factory and factoryd on your PATH",
        surface: "terminal",
        phase: "setup",
        precondition: "A1 finished.",
        command: `mkdir -p ~/.local/bin
ln -sf "$PWD/target/debug/factory"  ~/.local/bin/factory
ln -sf "$PWD/target/debug/factoryd" ~/.local/bin/factoryd
which factory factoryd`,
        expected: `which prints a path for factory and a path for factoryd`,
        assert: "which finds both programs",
        related: [{ id: "A1", rel: "depends" }],
        why: "`factoryd` finds the `factory` program next to itself, or from the `FACTORY_BIN` variable, so both must be linked.",
        source:
          "factory-user-guide.md § 4.2 Put them on your PATH; factory-user-guide.md § 7 Command reference: factoryd",
      },
      {
        id: "A3",
        title: "If which found nothing, add ~/.local/bin to PATH",
        surface: "terminal",
        phase: "setup",
        precondition:
          "A2 printed nothing from `which`. Skip this step if A2 found both programs. Add this line to ~/.zshrc or ~/.bashrc, then open a new terminal.",
        command: `export PATH="$HOME/.local/bin:$PATH"`,
        expected: `After a new terminal opens, which factory factoryd finds both programs`,
        assert: "A new terminal finds factory and factoryd",
        notes:
          "Put the line in `~/.zshrc` or `~/.bashrc`, then open a new terminal. The guide gives no output for this line.",
        flag: "trap",
        related: [{ id: "A2", rel: "depends" }],
        after:
          "Leaves one line in `~/.zshrc` or `~/.bashrc`. Delete the line to undo it.",
        source:
          "factory-user-guide.md § 4.2 Put them on your PATH; factory-user-guide.md § 13 Troubleshooting",
      },
      {
        id: "A4",
        title: "Install the /factory command (fallback)",
        surface: "terminal",
        phase: "setup",
        precondition: "A2 or A3 done, so `factory` runs from any folder.",
        command: `factory install`,
        expected: `installed <path>
note: the Claude Code plugin is the preferred install:
  claude plugin marketplace add Snehal1112/rocket-factory
  claude plugin install factory@rocket-factory
installed <path>`,
        assert: "One installed line per target, with the plugin note after the Claude line",
        notes:
          "The two targets are `~/.claude/commands/factory.md` (the Claude command) and `~/.agents/skills/factory/SKILL.md` (a Codex skill). The plugin is the preferred install for Claude Code (A10) and `factory install` is the fallback. With the plugin `factory@rocket-factory` installed, the Claude line instead reads `skipped Claude command: the factory plugin is installed`, and there is no plugin note.",
        why: "`factory install` writes the Claude command and the Codex skill. Without the plugin it follows the Claude line with a note that names the plugin as the preferred install.",
        after:
          "Leaves two files under your home folder. `factory install --force` replaces a copy that differs.",
        related: [{ id: "A10", rel: "contrasts" }],
        source: "factory-user-guide.md § 4.3 Install the /factory command; factory-user-guide.md § 6.17 factory install",
      },
      {
        id: "A5",
        title: "Running install again is safe",
        surface: "terminal",
        phase: "setup",
        precondition: "A4 done.",
        command: `factory install`,
        expected: `already installed: <path>
already installed: <path>`,
        assert: "Each target says already installed",
        flag: "divergence",
        notes:
          "Section 4.3 of the guide writes this message as `already installed`. Section 6.17 writes it as `already installed: <path>`. This step uses the longer form from 6.17, one line per target. With the plugin installed, the Claude line reads `skipped Claude command: the factory plugin is installed` instead.",
        related: [{ id: "A4", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.17 factory install",
      },
      {
        id: "A10",
        title: "Install the /factory command as a plugin (preferred)",
        surface: "terminal",
        phase: "setup",
        precondition: "Claude Code is installed and logged in.",
        command: `claude plugin marketplace add Snehal1112/rocket-factory
claude plugin install factory@rocket-factory`,
        expected: `The guide shows no output for these commands.`,
        assert: "Both commands finish without an error",
        notes:
          "This is the preferred install for Claude Code. After it, `factory install` skips the Claude command and still installs the Codex skill. If `factory.md` also exists, `factory install` adds `note: the factory plugin is installed and <path> also exists, so /factory is listed twice; remove it with: rm <path>`.",
        verify: {
          command: `factory install`,
          look: "The Claude line reads `skipped Claude command: the factory plugin is installed`.",
        },
        after:
          "Leaves the plugin `factory@rocket-factory` installed in Claude Code. `factory upgrade` then leaves `factory.md` alone and prints the plugin update command (G13).",
        related: [
          { id: "A4", rel: "contrasts" },
          { id: "G13", rel: "contrasts" },
        ],
        source: "factory-user-guide.md § 4.3 Install the /factory command; factory-user-guide.md § 6.17 factory install; factory-user-guide.md § 6.18 factory upgrade",
      },
      {
        id: "A6",
        title: "Register your project",
        surface: "terminal",
        phase: "setup",
        precondition:
          "~/code/my-app is a git repository and the branch it is on exists locally.",
        command: `factory project add ~/code/my-app --preset rust`,
        expected: `added project my-app (rust, base <branch>) at <folder>`,
        assert: "The added project line names my-app",
        why: "The command writes `~/.factory/projects/<name>/project.toml` and empty `agents/`, `state/` and `worktrees/` folders inside the factory home. It seeds the presets if they are missing.",
        after:
          "Leaves a project folder under `~/.factory/projects/my-app/`. Your repository is not changed. `factory project remove my-app` deletes the project folder. The seeded presets stay.",
        source: "factory-user-guide.md § 6.15 factory project add",
      },
      {
        id: "A7",
        title: "Check the project is healthy",
        surface: "terminal",
        phase: "setup",
        precondition: "A6 done.",
        command: `factory project show my-app`,
        expected: `<key>: <value>
...
problems: none`,
        assert: "The last line is problems: none",
        notes:
          "The guide says the output is one `key: value` per line and gives only the last line. If the output lists problems instead, fix them before you continue.",
        verify: {
          command: `factory project show my-app`,
          look: "The final line reads `problems: none`.",
        },
        related: [{ id: "A6", rel: "depends" }],
        source: "factory-user-guide.md § 4.4 Register your project; factory-user-guide.md § 6.15 factory project show",
      },
      {
        id: "A8",
        title: "Ask which project a folder belongs to",
        surface: "terminal",
        phase: "setup",
        precondition: "A6 done.",
        command: `factory project which ~/code/my-app/src`,
        expected: `my-app`,
        assert: "The project name is printed",
        notes:
          "With no path it uses your current folder. A folder that is in no project prints `no registered project contains <path>`.",
        related: [{ id: "A6", rel: "depends" }],
        source: "factory-user-guide.md § 6.15 factory project which",
      },
      {
        id: "A9",
        title: "List every registered project",
        surface: "terminal",
        phase: "setup",
        precondition: "A6 done.",
        command: `factory project list`,
        expected: `my-app rust <base-branch> <folder>`,
        assert: "One line for my-app with preset and base branch",
        notes:
          "A project that cannot be used shows `<name>  disabled: <why>`. With no projects it prints `no projects`.",
        related: [{ id: "A6", rel: "depends" }],
        source: "factory-user-guide.md § 6.15 factory project list",
      },
    ],
  },
  {
    key: "B",
    title: "Your first feature",
    actor: "You, and Claude for the spec",
    premise:
      "You take one small feature from an empty board to a merged job: write the spec and plans, create the job, run it, and push the result yourself.",
    context: [
      "The whole path is: check the board, write the spec and plans, create the job with `factory new`, run it with `factory run`, clean up, and push. The easy way to write the spec and plans is the `/factory` command in Claude Code, which interviews you. You can also write them by hand.",
      "While you are still specifying, you can keep a draft card on the board with `factory spec start`, `spec stage` and `spec resume`. These three commands only keep records. They run no agent and write no spec file.",
      "The factory merges only into your local base branch and never pushes. Your own checkout is not touched while a job builds, because the work happens in a separate worktree. This journey uses `my-app`, `login-fix` and job `028`.",
    ],
    cases: [
      {
        id: "B1",
        title: "Check the board is empty",
        surface: "terminal",
        phase: "observe",
        precondition: "Journey A is finished.",
        command: `factory --project my-app status`,
        expected: `project: free
no jobs`,
        assert: "project: free and no jobs",
        why: "`status` prints the lease line first, then a table or the text `no jobs`. It never changes anything.",
        source: "factory-user-guide.md § 5 Step 1; factory-user-guide.md § 6.3 factory status",
      },
      {
        id: "B2",
        title: "Start a draft card for the feature",
        surface: "terminal",
        phase: "spec",
        precondition: "B1 done.",
        command: `factory --project my-app spec start login-fix "Fix the login redirect"`,
        expected: `(the draft name, the stage interviewing, and the spec file path marked exists or missing)`,
        assert: "The draft is at stage interviewing",
        notes:
          "The guide says this prints the draft name, the stage, and the spec file path marked `exists` or `missing`. It does not show the exact layout, so the lines above give the three items in that order.",
        why: "A draft is a card on the board for a feature you are still specifying. `spec start` adds it at stage `interviewing`.",
        after:
          "Leaves a draft card on the board. `factory new` with the same slug removes it, and `factory cancel login-fix` removes it and keeps the spec file.",
        source: "factory-user-guide.md § 6.9 factory spec; factory-user-guide.md § 6.7 factory cancel",
      },
      {
        id: "B3",
        title: "Move the draft to the spec-writing stage",
        surface: "terminal",
        phase: "spec",
        precondition: "B2 done.",
        command: `factory --project my-app spec stage login-fix writing-spec`,
        expected: `The guide shows no output for this command. Run B4 to see the new stage.`,
        assert: "No error, stage becomes writing-spec",
        notes:
          "The stage is `interviewing`, `writing-spec` or `writing-plans`. Any direction is allowed.",
        related: [{ id: "B2", rel: "depends" }],
        source: "factory-user-guide.md § 6.9 factory spec",
      },
      {
        id: "B4",
        title: "Read the draft back",
        surface: "terminal",
        phase: "spec",
        precondition: "B3 done.",
        command: `factory --project my-app spec resume login-fix`,
        expected: `(the stage writing-spec and the spec file path)`,
        assert: "The stage reads writing-spec",
        notes:
          "The guide says `spec resume` prints the stage and the spec file of a draft. An unknown name fails with `no draft named X`.",
        related: [{ id: "B3", rel: "depends" }],
        source: "factory-user-guide.md § 6.9 factory spec",
      },
      {
        id: "B5",
        title: "Write the spec and the plans",
        surface: "claude",
        phase: "spec",
        precondition: "You are in Claude Code, inside the my-app repository.",
        command: `/factory`,
        expected: `(files laid out as in the guide's expected layout, § 5 Step 2; /factory is not promised to produce these exact names)
docs/superpowers/specs/2026-10-25-login-fix-design.md
docs/superpowers/plans/2026-10-25-login-fix/plan-01-login-fix.md
docs/superpowers/plans/2026-10-25-login-fix/plan-02-login-fix.md`,
        assert: "The spec and plan files exist at these paths",
        notes:
          "Commit the spec and the plans before the next step. You can write these files by hand instead of using `/factory`.",
        why: "The `/factory` command runs the interview and planning, then drives `factory` for you.",
        source: "factory-user-guide.md § 1 What the factory is; factory-user-guide.md § 5 Step 2",
      },
      {
        id: "B6",
        title: "Create the job",
        surface: "terminal",
        phase: "build",
        precondition: "B5 done and the spec and plans are committed.",
        command: `factory --project my-app new login-fix "Fix the login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/`,
        expected: `created job 028 in <job folder>`,
        assert: "The first line is created job 028",
        notes:
          "Write the job id down. You need it for every later command. The first line is followed by a token estimate per job.",
        why: "With several plans, plan 1 becomes a job at stage `build` and every later plan gets its own job, waiting at stage `spec`. A copy of the spec and of each plan is stored in each job folder.",
        after:
          "Leaves one or more jobs on the board and nothing is built yet. Edit the copy of the spec or plan in the job folder, not the committed file.",
        related: [{ id: "B5", rel: "depends" }],
        source: "factory-user-guide.md § 5 Step 3; factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "B7",
        title: "Run the job and its whole series",
        surface: "terminal",
        phase: "build",
        precondition: "B6 printed `created job 028`.",
        command: `factory --project my-app run 028 --series`,
        expected: `merged`,
        assert: "The last line is merged or needs-human",
        notes:
          "The last line is either `merged` or `needs-human: <reason>`. This can take a long time and use many tokens, and it never asks questions. A terminal run ends if you close the terminal.",
        why: "`--series` continues with the next plan after each merge and stops at the first `needs-human`. The stage on the board is the only state, so you can run again after a stop.",
        verify: {
          command: `factory --project my-app status`,
          look: "The STAGE column reads `merged` for the job.",
        },
        related: [
          { id: "B6", rel: "depends" },
          { id: "B8", rel: "contrasts" },
        ],
        source: "factory-user-guide.md § 5 Step 4; factory-user-guide.md § 6.2 factory run; factory-user-guide.md § 6.3 factory status",
      },
      {
        id: "B8",
        title: "If it stopped, read why on the board",
        surface: "terminal",
        phase: "stuck",
        precondition: "B7 ended with `needs-human: <reason>`.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: <reason>`,
        assert: "A needs a human block gives the reason and next commands",
        notes:
          "Under that line you also see a short piece of evidence and the `next:` commands you can run. The choices are `approve 028`, `rework 028 \"<note>\"` followed by `run 028`, or `cancel 028`.",
        related: [{ id: "B7", rel: "depends" }],
        source: "factory-user-guide.md § 5 Step 5; factory-user-guide.md § 6.3 factory status",
      },
      {
        id: "B9",
        title: "Clean up after the merge",
        surface: "terminal",
        phase: "merge",
        precondition: "Job 028 is merged.",
        command: `factory --project my-app cleanup 028`,
        expected: `removed worktree and branch of job 028`,
        assert: "The worktree and branch of job 028 are removed",
        why: "`cleanup` works only on a merged or cancelled job. It removes the git worktree and the job branch. The job folder with the ledger, logs and usage figures always stays, and a merged job keeps its card on the board.",
        after:
          "The job folder stays. Never add `--discard-commits` to a job you want to keep.",
        related: [{ id: "B7", rel: "depends" }],
        source: "factory-user-guide.md § 6.8 factory cleanup",
      },
      {
        id: "B10",
        title: "Push the merge yourself",
        surface: "terminal",
        phase: "merge",
        precondition: "Job 028 is merged into your local base branch.",
        command: `git push`,
        expected: `The guide shows no output for this command. The factory merged into your local branch only, and this step sends it to the remote.`,
        assert: "You ran git push, not the factory",
        flag: "trap",
        notes:
          "Until you run this, the merge exists only on your machine.",
        why: "The factory merges into your local base branch and never pushes.",
        related: [{ id: "B7", rel: "depends" }],
        source: "factory-user-guide.md § 1 What the factory is; factory-user-guide.md § 5 Step 7",
      },
    ],
  },
  {
    key: "C",
    title: "Bigger features",
    actor: "You",
    premise:
      "A feature too big for one job is split into a chain of plans that run in order, and `factory new` checks the spec and every plan before it accepts them.",
    context: [
      "A plan is a markdown file with 1 to 3 tasks, and a feature can have up to 6 plans in a chain. Plan 1 starts as a job at stage `build`. Every later plan gets its own job at stage `spec` and starts only after the plan before it is merged. The jobs share a series id such as `S1`.",
      "Before it creates anything, `factory new` checks the rules in this journey and refuses with a message that starts `factory:`. Each refusal below is a step, so you can recognise the message when you meet it and know what to fix.",
      "If you would rather have the whole chain in one job, `--one-job` does that. It lifts the 3-task limit per plan, but the job gets an estimate of 110,000 plus 100,000 per task in weighted tokens, and an estimate over `budget_weighted` needs your consent.",
    ],
    cases: [
      {
        id: "C1",
        title: "Name the plan files plan-NN in one folder",
        surface: "files",
        phase: "spec",
        precondition: "You are writing plans for login-fix.",
        command: `Create these files in the my-app repository.`,
        expected: `docs/superpowers/specs/2026-10-25-login-fix-design.md
docs/superpowers/plans/2026-10-25-login-fix/plan-01-login-fix.md
docs/superpowers/plans/2026-10-25-login-fix/plan-02-login-fix.md`,
        assert: "Plan files are named plan-NN and share one folder",
        why: "Only files named `plan-*.md` count in the plans folder, and they are sorted by name.",
        source: "factory-user-guide.md § 5 Step 2; factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C2",
        title: "End every plan with a Next plan line",
        surface: "editor",
        phase: "spec",
        precondition: "C1 done.",
        command: `Open each plan file and check its last line.`,
        expected: `Next plan: <path>
Next plan: none`,
        assert: "Last line is Next plan: a path, or none on the final plan",
        notes:
          "The path has no spaces. Plan 1 points at plan 2, and the last plan says `none`. A wrong last line is refused with `<file>: the last line must be 'Next plan: <path|none>'`.",
        why: "The factory follows the `Next plan:` lines to find the chain. A `plan-*.md` file that the chain does not reach is refused, and a loop in the chain is refused.",
        related: [{ id: "C1", rel: "depends" }],
        source: "factory-user-guide.md § 5 Step 2; factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C3",
        title: "A plan chain becomes a series of jobs",
        surface: "terminal",
        phase: "build",
        precondition:
          "You ran `factory new` with --spec and --plans for a folder of several plans (see B6).",
        command: `factory --project my-app status`,
        expected: `(plan 1 shows stage build, every later plan shows stage spec, and SERIES reads index/total, for example 1/2)`,
        assert: "Plan 1 is at build, later plans wait at spec",
        notes:
          "The guide describes the table columns and the stages but prints no exact rows, so the lines above describe what to look for.",
        why: "Later plans start in order when you run with `--series`, and a plan starts only after the plan before it is merged.",
        related: [{ id: "B6", rel: "depends" }],
        source: "factory-user-guide.md § 6.1 factory new; factory-user-guide.md § 6.3 factory status",
      },
      {
        id: "C4",
        title: "A plan with more than 3 tasks is refused",
        surface: "terminal",
        phase: "build",
        precondition:
          "plan-01 holds four task lines, written `- [ ] Task 1: ...` up to `- [ ] Task 4: ...`.",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/`,
        expected: `factory: a plan needs 1 to 3 tasks, found N`,
        assert: "Refused with a plan needs 1 to 3 tasks message",
        notes:
          "Split the plan or merge tasks, or use `--one-job`. Here N is 4.",
        why: "Each plan needs 1 to 3 tasks. With `--one-job` the upper limit is lifted, but each plan still needs at least 1.",
        related: [{ id: "C9", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C5",
        title: "More than 6 plans is refused",
        surface: "terminal",
        phase: "build",
        precondition: "The Next plan chain in the folder reaches 7 plans.",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/`,
        expected: `The guide says a chain of more than 6 plans is refused and does not print the message. No job is created.`,
        assert: "A 7-plan chain is refused",
        notes:
          "Cut the feature into at most 6 plans. The exact message text is not in the guide, so match on the refusal and the `factory:` prefix.",
        why: "A feature can have up to 6 plans in a chain.",
        source: "factory-user-guide.md § 2 The ideas you need; factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C6",
        title: "A spec without a Goal is refused",
        surface: "terminal",
        phase: "build",
        precondition:
          "The spec file has no `## Goal` section.",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/`,
        expected: `factory: <path> has no \`## Goal\` section`,
        assert: "Refused with a no Goal section message",
        notes:
          "Add a `## Goal` section of at least 8 words.",
        why: "The spec must have a `## Goal` of at least 8 words.",
        source: "factory-user-guide.md § 5 Step 2; factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C7",
        title: "A spec without acceptance bullets is refused",
        surface: "terminal",
        phase: "build",
        precondition:
          "The spec has a `## Acceptance criteria` section with no bullet in it.",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/`,
        expected: `factory: ... has no list item; list each check as a bullet`,
        assert: "Refused with a list each check as a bullet message",
        notes:
          "Write each acceptance check as a bullet under `## Acceptance criteria`.",
        why: "The spec must have a `## Acceptance criteria` section with at least one bullet.",
        source: "factory-user-guide.md § 5 Step 2; factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C8",
        title: "Leftover TBD text is refused",
        surface: "terminal",
        phase: "build",
        precondition:
          "A line of the spec or of a plan, outside a code block, contains the word `TBD`.",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/`,
        expected: `factory: <name> line N still has \`TBD\``,
        assert: "Refused, naming the file and line that has TBD",
        notes:
          "`TODO`, `FIXME` and `???` are refused the same way. Finish or remove that text. Code blocks are not checked.",
        why: "No `TBD`, `TODO`, `FIXME` or `???` is allowed outside code blocks, in the spec or in a plan.",
        source: "factory-user-guide.md § 5 Step 2; factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C9",
        title: "Combine the whole chain into one job with --one-job",
        surface: "terminal",
        phase: "build",
        precondition:
          "The spec and plans pass the checks, and the estimate is within budget_weighted.",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/ \\
  --one-job`,
        expected: `created job 028 in <job folder>`,
        assert: "One job is created for the whole chain",
        notes:
          "`--one-job` needs `--plans`. The guide gives the `created job` line for `factory new` in general and no separate output for this form.",
        why: "With `--one-job` the upper limit of 3 tasks per plan is lifted. The estimate is 110,000 plus 100,000 per task, in weighted tokens.",
        related: [
          { id: "C4", rel: "contrasts" },
          { id: "C10", rel: "contrasts" },
        ],
        source: "factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C10",
        title: "An over-budget --one-job asks for your consent",
        surface: "terminal",
        phase: "build",
        precondition:
          "The one-job estimate of 110,000 plus 100,000 per task is over budget_weighted.",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/ \\
  --one-job`,
        expected: `needs consent: this job is estimated at E weighted tokens and the budget is B; raise budget_weighted in project.toml or pass --accept-over-budget`,
        assert: "Stopped with a needs consent message",
        notes:
          "Here E is the estimate and B is the budget. No job is created. Either raise `budget_weighted` in `project.toml`, or go to C11. Printed as `factory: <message>` (guide § 6.0).",
        flag: "trap",
        why: "If the one-job estimate is over `budget_weighted` the command stops.",
        related: [
          { id: "C9", rel: "contrasts" },
          { id: "C11", rel: "contrasts" },
        ],
        source: "factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C11",
        title: "Accept an over-budget --one-job on purpose",
        surface: "terminal",
        phase: "build",
        precondition: "C10 stopped with `needs consent:`.",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/ \\
  --one-job --accept-over-budget`,
        expected: `created job 028 in <job folder>`,
        assert: "The job is created even though the estimate is over budget",
        notes:
          "`--accept-over-budget` needs `--one-job`. The guide gives the `created job` line for `factory new` in general and no separate output for this form.",
        why: "`--accept-over-budget` allows a `--one-job` job whose estimate is over the budget.",
        related: [{ id: "C10", rel: "depends" }],
        source: "factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C12",
        title: "--one-job on a one-plan chain does nothing",
        surface: "terminal",
        phase: "build",
        precondition: "The plans folder holds a single plan whose Next plan line is `none`.",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/ \\
  --one-job`,
        expected: `note: --one-job had no effect: the chain has one plan`,
        assert: "The note says --one-job had no effect",
        notes:
          "This is a note, not an error. The job is still created as a normal single job.",
        related: [{ id: "C9", rel: "diverges" }],
        source: "factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C13",
        title: "--model runs every agent of the job on that model",
        surface: "terminal",
        phase: "build",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/ \\
  --model opus`,
        expected: `created job 028 in <job folder>`,
        assert: "Job created, and every agent of the job runs on opus",
        notes:
          "`factory new` refuses a blank model (`--model must not be blank`), a Fable model (`--model must not be a Fable model`) and a Claude model on a Codex project (`--model <name> is a Claude model, but this project runs Codex`).",
        why: "The value is stored on the job card and carried to the later jobs of a series. Every agent of the job runs on it: the builder and its fix rounds, the reviewer, the committer, the merge message, the KB refresher and the helper agents they start. It replaces `builder_model`, `reviewer_model` and the other role models from `project.toml` for this job only.",
        verify: {
          look: "The review ledger line reads `review model: opus (job model)`.",
        },
        source: "factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "C14",
        title: "Create the job although the spec check refused it",
        surface: "terminal",
        phase: "build",
        precondition:
          "`factory new` refused the spec because the plans touch risky paths and no `## Decisions` item says `Decided by: user`, or because the spec reviewer answered `CHANGES` or gave no verdict.",
        command: `factory --project my-app new login-fix "Fix login redirect" \\
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \\
  --plans docs/superpowers/plans/2026-10-25-login-fix/ \\
  --accept-spec "risky paths reviewed by hand"`,
        expected: `created job 028 in <job folder>`,
        assert: "The job is created even though the spec check refused it",
        flag: "trap",
        notes:
          "The reason is one line, at most 200 characters. The refusal message names the review file under the state folder `spec-reviews/`. The normal fix is to repair the spec, record your answers as `Decided by: user` decisions, commit, and run the same command again. Pass `--accept-spec` only when you decide to go ahead anyway. The guide gives the `created job` line for `factory new` in general and no separate output for this form.",
        why: "A spec refusal creates nothing. `--accept-spec \"<reason>\"` creates the job although the spec check refused it.",
        after:
          "Leaves one or more jobs on the board and nothing is built yet.",
        related: [
          { id: "C6", rel: "contrasts" },
          { id: "C7", rel: "contrasts" },
          { id: "C8", rel: "contrasts" },
        ],
        source: "factory-user-guide.md § 6.1 factory new",
      },
    ],
  },
]
