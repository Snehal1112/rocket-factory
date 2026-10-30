import type { Suite } from "./types"

export const journeysFH: Suite[] = [
  {
    key: "F",
    title: "Run it in the background",
    actor: "factoryd",
    premise:
      "factoryd is an optional background supervisor that keeps a queue of run requests and starts `factory` for you. This journey starts it, queues work, reads its status and stops it without stopping any job.",
    context: [
      "Use plain `factory run` when you work on one project at a time. Use `factoryd` when you want several projects to build at once, runs queued and started for you, a daily token limit, or the multi-project dashboard.",
      "factoryd runs `factory --project <name> run <job>` as a child process for each request. Only one child runs per project, and at most `max_parallel` projects run at once (default 2). Only one factoryd can run per factory home, and it does not start by itself after a reboot.",
      "Stopping factoryd never stops running jobs. The next factoryd adopts them. It finds the `factory` program next to itself, or from the `FACTORY_BIN` variable, so link both programs onto your PATH first.",
    ],
    cases: [
      {
        id: "F1",
        title: "A plain run lives and dies with your terminal",
        surface: "terminal",
        phase: "build",
        precondition:
          "A job 028 exists in my-app. No factoryd is involved in this step.",
        command: `factory --project my-app run 028 --series`,
        expected: `merged`,
        assert: "The last line is merged or needs-human: <reason>",
        notes:
          "The last line is either `merged` or `needs-human: <reason>`. A terminal run ends if you close the terminal. For runs that survive, use `factoryd enqueue` or `nohup`.",
        why: "A terminal run ends if you close the terminal. `factoryd` runs the same command as its own child process, and `factoryd stop` never stops running jobs (§ 6.2, § 7).",
        source:
          "factory-user-guide.md § 6.2 factory run; § 7 factoryd",
      },
      {
        id: "F2",
        title: "Start factoryd in the background",
        surface: "terminal",
        phase: "operate",
        command: `nohup factoryd run >> ~/.factory/factoryd.log 2>&1 &`,
        expected: `api listening on <address>`,
        assert: "factoryd keeps running after you close the terminal",
        notes:
          "The `api listening on <address>` line goes into `~/.factory/factoryd.log`, not to your screen, because the output is redirected. The guide does not say which log file holds it.",
        why: "In normal mode factoryd prints `api listening on <address>`, then loops until you run `factoryd stop`. `nohup` keeps it alive after you close the terminal.",
        after:
          "A supervisor process keeps running until you run `factoryd stop`. It does not start again after a reboot.",
        source:
          "factory-user-guide.md § 7.1 factoryd run; factory-user-guide.md § 7 Command reference: factoryd",
      },
      {
        id: "F3",
        title: "Do one pass only, for testing",
        surface: "terminal",
        phase: "operate",
        precondition: "No factoryd is running for this home.",
        command: `factoryd run --once`,
        expected: `started N, finished N, rejected N`,
        assert: "One pass prints started, finished and rejected counts",
        why: "`--once` does one pass and exits, with no API and no loop.",
        after: "One pass can start queued requests. Check `factoryd status` first.",
        source: "factory-user-guide.md § 7.1 factoryd run; § 7.2 factoryd status",
      },
      {
        id: "F4",
        title: "Ask whether factoryd is running: yes",
        surface: "terminal",
        phase: "observe",
        precondition: "factoryd was started in F2 and is still running.",
        command: `factoryd is-running; echo $?`,
        expected: `0`,
        assert: "The exit code is 0 when factoryd runs",
        notes:
          "The command itself prints nothing. The `0` comes from `echo $?`.",
        why: "`is-running` prints nothing and exits 0 when a factoryd runs for this home. It is meant for scripts, and the `/factory` command uses it to decide whether to queue the work or run it directly.",
        related: [{ id: "F5", rel: "contrasts" }],
        source: "factory-user-guide.md § 7.4 factoryd is-running",
      },
      {
        id: "F5",
        title: "Ask whether factoryd is running: no",
        surface: "terminal",
        phase: "observe",
        precondition: "No factoryd is running for this home.",
        command: `factoryd is-running; echo $?`,
        expected: `1`,
        assert: "The exit code is 1 when factoryd is not running",
        related: [{ id: "F4", rel: "contrasts" }],
        why: "`is-running` exits 1 when no factoryd runs for this home.",
        source: "factory-user-guide.md § 7.4 factoryd is-running",
      },
      {
        id: "F6",
        title: "Read the status page of the queue",
        surface: "terminal",
        phase: "observe",
        command: `factoryd status`,
        expected: `(sections in order: Queue (waiting requests, or (empty)), Running (<project> job <job> pid <pid> age <N>s, or (none)), Results (the last 10, newest first), then the factoryd version)`,
        assert: "Queue, Running and Results sections are printed",
        notes:
          "The guide does not show a full sample. The sections come in the order Queue, Running, Results, then the factoryd version. Each running line reads `<project> job <job> pid <pid> age <N>s`. A result ends as `merged`, `needs-human: <reason>`, `failed (code N) <last error line>`, `lost: ...` or `rejected: ...`.",
        why: "`factoryd status` only reads, so it works whether or not factoryd runs. It prints the daily budget line (only when a budget is set), then Queue, Running, Results and the version.",
        source: "factory-user-guide.md § 7.2 factoryd status",
      },
      {
        id: "F7",
        title: "Queue a run request",
        surface: "terminal",
        phase: "operate",
        precondition:
          "Project my-app is registered and job 028 exists on its board.",
        command: `factoryd enqueue --project my-app --job 028`,
        expected: `1761379200000-my-app-028`,
        assert: "A new request id is printed",
        notes:
          "The id shown is the guide's example. Yours starts with a different number.",
        why: "A request starts `factory --project <name> run <job>`. `enqueue` does not check that the project exists and does not need factoryd to be running. An unknown project is rejected later with the reason `unknown project`. If no factoryd runs, the request waits in the queue until one starts.",
        after:
          "A file stays in `~/.factory/queue/` until factoryd picks it up, then its result lands in `~/.factory/results/`.",
        source:
          "factory-user-guide.md § 7.5 factoryd enqueue; factory-user-guide.md § 12 Where files live",
      },
      {
        id: "F8",
        title: "Queue a whole series",
        surface: "terminal",
        phase: "operate",
        precondition: "Job 028 is the first plan of a multi-plan series.",
        command: `factoryd enqueue --project my-app --job 028 --series`,
        expected: `1761379200000-my-app-028`,
        assert: "A request id is printed for the series run",
        notes:
          "The id shown is the guide's example. Yours starts with a different number.",
        why: "`--series` is for a run request only. After each merge the run continues with the next plan.",
        after:
          "A request file stays in `~/.factory/queue/` until factoryd picks it up.",
        related: [{ id: "F7", rel: "depends" }],
        source: "factory-user-guide.md § 7.5 factoryd enqueue; § 12 Where files live",
      },
      {
        id: "F9",
        title: "Queue an approval instead of a run",
        surface: "terminal",
        phase: "operate",
        precondition: "Job 028 is in needs-human or approve.",
        command: `factoryd enqueue --project my-app --job 028 --approve`,
        expected: `1761379200000-my-app-028`,
        assert: "A request id is printed for the approve request",
        notes:
          "The id shown is the guide's example. Yours starts with a different number.",
        why: "`--approve` makes it an approve request, so it starts `factory --project <name> approve <job>`. Approvals still start when the daily budget is used up.",
        after:
          "A request file stays in `~/.factory/queue/` until factoryd picks it up.",
        related: [{ id: "F7", rel: "contrasts" }],
        source:
          "factory-user-guide.md § 7.5 factoryd enqueue; § 12 Where files live; factory-user-guide.md § 7.6 The daily budget",
      },
      {
        id: "F10",
        title: "A series cannot be approved",
        surface: "terminal",
        phase: "operate",
        command: `factoryd enqueue --project my-app --job 028 --series --approve`,
        expected: `series is only valid for a run request`,
        assert: "The request is refused with the series message",
        flag: "trap",
        notes:
          "Pick one: `--series` for a run request, or `--approve` for an approve request.",
        why: "`--series` together with `--approve` is refused.",
        related: [{ id: "F8", rel: "contrasts" }],
        source: "factory-user-guide.md § 7.5 factoryd enqueue",
      },
      {
        id: "F11",
        title: "There is no way to approve without review here",
        surface: "terminal",
        phase: "observe",
        command: `factoryd enqueue --help`,
        expected: `Options listed: --project <name>, --job <id>, --series, --approve (no --without-review)`,
        assert: "No --without-review option in the list",
        flag: "gap",
        notes:
          "The guide gives no verbatim help text. Check that the options are only `--project`, `--job`, `--series` and `--approve`. To merge without review, run `factory approve 028 --without-review` yourself.",
        why: "`--without-review` does not exist on `enqueue`. You can pass it only through the API or by running `factory approve --without-review` yourself.",
        source: "factory-user-guide.md § 7.5 factoryd enqueue",
      },
      {
        id: "F12",
        title: "Stop factoryd without stopping jobs",
        surface: "terminal",
        phase: "operate",
        precondition: "factoryd is running.",
        command: `factoryd stop`,
        expected: `stop requested, factoryd pid N will end after its current tick; running jobs keep running`,
        assert: "The stop message says running jobs keep running",
        notes:
          "If no factoryd is running it prints `no factoryd is running; the stop file is removed when the next one starts`.",
        why: "`stop` writes a stop flag. The supervisor ends after its current pass, and the next factoryd adopts any running jobs.",
        after:
          "Running jobs keep running with no supervisor. Start factoryd again to adopt them, and wait for jobs to finish before you upgrade.",
        source:
          "factory-user-guide.md § 7.3 factoryd stop; factory-user-guide.md § 7 Command reference: factoryd",
      },
      {
        id: "F13",
        title: "A second factoryd is refused",
        surface: "terminal",
        phase: "operate",
        precondition: "A factoryd is already running for this home.",
        command: `factoryd run`,
        expected: `factoryd is already running (pid N)`,
        assert: "The second factoryd refuses to start",
        flag: "trap",
        notes:
          "Use `factoryd status`, or `factoryd stop` first. Do not start a second one.",
        why: "Only one factoryd can run per factory home.",
        source:
          "factory-user-guide.md § 7.1 factoryd run; factory-user-guide.md § 13 Troubleshooting",
      },
      {
        id: "F14",
        title: "A lost result: what to read",
        surface: "files",
        phase: "observe",
        precondition: "A request in `factoryd status` ended with `lost: ...`.",
        command: `less ~/.factory/logs/my-app/<request>.err`,
        expected: `The child's error output. Read it together with progress.md in the job folder.`,
        assert: "You found why the child died",
        notes:
          "The guide gives no exact text for this file. After reading, run the job again by hand or enqueue again.",
        why: "`lost` means the child died without reporting a result. factoryd never restarts a lost job.",
        source:
          "factory-user-guide.md § 7.2 factoryd status; factory-user-guide.md § 13 Troubleshooting",
      },
      {
        id: "F15",
        title: "Set a daily token limit and see it in status",
        surface: "terminal",
        phase: "operate",
        precondition:
          "`daily_budget_weighted` is set in ~/.factory/config.toml.",
        command: `factoryd status`,
        expected: `daily budget: <used> of <limit> weighted tokens today (<pct>%), resets at <time>`,
        assert: "The first line is the daily budget line",
        notes:
          "The line is printed only when a daily budget is set. The key goes in `~/.factory/config.toml`, and unknown keys there are rejected.",
        why: "When the day's tokens are used up, new run requests wait. Approvals still start. Running jobs are never stopped. The `/factory` fallback that runs `factory` directly does not check this budget.",
        source:
          "factory-user-guide.md § 7.6 The daily budget; factory-user-guide.md § 7.8 Factory home settings",
      },
      {
        id: "F16",
        title: "Keep the API token private",
        surface: "terminal",
        phase: "observe",
        command: `ls ~/.factory/api.token`,
        expected: `~/.factory/api.token`,
        assert: "The file exists and you did not open it",
        flag: "trap",
        notes:
          "This only checks that the file exists. Do not print it. Do not paste the token into chat, tickets or screenshots. This `ls` is a check added by the journeybook, not a command from the guide.",
        why: "The API listens on `127.0.0.1:8766` by default and is protected by the secret token in `~/.factory/api.token`. The dashboard reads that file on its own server side, so you do not need to touch it.",
        source: "factory-user-guide.md § 7.7 The local API",
      },
      {
        id: "F17",
        title: "Change how often factoryd checks the queue",
        surface: "terminal",
        phase: "operate",
        precondition: "No factoryd is running for this home.",
        command: `factoryd run --interval-ms 500`,
        expected: `api listening on <address>`,
        assert: "factoryd starts and prints api listening on <address>",
        notes:
          "The value is in milliseconds. The default is 2000 and the lowest accepted value is 50. This form keeps your terminal busy. Run `factoryd stop` to end it.",
        why: "`--interval-ms` sets how often factoryd checks the queue. In normal mode it prints `api listening on <address>`, then loops until you run `factoryd stop`.",
        after:
          "A supervisor process runs until you run `factoryd stop`. If another `factoryd` already runs for this home, it stops with `factoryd is already running (pid N)`.",
        related: [{ id: "F2", rel: "contrasts" }],
        source: "factory-user-guide.md § 7.1 factoryd run",
      },
    ],
  },
  {
    key: "G",
    title: "Projects and upkeep",
    actor: "You",
    premise:
      "Registered projects, the presets and the factory programs themselves need a little upkeep. This journey lists, adds and removes projects, refreshes presets and upgrades the factory after a new build.",
    context: [
      "The `factory project`, `presets`, `install` and `upgrade` commands use only the factory home, which is `$FACTORY_HOME` or else `~/.factory`. They need no repository, and `--root` does nothing useful for them.",
      "Run `factory upgrade` after every time you install a new `factory` binary. Use `--dry-run` first to see what would change.",
      "Projects live in `~/.factory/projects/<name>/`. Removing a project deletes only that folder. Your repository and the presets stay.",
    ],
    cases: [
      {
        id: "G1",
        title: "List your registered projects",
        surface: "terminal",
        phase: "observe",
        command: `factory project list`,
        expected: `<name> <preset> <base-branch> <folder>`,
        assert: "One line per project, or no projects",
        notes:
          "It prints `no projects` when nothing is registered. A project that cannot be used shows as `<name>  disabled: <why>`.",
        why: "A project that cannot be used is listed as `<name>  disabled: <why>`.",
        source: "factory-user-guide.md § 6.15 factory project (project list)",
      },
      {
        id: "G2",
        title: "Find out which project a folder belongs to",
        surface: "terminal",
        phase: "observe",
        command: `factory project which ~/code/my-app/src`,
        expected: `my-app`,
        assert: "The project name is printed",
        notes:
          "Without a path it uses your current folder. If none matches it errors with `no registered project contains <path>`.",
        source: "factory-user-guide.md § 6.15 factory project (project which)",
      },
      {
        id: "G3",
        title: "Check that a project is healthy",
        surface: "terminal",
        phase: "observe",
        command: `factory project show my-app`,
        expected: `<key>: <value> (one line per setting)
problems: none`,
        assert: "The last line is problems: none",
        notes:
          "If it lists problems instead, fix them before you continue. Run it again after any settings edit.",
        why: "It prints the resolved settings, one `key: value` per line. If the project settings are missing or broken, the factory treats every change as risky and never falls back to a permissive setting.",
        source:
          "factory-user-guide.md § 6.15 factory project (project show); factory-user-guide.md § 11 Settings that matter most",
      },
      {
        id: "G4",
        title: "Register a monorepo subfolder with its own name and branch",
        surface: "terminal",
        phase: "setup",
        precondition:
          "~/code/monorepo is a git repository and the branch main exists locally.",
        command: `factory project add ~/code/monorepo/services/api --preset go --name api --base main`,
        expected: `added project api (go, base main) at <folder>`,
        assert: "The added project line shows the name, preset and base",
        notes:
          "The guide shows the pattern `added project <name> (<preset>, base <branch>) at <folder>`. The values here are filled in from the command.",
        why: "`<PATH>` can be a subfolder of the repository for a monorepo project. `--name` is 1 to 40 characters of `a-z`, `0-9` and `-`. `--base` must exist locally.",
        after:
          "A folder `~/.factory/projects/api/` with `project.toml` and empty `agents/`, `state/` and `worktrees/` folders. Your repository is not changed. To undo it, run `factory project remove api`.",
        source: "factory-user-guide.md § 6.15 factory project (project add)",
      },
      {
        id: "G5",
        title: "Register a project that uses Codex",
        surface: "terminal",
        phase: "setup",
        precondition:
          "No project named my-app is registered yet (or run `factory project remove my-app` first).",
        command: `factory project add ~/code/my-app --preset rust --agent-cli codex`,
        expected: `added project my-app (rust, base <branch>) at <folder>`,
        assert: "The project is added with the rust preset",
        notes:
          "`--agent-cli codex` seeds Codex model names. If the repository is in a detached HEAD state, add `--base`.",
        after:
          "The same folder under `~/.factory/projects/my-app/` as any added project. To undo it, run `factory project remove my-app`.",
        source: "factory-user-guide.md § 6.15 factory project (project add)",
      },
      {
        id: "G6",
        title: "Remove a project safely",
        surface: "terminal",
        phase: "operate",
        flag: "trap",
        precondition:
          "Every job of my-app is merged or cancelled and no driver holds its lease.",
        command: `factory project remove my-app`,
        expected: `The guide gives no exact output. The project folder is deleted and any job branches left in place are listed.`,
        assert: "The project is gone and your repository is untouched",
        notes:
          "It refuses while a driver holds the project lease. Check with `factory project list` afterwards.",
        why: "It deletes only `~/.factory/projects/<name>/`. It never deletes job branches, and it lists any it leaves in place.",
        verify: {
          command: `factory project list`,
          look: "my-app is no longer in the list.",
        },
        after:
          "Your repository and the presets stay. Any job branches stay in your repository. Run `factory cleanup` on merged or cancelled jobs before you remove the project if you want those branches gone. The board, ledgers and logs of the project go with its folder (§ 12).",
        related: [{ id: "G7", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.15 factory project (project remove)",
      },
      {
        id: "G7",
        title: "Force-remove a project with unfinished jobs",
        surface: "terminal",
        phase: "operate",
        precondition:
          "my-app has jobs that are not merged or cancelled, or its project file is broken, and you accept losing its board.",
        command: `factory project remove my-app --force`,
        expected: `The guide gives no exact output. The project folder is deleted and any job branches left in place are listed.`,
        assert: "The project is removed despite unfinished jobs",
        flag: "trap",
        notes:
          "`--force` does not override a held project lease. If a driver holds the lease, run `factory --project my-app cancel <job> --force`, or delete the lease file.",
        why: "`--force` removes the project even though some jobs are not merged or cancelled, or the project file is broken.",
        after:
          "The board, ledgers and logs of the project are deleted with its folder. Your repository, the presets and every job branch stay.",
        related: [{ id: "G6", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.15 factory project (project remove)",
      },
      {
        id: "G8",
        title: "Refresh the presets",
        surface: "terminal",
        phase: "operate",
        command: `factory presets refresh`,
        expected: `Each preset file ends as one of: written, updated, unchanged or refused`,
        assert: "Every file shows written, updated, unchanged or refused",
        notes:
          "The guide lists the four states but gives no exact output lines. There is no dry-run option here. `factory upgrade --dry-run` shows the preview.",
        why: "`written` means the file was missing and is now created. `updated` means you had not edited it. `unchanged` means it already matches. `refused` means you edited it, so it was kept.",
        after:
          "Files in `~/.factory/presets/` match the text built into the binary, except the ones shown as `refused`.",
        source: "factory-user-guide.md § 6.16 factory presets refresh",
      },
      {
        id: "G9",
        title: "Overwrite your hand-edited presets",
        surface: "terminal",
        phase: "operate",
        precondition:
          "A refresh showed `refused` files and you want to lose those edits.",
        command: `factory presets refresh --force`,
        expected: `Each preset file ends as one of: written, updated, unchanged or refused`,
        assert: "Files you edited are replaced",
        flag: "trap",
        notes:
          "Use `--force` only if you want to lose your edits. Copy any file you want to keep first.",
        why: "`--force` overwrites files you edited by hand.",
        after:
          "Your hand edits in `~/.factory/presets/` are gone. Nothing in the project folders changes.",
        related: [{ id: "G8", rel: "contrasts" }],
        source:
          "factory-user-guide.md § 6.16 factory presets refresh; factory-user-guide.md § 13 Troubleshooting",
      },
      {
        id: "G10",
        title: "Install the Claude command into a folder you choose",
        surface: "terminal",
        phase: "setup",
        command: `factory install --dir ~/.claude/commands`,
        expected: `installed <path>`,
        assert: "The factory.md path is printed as installed",
        notes:
          "It prints `already installed: <path>` if the file is already there. No Codex skill is installed with `--dir`.",
        why: "`--dir` writes only `factory.md` into that folder.",
        after:
          "One file, `factory.md`, in that folder. Delete it to undo.",
        source: "factory-user-guide.md § 6.17 factory install",
      },
      {
        id: "G11",
        title: "Replace a changed copy of the command",
        surface: "terminal",
        phase: "setup",
        precondition: "The installed factory.md differs from the shipped one.",
        command: `factory install --force`,
        expected: `installed <path>`,
        assert: "The changed copy is replaced",
        flag: "trap",
        notes:
          "Without `--force` it stops with `<path> exists and differs from the shipped command, use --force to replace it`. For the Codex skill that is only a note, and the command still succeeds.",
        why: "`--force` replaces an existing file that differs.",
        after:
          "Your edits to `~/.claude/commands/factory.md` are gone. Copy the file first if you want to keep them.",
        source: "factory-user-guide.md § 6.17 factory install",
      },
      {
        id: "G12",
        title: "Preview an upgrade",
        surface: "terminal",
        phase: "operate",
        precondition: "You installed a new `factory` binary.",
        command: `factory upgrade --dry-run`,
        expected: `Changes are shown as "would ..." lines. Files that need nothing show as "unchanged".`,
        assert: "Nothing is written, changes show as would",
        notes:
          "The guide gives no full sample. `shadowed:` lines name project or repository files that hide the new preset text.",
        why: "`--dry-run` shows what would change and writes nothing. If the home was last used by a newer binary, it refuses and tells you to install a newer binary.",
        source: "factory-user-guide.md § 6.18 factory upgrade",
      },
      {
        id: "G13",
        title: "Run the upgrade",
        surface: "terminal",
        phase: "operate",
        precondition: "G12 showed changes you accept.",
        command: `factory upgrade`,
        expected: `plugin: factory@rocket-factory is installed; update it with: claude plugin update factory@rocket-factory
(without the plugin: the command file is updated, then the plugin install note from section 6.17, or note: to switch to the plugin, run the two commands below, then remove <path>: followed by the two claude plugin commands when factory.md exists)`,
        assert: "With the plugin installed, upgrade prints the plugin update command",
        notes:
          "The first line is what you see with the plugin `factory@rocket-factory` installed. In that case `factory.md` is left alone, and the duplicate note from section 6.17 is printed when `factory.md` still exists. Without the plugin, `factory.md` is updated and a changed old copy is saved as `factory.md.bak`. The Codex skill and the presets are updated either way, and any `shadowed:` lines name files that hide the new preset text. The guide gives no further exact lines.",
        why: "In order: it checks the home was not last used by a newer binary, updates `~/.claude/commands/factory.md` only when the Claude Code plugin is not installed, updates the Codex skill only if already installed, refreshes presets you have not edited, reports `shadowed:` lines, and writes the version marker in the factory home.",
        after:
          "A `factory.md.bak` next to the command file if the plugin is not installed and the old copy had changed. The version marker in the factory home now matches the new binary.",
        related: [
          { id: "G12", rel: "depends" },
          { id: "A10", rel: "contrasts" },
        ],
        source: "factory-user-guide.md § 6.18 factory upgrade; factory-user-guide.md § 6.17 factory install",
      },
      {
        id: "G14",
        title: "Update the factory itself",
        surface: "terminal",
        phase: "operate",
        precondition: "You work from a clone of rocket-factory.",
        command: `cd ~/data/rocket/rocket-factory
git pull
cargo build -j4 -p factory-cli -p factory-supervisor
factoryd stop                    # wait for running jobs to finish
factory upgrade --dry-run
factory upgrade
nohup factoryd run >> ~/.factory/factoryd.log 2>&1 &`,
        expected: `stop requested, factoryd pid N will end after its current tick; running jobs keep running`,
        assert: "factoryd stops, upgrade runs, factoryd starts again",
        notes:
          "The expected line is the output of `factoryd stop`. The other commands print their own output, covered in the steps above. Wait for running jobs to finish before `factoryd stop`.",
        why: "Run `factory upgrade` after installing a new `factory` binary. Stopping factoryd never stops running jobs, and the next factoryd adopts them.",
        after:
          "factoryd runs again from the new build. Its log is `~/.factory/factoryd.log`.",
        related: [
          { id: "G12", rel: "depends" },
          { id: "G13", rel: "depends" },
        ],
        source:
          "factory-user-guide.md § 10 Recipes (Update the factory itself); factory-user-guide.md § 6.18 factory upgrade",
      },
      {
        id: "G15",
        title: "Check the layout of the factory home",
        surface: "files",
        phase: "observe",
        command: `ls ~/.factory`,
        expected: `(the entries of the factory home. The guide shows them as a tree: config.toml, VERSION, api.token, factoryd.log.jsonl, presets/, projects/, queue/, results/, run/, logs/)`,
        assert: "Presets, projects, queue and results folders exist",
        notes:
          "The guide shows this layout as a tree, not as `ls` output. `config.toml` is optional, and the queue, results, run and logs folders appear once factoryd has run. `api.token` is the API secret: never open it or paste it.",
        why: "`$FACTORY_HOME` overrides `~/.factory`. Each project keeps its settings in `projects/<name>/project.toml` and its board and logs under `projects/<name>/state/`.",
        source: "factory-user-guide.md § 12 Where files live",
      },
      {
        id: "G16",
        title: "Find out why your factory is older than its home",
        surface: "terminal",
        phase: "operate",
        flag: "trap",
        precondition:
          "A factory command stops with `this home was last used by factory X, this binary is Y`.",
        command: `which factory factoryd
factory --version
cat ~/.factory/VERSION`,
        expected: `(the path of each program, then the version of the binary you ran, then the version in the marker file. The marker is higher than the binary.)`,
        assert: "The binary version is lower than the marker",
        notes:
          "The guide gives no sample for these lines. `factory --version` and `cat` are checks added by the journeybook, not commands from the guide. The links you made in § 4.2 point at `target/debug/factory` and `target/debug/factoryd` in your clone, so the programs on your PATH change only when you build in that clone. A newer factory run from anywhere else raises the marker, and the older build on your PATH then refuses to run. To fix it, build again in the clone and run `factory upgrade` (G14 has the commands). The guide's other fix, deleting `VERSION`, switches the check off.",
        why: "The factory home keeps a version marker in `VERSION`. `factory project add`, `factoryd run` and `factory upgrade` each raise it to the version of the binary that runs them when that version is higher. A binary lower than the marker stops with the error.",
        verify: {
          command: `factory --version`,
          look: "The version printed is the same as or higher than the one in `~/.factory/VERSION`.",
        },
        related: [
          { id: "G13", rel: "depends" },
          { id: "G14", rel: "depends" },
        ],
        source:
          "crates/factory-core/src/version.rs:42-53; crates/factory-core/src/version.rs:57-69; crates/factory-cli/src/project.rs:170; crates/factory-supervisor/src/bin/factoryd.rs:72; crates/factory-cli/src/upgrade.rs:117; factory-user-guide.md § 4.2 Put them on your PATH; factory-user-guide.md § 12 Where files live",
      },
      {
        id: "G17",
        title: "Add the missing settings to one project as comments",
        surface: "terminal",
        phase: "operate",
        precondition: "Project my-app is registered.",
        command: `factory project defaults my-app`,
        expected: `my-app: added the settings block
my-app: already lists every setting`,
        assert: "One line names my-app",
        notes:
          "The first line is printed when settings were added. The second line is printed when nothing was missing. With neither a name nor `--all` the command stops with `name a project or pass --all`.",
        why: "It adds every setting the project's `project.toml` does not list yet, as a comment with its default. The rest of the file stays as it is, so it is a safe way to see which settings exist.",
        after:
          "Writes only `~/.factory/projects/my-app/project.toml`. Your repository does not change.",
        related: [{ id: "G18", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.15 factory project (project defaults)",
      },
      {
        id: "G18",
        title: "Add the missing settings to every project",
        surface: "terminal",
        phase: "operate",
        precondition: "At least one project is registered.",
        command: `factory project defaults --all`,
        expected: `<name>: added the settings block
<name>: already lists every setting
<name>: skipped, <why>`,
        assert: "One line per registered project",
        notes:
          "A project that cannot be read prints the `skipped` line and the others still run.",
        why: "`--all` runs the same change for every registered project.",
        after:
          "Writes only `~/.factory/projects/<name>/project.toml` of each project. Your repositories do not change.",
        related: [{ id: "G17", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.15 factory project (project defaults)",
      },
      {
        id: "G19",
        title: "Probe what every agent sees",
        surface: "terminal",
        phase: "operate",
        flag: "trap",
        precondition:
          "The agent program (`claude` or `codex`) is on your PATH and signed in. `factoryd` is not needed.",
        command: `factory --project my-app probe`,
        expected: `harness claude  cli <claude --version>

role           agent_text  skills   bootstrap  no_extra_skills
builder        ok          ok       ok         -
...

Report: <state dir>/probes/<stamp>-claude.json
Transcripts: <state dir>/probes/<stamp>-claude`,
        assert: "Exit code 0 and every expected cell reads ok",
        notes:
          "**It spends real tokens**: one short turn per role, at most 50,000 weighted tokens each. `factory run` and the tests never start it. The table is followed by one `Skills seen (<role>): ...` line per role and a `Missing (<role>) <channel>: <detail>` line for every channel that failed. Use it after you change agent files, skills or the agent program, and before a release.",
        why: "For each role it builds the real agent setup, adds a unique marker to the agent text, turns every tool off and asks the agent, in one turn, what reached it. The channels are `agent_text`, `skills`, `bootstrap` and `no_extra_skills`.",
        verify: {
          look: "Every cell the table expects reads `ok`, and no `Missing` line is printed.",
        },
        after:
          "Leaves a report and a transcripts folder under `<state dir>/probes/`. It does not touch the board, a job, a worktree or your repository, and it does not take the project lease.",
        related: [
          { id: "G20", rel: "contrasts" },
          { id: "G22", rel: "contrasts" },
        ],
        source: "factory-user-guide.md § 6.19 factory probe",
      },
      {
        id: "G20",
        title: "Probe one role only",
        surface: "terminal",
        phase: "operate",
        flag: "trap",
        precondition:
          "The agent program (`claude` or `codex`) is on your PATH and signed in.",
        command: `factory --project my-app probe --role builder`,
        expected: `(the same table with one row, builder, then the Skills seen line, and the Report and Transcripts lines)`,
        assert: "The table has one row, builder",
        notes:
          "`--role` takes `builder`, `reviewer`, `spec-reviewer`, `committer` or `kb`. Repeat it for more than one role. It spends real tokens: one short turn for the role, at most 50,000 weighted tokens. A bad option value exits with code 2.",
        why: "`--role` limits the probe to the roles you name. The default is all five.",
        after:
          "Leaves a report and a transcripts folder under `<state dir>/probes/`. Nothing else changes.",
        related: [{ id: "G19", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.19 factory probe",
      },
      {
        id: "G21",
        title: "Print the probe report as JSON",
        surface: "terminal",
        phase: "operate",
        flag: "trap",
        precondition:
          "The agent program (`claude` or `codex`) is on your PATH and signed in.",
        command: `factory --project my-app probe --json`,
        expected: `(the report as JSON and nothing else)`,
        assert: "The output is JSON only, with no table",
        notes:
          "It spends real tokens: one short turn per role, at most 50,000 weighted tokens each.",
        why: "`--json` prints the report as JSON and nothing else.",
        after:
          "Writes only under `<state dir>/probes/`. It does not touch the board, a job, a worktree or your repository.",
        related: [{ id: "G19", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.19 factory probe",
      },
      {
        id: "G22",
        title: "A MISSING cell makes probe exit with code 1",
        surface: "terminal",
        phase: "operate",
        flag: "trap",
        precondition:
          "A channel the probe expects did not reach one role, for example after you changed an agent file or a skill.",
        command: `factory --project my-app probe; echo $?`,
        expected: `(the table with a MISSING cell, then a line Missing (<role>) <channel>: <detail>)
1`,
        assert: "A MISSING cell, a Missing line and exit code 1",
        notes:
          "A cell is `ok` (expected and present), `MISSING` (expected and absent), `-` (not expected) or `extra` (not expected but present). The table is still printed when the exit code is 1. Exit code 0 means every expected channel is present, and 2 means a bad option value. It spends real tokens: one short turn per role, at most 50,000 weighted tokens each.",
        why: "A `MISSING` cell means the channel was expected and absent for that role. The `Missing` line gives the detail.",
        after:
          "Leaves a report and a transcripts folder under `<state dir>/probes/`. Read the transcripts there to see what the agent answered.",
        related: [{ id: "G19", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.19 factory probe",
      },
      {
        id: "G23",
        title: "Check that the plugin files are current",
        surface: "terminal",
        phase: "operate",
        precondition:
          "You work on the factory itself and your current folder is the root of a rocket-factory clone.",
        command: `factory plugin sync --check`,
        expected: `plugin files are current`,
        assert: "Output is plugin files are current",
        notes:
          "Only people who work on the factory itself need this. In your own projects you never need it. Install the plugin as in A10. When a file differs the command stops with `plugin files out of date: <file>; run factory plugin sync`. Outside the repository root it stops with `<path> is not the repository root: .claude-plugin/marketplace.json is missing; use --repo`.",
        why: "`--check` writes nothing and fails when a plugin file differs from the text generated from the command text built into the binary.",
        after: "Nothing was written.",
        related: [{ id: "A10", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.20 factory plugin sync",
      },
    ],
  },
  {
    key: "H",
    title: "The dashboard",
    actor: "You",
    premise:
      "The dashboard is a local web page that shows the board of every project. This journey builds it, starts it in the right mode and reads its cards and error banners.",
    context: [
      "Build the dashboard once. It is a separate program called `factory-server`, started with `--home` to show every registered project through factoryd's API.",
      "The recommended `--home` mode needs factoryd running. From the page you can create a job and approve, rework or cancel one. The old `--root` mode is read-only and reads the old in-repository `.superpowers/` folder.",
    ],
    cases: [
      {
        id: "H1",
        title: "Build the dashboard",
        surface: "terminal",
        phase: "setup",
        precondition: "You are in the rocket-factory repository root.",
        command: `cargo build -j4 --manifest-path dashboard/Cargo.toml`,
        expected: `dashboard/target/debug/factory-server`,
        assert: "The factory-server program exists",
        notes:
          "The guide gives no build output. The expected line is the path of the program this produces. Always use `-j4`. This `ls` is a check added by the journeybook, not a command from the guide.",
        why: "The dashboard is its own workspace, so it builds with its own manifest.",
        verify: {
          command: `ls dashboard/target/debug/factory-server`,
          look: "The path is printed and no error appears.",
        },
        source: "factory-user-guide.md § 4.1 Build the programs",
      },
      {
        id: "H2",
        title: "Start it for every project",
        surface: "terminal",
        phase: "operate",
        precondition: "factoryd is running (see F2).",
        command: `nohup factoryd run >> ~/.factory/factoryd.log 2>&1 &
dashboard/target/debug/factory-server --home ~/.factory --port 8765`,
        expected: `The guide gives no output for this command. The page is served on port 8765.`,
        assert: "The server keeps running with no error",
        notes:
          "Skip the first line if factoryd already runs. `--home` cannot be combined with `--root`.",
        why: "`--home` shows every project of a factory home and needs factoryd. The dashboard holds the API token server-side.",
        after:
          "A server process stays in this terminal until you stop it. factoryd keeps running in the background.",
        source:
          "factory-user-guide.md § 8 The dashboard; factory-user-guide.md § 7.7 The local API",
      },
      {
        id: "H3",
        title: "Open the page and find a tab for each project",
        surface: "dashboard",
        phase: "observe",
        precondition: "H2 is running.",
        command: `http://127.0.0.1:8765/`,
        expected: `A tab for each project`,
        assert: "Each registered project has its own tab",
        notes:
          "Open this address in your browser. From the page you can create a job and approve, rework or cancel one.",
        related: [{ id: "H2", rel: "depends" }],
        source: "factory-user-guide.md § 8 The dashboard",
      },
      {
        id: "H4",
        title: "Read a job card",
        surface: "dashboard",
        phase: "observe",
        precondition: "The project tab has at least one job.",
        command: `http://127.0.0.1:8765/`,
        expected: `Each card shows: the job id, a "plan k of N" chip for a series, the feature, the branch, the builder round, review chips, time in the stage, tokens against the budget (amber at 80%, red at 100%), and ready-to-copy approve, rework and cancel commands.`,
        assert: "The card shows id, feature, branch, tokens and commands",
        notes:
          "A job waiting on you has a button that shows what went wrong.",
        source: "factory-user-guide.md § 8 The dashboard",
      },
      {
        id: "H5",
        title: "A registered project looks empty in --root mode",
        surface: "dashboard",
        phase: "observe",
        precondition: "my-app was registered with `factory project add`.",
        command: `dashboard/target/debug/factory-server --root /path/to/repo --port 8765`,
        expected: `The board is empty.`,
        assert: "The board is empty for a registered project",
        flag: "divergence",
        notes:
          "Use `--home`, not `--root`, for registered projects. `--root` shows one repository, read-only.",
        why: "`--root` reads the old in-repository `.superpowers/` folder. For a project registered with `factory project add`, the board lives in the factory home, so this mode looks empty.",
        related: [{ id: "H2", rel: "diverges" }],
        source:
          "factory-user-guide.md § 8 The dashboard; factory-user-guide.md § 13 Troubleshooting",
      },
      {
        id: "H6",
        title: "Choose the page port and the factoryd address",
        surface: "terminal",
        phase: "operate",
        precondition: "factoryd is running.",
        command: `dashboard/target/debug/factory-server --home ~/.factory --port 8770 --api 127.0.0.1:8766`,
        expected: `The guide gives no output for this command. The page is served on port 8770.`,
        assert: "The page opens on port 8770",
        notes:
          "`--port` defaults to 8765. `--api` defaults to `127.0.0.1:8766` and is loopback only.",
        source: "factory-user-guide.md § 8 The dashboard",
      },
      {
        id: "H7",
        title: "api_unreachable means factoryd is not running",
        surface: "dashboard",
        phase: "observe",
        precondition: "The dashboard runs in --home mode and factoryd is stopped.",
        command: `http://127.0.0.1:8765/`,
        expected: `api_unreachable`,
        assert: "The page shows `api_unreachable`.",
        why: "The dashboard cannot reach factoryd's API. Start factoryd (see F2).",
        related: [{ id: "F2", rel: "depends" }],
        source: "factory-user-guide.md § 8 The dashboard",
      },
      {
        id: "H8",
        title: "token_rejected means factoryd refused the token",
        surface: "dashboard",
        phase: "observe",
        precondition:
          "The dashboard and factoryd use different factory homes.",
        command: `http://127.0.0.1:8765/`,
        expected: `token_rejected`,
        assert: "The page shows `token_rejected`.",
        notes:
          "Make sure factoryd uses the same home. Do not paste the token anywhere to debug this.",
        why: "factoryd does not accept the token the dashboard read.",
        related: [{ id: "H7", rel: "contrasts" }],
        source:
          "factory-user-guide.md § 8 The dashboard; factory-user-guide.md § 13 Troubleshooting",
      },
    ],
  },
]
