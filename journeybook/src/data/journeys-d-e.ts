import type { Suite } from "./types"

export const journeysDE: Suite[] = [
  {
    key: "D",
    title: "When a job stops",
    actor: "You",
    premise:
      "A job that says needs-human has stopped and is waiting for you. This journey reads the reason, then walks the recovery commands: approve, rework, cancel and cleanup.",
    context: [
      "The factory builds a change, runs the checks, has a reviewer read it, then decides. If anything looks unsafe or unfinished it does not guess. It stops and waits for you. That state is called needs-human, or parked.",
      "Your first move is always the same: run `factory status`. It prints the reason, a short piece of evidence and the `next:` commands that are valid for that reason. Nothing in `status` changes anything, so run it whenever you are unsure.",
      "After you pick a path, `approve` merges the job straight away, `rework` sends it back to build (you then run it again), and `cancel` stops it (you then clean up its files when you are ready). The factory never pushes, so a merge only touches your local branch.",
    ],
    cases: [
      {
        id: "D1",
        title: "Read the status of a job that is waiting for you",
        surface: "terminal",
        phase: "stuck",
        precondition:
          "A run ended with `needs-human: <reason>` for job 028 in project my-app.",
        command: `factory --project my-app status`,
        expected: `project: free
(a table with the columns ID, STAGE, ROUND, REVIEWS, TOKENS, SERIES, SLUG, one row per job)
028 needs a human: <reason>
(a short piece of evidence)
next: (the commands you can run)`,
        assert: "A block reading 028 needs a human, with a reason",
        notes:
          "The lease line can also read `project: busy, job N (owner O, S s left)` or `project: stale lease from O (job N), parked`. The block has three parts: the reason, a short piece of evidence, and the `next:` commands.",
        why: "`status` prints a waiting block for each job that needs you, and the `next:` commands are the ones valid for that reason.",
        verify: {
          command: `factory --project my-app status`,
          look: "Find the line `028 needs a human: <reason>` below the table.",
        },
        related: [{ id: "D22", rel: "contrasts" }],
        source:
          "factory-user-guide.md § 6.3 factory status; factory-user-guide.md § 9 When a job stops: what to do",
      },
      {
        id: "D2",
        title: "Reason: the change touched a sensitive file",
        surface: "terminal",
        phase: "stuck",
        precondition: "Job 028 is in needs-human after touching a sensitive file.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: risky diff: risky paths (K): ...`,
        assert: "Reason starts with risky diff: risky paths",
        notes:
          "This always needs you. Read the diff. Then run `factory --project my-app approve 028`, or `rework` with a note.",
        why: "A risky diff is a change touching a sensitive path such as `Cargo.toml`, authentication or secrets, and it always needs your approval.",
        related: [
          { id: "D14", rel: "depends" },
          { id: "D16", rel: "depends" },
        ],
        source:
          "factory-user-guide.md § 9 When a job stops: what to do; factory-user-guide.md § 2 The ideas you need (glossary)",
      },
      {
        id: "D3",
        title: "Reason: the tests still fail after the fix rounds",
        surface: "terminal",
        phase: "stuck",
        precondition: "Job 028 is in needs-human after its fix rounds ran out.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: checks failed after N fix rounds`,
        assert: "Reason reads checks failed after N fix rounds",
        notes:
          "Send the builder a hint with `factory --project my-app rework 028 \"<one line hint>\"`, then run the job again.",
        why: "The builder gets a fixed number of fix rounds after a failed check. When the tests still fail after them, the job stops.",
        related: [{ id: "D16", rel: "depends" }],
        source:
          "factory-user-guide.md § 9 When a job stops: what to do; factory-user-guide.md § 11 Settings that matter most",
      },
      {
        id: "D4",
        title: "Reason: the reviewer still wants changes",
        surface: "terminal",
        phase: "stuck",
        precondition: "Job 028 is in needs-human after its fix rounds ran out.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: review asked for changes after N fix rounds`,
        assert: "Reason reads review asked for changes after N fix rounds",
        notes:
          "Run `rework` with a note, or `approve 028 --waive \"<reason>\"` if you disagree with the reviewer. A plain `approve` is refused (guide § 6.5, see D25).",
        related: [
          { id: "D24", rel: "depends" },
          { id: "D25", rel: "contrasts" },
          { id: "D16", rel: "depends" },
        ],
        source: "factory-user-guide.md § 9 When a job stops: what to do; factory-user-guide.md § 6.5 factory approve",
      },
      {
        id: "D5",
        title: "Reason: the change is too big to merge on its own",
        surface: "terminal",
        phase: "stuck",
        precondition:
          "Job 028 is in needs-human because its diff is over the line cap.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: diff has N changed lines, the cap is C`,
        assert: "Reason reads diff has N changed lines, the cap is C",
        notes:
          "Run `approve` after reading the change, or `rework` to split it up.",
        why: "A change above `diff_cap_lines` never auto-merges. The default cap is 4000 lines.",
        related: [
          { id: "D14", rel: "depends" },
          { id: "D16", rel: "depends" },
        ],
        source:
          "factory-user-guide.md § 9 When a job stops: what to do; factory-user-guide.md § 11 Settings that matter most",
      },
      {
        id: "D6",
        title: "Reason: the job used up its token budget",
        surface: "terminal",
        phase: "stuck",
        precondition: "Job 028 is in needs-human because it hit its token budget.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: budget is stopped`,
        assert: "Reason reads budget is stopped",
        notes:
          "Run `approve` or `cancel`. If the job is legitimately large, raise `budget_weighted` in `project.toml`.",
        why: "Each job has a token budget, 1,000,000 weighted tokens by default. The agent is stopped at the limit.",
        related: [
          { id: "D14", rel: "depends" },
          { id: "D17", rel: "depends" },
        ],
        source:
          "factory-user-guide.md § 9 When a job stops: what to do; factory-user-guide.md § 11 Settings that matter most",
      },
      {
        id: "D7",
        title: "Reason: the builder needs more information",
        surface: "terminal",
        phase: "stuck",
        precondition: "Job 028 is in needs-human because the builder is blocked.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: builder blocked/needs context ...`,
        assert: "Reason starts with builder blocked/needs context",
        notes:
          "Run `rework` and put the missing information in the note. `builder stopped: ...` means the builder ended early. The fix is also `rework`.",
        related: [{ id: "D16", rel: "depends" }],
        source: "factory-user-guide.md § 9 When a job stops: what to do",
      },
      {
        id: "D8",
        title: "Reason: the builder produced nothing",
        surface: "terminal",
        phase: "stuck",
        precondition: "Job 028 is in needs-human with nothing built.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: builder made no changes`,
        assert: "Reason reads builder made no changes or task N changed nothing",
        notes:
          "The reason can also read `task N changed nothing`. Run `rework`, or `cancel`.",
        related: [
          { id: "D16", rel: "depends" },
          { id: "D17", rel: "depends" },
        ],
        source: "factory-user-guide.md § 9 When a job stops: what to do",
      },
      {
        id: "D9",
        title: "Reason: no check covers the files that changed",
        surface: "terminal",
        phase: "stuck",
        precondition:
          "Job 028 is in needs-human because a changed code path matches no check rule.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: no check covers N changed code path(s)`,
        assert: "Reason reads no check covers N changed code path(s)",
        notes:
          "Run `approve` if you accept that, or `rework`.",
        related: [
          { id: "D14", rel: "depends" },
          { id: "D16", rel: "depends" },
        ],
        source: "factory-user-guide.md § 9 When a job stops: what to do",
      },
      {
        id: "D10",
        title: "Reason: your own checkout has uncommitted edits",
        surface: "terminal",
        phase: "stuck",
        flag: "trap",
        precondition:
          "Job 028 parked with `main busy: the main checkout has tracked changes`. Your repository is at ~/code/my-app.",
        command: `git -C ~/code/my-app stash
factory --project my-app approve 028
git -C ~/code/my-app stash pop`,
        expected: `merged <sha>`,
        assert: "approve prints merged <sha> once your checkout is clean",
        notes:
          "Untracked files are fine. Only edits to tracked files block the merge. Run `stash pop` last so your own edits come back.",
        why: "Your own checkout has uncommitted edits to tracked files, so the merge cannot happen. The job stays in needs-human, and a parked approve is safe to repeat.",
        verify: {
          command: `factory --project my-app status`,
          look: "Job 028 no longer shows a waiting block.",
        },
        after:
          "The job is merged into your local base branch. Nothing was pushed. Your stashed edits are back in your checkout after `stash pop`.",
        related: [{ id: "D14", rel: "depends" }],
        source:
          "factory-user-guide.md § 9 When a job stops: what to do; factory-user-guide.md § 10 Recipes (Retry after fixing your checkout); factory-user-guide.md § 6.5 factory approve",
      },
      {
        id: "D11",
        title: "Reason: your checkout is on another branch",
        surface: "terminal",
        phase: "stuck",
        precondition:
          "Job 028 parked with `main busy: the main checkout is not on <base>`. You switched ~/code/my-app back to its base branch.",
        command: `factory --project my-app approve 028`,
        expected: `merged <sha>`,
        assert: "approve prints merged <sha> after you switch to the base branch",
        notes:
          "The guide says to switch to the base branch, then run `approve` again. It gives no command for the switch.",
        why: "The merge needs your checkout on the base branch. A parked approve leaves the job in needs-human with the new reason, so it is safe to fix the cause and run it again.",
        after:
          "The job is merged into your local base branch. Nothing was pushed.",
        related: [{ id: "D10", rel: "contrasts" }],
        source:
          "factory-user-guide.md § 9 When a job stops: what to do; factory-user-guide.md § 6.5 factory approve",
      },
      {
        id: "D12",
        title: "Reason: the base branch moved and the change conflicts",
        surface: "terminal",
        phase: "stuck",
        precondition: "Job 028 is in needs-human after the base branch moved.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: merge conflict in <paths>`,
        assert: "Reason reads merge conflict in <paths>",
        notes:
          "The reason can also read `sync conflict in <paths>`. Look at the job's worktree, or `rework` with a note.",
        related: [{ id: "D16", rel: "depends" }],
        source: "factory-user-guide.md § 9 When a job stops: what to do",
      },
      {
        id: "D13",
        title: "Reason: the job has no plan",
        surface: "terminal",
        phase: "stuck",
        precondition:
          "Job 028 was created without plans and run before plan.md was added.",
        command: `factory --project my-app status`,
        expected: `028 needs a human: no plan.md in the job folder`,
        assert: "Reason reads no plan.md in the job folder",
        notes:
          "Put `plan.md` in the job folder, then run `factory --project my-app run 028` again. `factory new` prints the folder in `no plan yet: put plan.md at <job folder>/plan.md before running it`.",
        related: [{ id: "D16", rel: "contrasts" }],
        source:
          "factory-user-guide.md § 9 When a job stops: what to do; factory-user-guide.md § 6.1 factory new",
      },
      {
        id: "D14",
        title: "Approve a change you have read",
        surface: "terminal",
        phase: "stuck",
        precondition:
          "Job 028 is in needs-human or approve, and you have read the change.",
        command: `factory --project my-app approve 028`,
        expected: `merged <sha>`,
        assert: "Output is merged <sha>",
        notes:
          "A `parked: <reason>` result leaves the job in needs-human with the new reason. A cancelled job is refused with `job N was cancelled`. Use `rework` to revive it.",
        why: "The merge is the same as an automatic one: it merges the reviewed change with a merge commit into your base branch.",
        after:
          "The change is in your local base branch. Nothing was pushed, so run `git push` when you are ready. The branch and worktree stay until `factory cleanup 028`.",
        related: [{ id: "D20", rel: "depends" }],
        source:
          "factory-user-guide.md § 6.5 factory approve; factory-user-guide.md § 5 Your first feature, start to finish",
      },
      {
        id: "D15",
        title: "Merge a job whose code review never ran",
        surface: "terminal",
        phase: "stuck",
        precondition:
          "Job 028 is in needs-human and plain `approve` was refused with `job N has no code review yet ...`.",
        command: `factory --project my-app approve 028 --without-review`,
        expected: `merged <sha>`,
        assert: "Output is merged <sha> without a review",
        notes:
          "Without the flag, `approve` refuses with `job N has no code review yet ...`. The other way out is `rework`.",
        why: "`--without-review` merges a needs-human job even though the code review never ran.",
        after:
          "The change is in your local base branch with no review on record. Nothing was pushed.",
        related: [{ id: "D14", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.5 factory approve",
      },
      {
        id: "D16",
        title: "Send the builder a note with rework",
        surface: "terminal",
        phase: "stuck",
        precondition: "Job 028 is in needs-human.",
        command: `factory --project my-app rework 028 "Handle the empty list case and add a test for it"`,
        expected: `job N back to build, round <N>
job N back to build, same task`,
        assert: "rework prints job N back to build, with round <N> or same task",
        flag: "trap",
        notes:
          "The note must not be empty, must be one line and must be at most 200 characters. `rework` does not run the job, so run `factory --project my-app run 028` afterwards. The first line is printed for a job parked at a task boundary. The second line is printed for a job parked in the middle of a task.",
        why: "`rework` sends one line to the builder and sends the job back to the build stage. A job parked at a task boundary starts the next round: the round number goes up, the builder gets a fresh set of fix rounds, and it reads `changes-r<N>.md` with your note first and then the open review findings. Earlier review and builder files are kept. A job parked in the middle of a task keeps its round and reads your note from `rework-note.md`.",
        after:
          "The job is at stage build, and nothing runs until you run it. It also revives a cancelled job.",
        related: [{ id: "D17", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.6 factory rework",
      },
      {
        id: "D17",
        title: "Cancel a job and say why",
        surface: "terminal",
        phase: "stuck",
        precondition: "Job 028 is in needs-human.",
        command: `factory --project my-app cancel 028 --reason "superseded by job 031"`,
        expected: `(the guide does not give the output line; the reason is stored as: cancelled: superseded by job 031)`,
        assert: "No error, and the reason is stored as cancelled: <reason>",
        notes:
          "Without `--reason` the stored reason is `cancelled by user`. A merged job cannot be cancelled: `job N is already merged`.",
        why: "Cancelling does not delete anything. A cancelled job is stored as needs-human with a cancelled flag.",
        after:
          "The branch and worktree stay until you run `factory --project my-app cleanup 028`. `rework` revives the job.",
        related: [{ id: "D20", rel: "depends" }],
        source: "factory-user-guide.md § 6.7 factory cancel",
      },
      {
        id: "D18",
        title: "Cancel a job that still holds a live lease",
        surface: "terminal",
        phase: "stuck",
        flag: "trap",
        precondition: "Job 028 holds a live lease.",
        command: `factory --project my-app cancel 028`,
        expected: `job has a live lease, use --force to cancel anyway`,
        assert: "Without --force the cancel is refused with this message",
        notes:
          "The expected line is the refusal you get without `--force`. Go to D23 to force it. Printed as `factory: <message>` (guide § 6.0).",
        why: "A job with a live lease is refused unless you force it.",
        after: "Nothing was done. The job is unchanged.",
        related: [{ id: "D22", rel: "depends" }],
        source: "factory-user-guide.md § 6.7 factory cancel",
      },
      {
        id: "D23",
        title: "--force cancels a job whose owner is gone",
        surface: "terminal",
        phase: "stuck",
        flag: "trap",
        precondition: "The process that owned job 028 is gone.",
        command: `factory --project my-app cancel 028 --force`,
        expected: `(the guide does not give the output line; the stored reason is: cancelled: cancelled by user)`,
        assert: "The job is cancelled and its lease is cleared",
        notes:
          "Use `--force` only when the owning process is gone. It also clears the project lease held for this job.",
        related: [{ id: "D18", rel: "depends" }],
        source: "factory-user-guide.md § 6.7 factory cancel",
      },
      {
        id: "D19",
        title: "Cancel a draft by its name",
        surface: "terminal",
        phase: "stuck",
        precondition: "A draft named login-fix exists from `factory spec start`.",
        command: `factory --project my-app cancel login-fix`,
        expected: `(the guide does not give the output line; the draft card is removed)`,
        assert: "The draft card is gone and the spec file is still there",
        notes:
          "A draft name goes in place of the job id. A draft also ends when you run `factory new` with the same slug.",
        why: "With a draft name instead of a number, the draft card is removed and its spec file is kept.",
        after: "The spec file stays on disk. Only the draft card is removed.",
        source:
          "factory-user-guide.md § 6.7 factory cancel; factory-user-guide.md § 6.9 factory spec",
      },
      {
        id: "D20",
        title: "Remove the worktree and branch of a finished job",
        surface: "terminal",
        phase: "stuck",
        precondition: "Job 028 is merged or cancelled.",
        command: `factory --project my-app cleanup 028`,
        expected: `removed worktree and branch of job N`,
        assert: "Output is removed worktree and branch of job N",
        notes:
          "The other outputs are `removed leftover folder for job N; ...` and `nothing to remove for job N, ...`. `cleanup` only works on a merged or cancelled job.",
        why: "`cleanup` removes the git worktree and the job branch. A merged job keeps its card on the board.",
        after:
          "The job folder (ledger, logs, usage figures) always stays. A cancelled job also loses its card, unless it is in a series with other jobs, the project lease is held by it, or it holds a lease. The output then says `card kept on the board: <why>`.",
        related: [{ id: "D21", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.8 factory cleanup",
      },
      {
        id: "D21",
        title: "Throw away a cancelled job and its unmerged commits",
        surface: "terminal",
        phase: "stuck",
        flag: "trap",
        precondition:
          "Job 031 was cancelled (for example with `factory --project my-app cancel 031 --reason \"wrong approach\"`), and its work is unmerged and not wanted.",
        command: `factory --project my-app cleanup 031 --discard-commits`,
        expected: `removed worktree and branch of job N`,
        assert: "Branch removed even though it had unmerged commits",
        notes:
          "**`--discard-commits` deletes real work for good.** Use it only on jobs you are throwing away. Without it, a branch with commits that are not in the base branch is not removed. `--discard-commits` removes the branch even though it has commits that are not in the base branch.",
        why: "`--discard-commits` removes the branch even though it has commits that are not in the base branch.",
        after:
          "Nothing can bring the commits back. The job folder (ledger, logs, usage figures) stays.",
        related: [{ id: "D20", rel: "diverges" }],
        source:
          "factory-user-guide.md § 6.8 factory cleanup; factory-user-guide.md § 10 Recipes (Throw a job away completely)",
      },
      {
        id: "D22",
        title: "A busy project exits with code 75 and does nothing",
        surface: "terminal",
        phase: "stuck",
        precondition: "Another run holds the project lease.",
        command: `factory --project my-app run 028
echo $?`,
        expected: `project is busy: job N is running ...
75`,
        assert: "Message says project is busy and the exit code is 75",
        notes:
          "Wait for the other run. If the process is gone, run `factory cancel N --force`, or delete `project.lease` in the state folder. Printed as `factory: <message>` (guide § 6.0).",
        why: "A run takes the project lease, so two drivers never work on the same project at once.",
        after: "Nothing was done. The job is unchanged.",
        related: [{ id: "D18", rel: "contrasts" }],
        source:
          "factory-user-guide.md § 6.2 factory run; factory-user-guide.md § 13 Troubleshooting; factory-user-guide.md § 6.0 Global options",
      },
      {
        id: "D24",
        title: "Merge a job whose review asked for changes, with a reason",
        surface: "terminal",
        phase: "stuck",
        flag: "trap",
        precondition:
          "Job 028 is in needs-human or approve, and its last review asked for changes.",
        command: `factory --project my-app approve 028 --waive "known flake, fixing in a follow-up"`,
        expected: `merged <sha>`,
        assert: "Output is merged <sha> with the waiver on record",
        notes:
          "The reason is one line, not empty and at most 200 characters. `--waive` cannot be combined with `--without-review`. The dashboard asks for the reason in a dialog when you press Approve on such a job.",
        why: "`--waive` writes a ledger line, an `operator.approve` event and an entry in `waivers.json` in the job folder. After the merge it files one ticket for each waived blocker, whether or not `tickets_on_merge` is set. A ticket that cannot be filed never changes the merge result, and `factory tickets 028` retries it.",
        after:
          "The change is in your local base branch. Nothing was pushed. `waivers.json` is in the job folder.",
        related: [
          { id: "D25", rel: "depends" },
          { id: "D14", rel: "contrasts" },
        ],
        source: "factory-user-guide.md § 6.5 factory approve",
      },
      {
        id: "D25",
        title: "A plain approve is refused while review blockers are open",
        surface: "terminal",
        phase: "stuck",
        flag: "trap",
        precondition:
          "Job 028 is in needs-human and its last review asked for changes.",
        command: `factory --project my-app approve 028`,
        expected: `job N has B open review blockers from round R (FILE): use rework, cancel, or approve --waive "<reason>" to merge anyway`,
        assert: "Refused with a message naming the open review blockers",
        notes:
          "The command exits with code 1. Use `rework`, `cancel`, or `approve --waive` (D24). Printed as `factory: <message>` (guide § 6.0).",
        why: "A plain approve of a job whose last review asked for changes is refused. `--waive` is the way to merge it anyway.",
        after: "Nothing was done. The job is unchanged.",
        related: [
          { id: "D24", rel: "contrasts" },
          { id: "D16", rel: "contrasts" },
        ],
        source: "factory-user-guide.md § 6.5 factory approve",
      },
      {
        id: "D26",
        title: "--waive needs a review that asked for changes",
        surface: "terminal",
        phase: "stuck",
        precondition:
          "Job 028 is in needs-human, and its last review approved or it has no review yet.",
        command: `factory --project my-app approve 028 --waive "known flake, fixing in a follow-up"`,
        expected: `job N has no open review findings to waive`,
        assert: "Refused with job N has no open review findings to waive",
        notes:
          "The command exits with code 1. Drop `--waive`, and add `--without-review` when no review ran. Printed as `factory: <message>` (guide § 6.0).",
        why: "`--waive` needs a last review that asked for changes.",
        after: "Nothing was done. The job is unchanged.",
        related: [
          { id: "D24", rel: "contrasts" },
          { id: "D15", rel: "contrasts" },
        ],
        source: "factory-user-guide.md § 6.5 factory approve",
      },
    ],
  },
  {
    key: "E",
    title: "Look back at what happened",
    actor: "You",
    premise:
      "Most of these commands only read. Three change something: `costs` writes `costs.json`, and `tickets` and `kb-refresh` act on a merged job.",
    context: [
      "Start with `factory status` for a readable view of the board, and `factory show` when you need the raw record of one job. For the story of a run, `factory log` prints the project's structured log, oldest first, one JSON object per line.",
      "Cost is counted in weighted tokens. Cache reads count one tenth, and each job has a budget, 1,000,000 by default. `factory tokens` shows one job, and `factory costs` shows every job.",
      "The last two steps act on a merged job. `factory tickets` files follow-up GitHub issues, and `factory kb-refresh` refreshes the knowledge base. Neither one changes the merge result.",
    ],
    cases: [
      {
        id: "E1",
        title: "Read the columns of the status table",
        surface: "terminal",
        phase: "observe",
        precondition: "Project my-app has at least one job.",
        command: `factory --project my-app status`,
        expected: `project: free
(a table with the columns ID, STAGE, ROUND, REVIEWS, TOKENS, SERIES, SLUG, one row per job)`,
        assert: "The table has seven columns, ID to SLUG",
        notes:
          "`REVIEWS` uses the words pending, running, pass, changes and skipped, for example `code:pass`. `TOKENS` looks like `12.3K (4%)`. `SERIES` is `index/total`, or `-` for a single job.",
        why: "`status` never changes anything. It prints the lease line, then the table, or the text `no jobs`.",
        source: "factory-user-guide.md § 6.3 factory status",
      },
      {
        id: "E2",
        title: "Print the full record of one job",
        surface: "terminal",
        phase: "observe",
        precondition: "Job 028 exists.",
        command: `factory show 028`,
        expected: `(raw JSON with the fields: id, slug, stage, round, reason, reviews, series, branch, worktree, tokens, estimate and more)`,
        assert: "Raw JSON, not a table",
        notes:
          "This is meant for debugging. Use `status` for a readable view.",
        source: "factory-user-guide.md § 6.4 factory show",
      },
      {
        id: "E3",
        title: "Show only one kind of log event",
        surface: "terminal",
        phase: "observe",
        precondition: "The project has logged at least one agent run.",
        command: `factory --project my-app log --event agent.end`,
        expected: `(one JSON object per line, each with ts, level, scope, project, event, msg and event-specific fields)`,
        assert: "Every line has the event agent.end",
        notes:
          "If nothing has been logged yet, the output is `no log yet`. Secrets are redacted.",
        why: "`log` only reads. It prints the last 100 lines by default.",
        related: [{ id: "E6", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.12 factory log",
      },
      {
        id: "E4",
        title: "Combine a job, a level and a tail",
        surface: "terminal",
        phase: "observe",
        precondition: "Job 028 has logged warnings.",
        command: `factory --project my-app log --job 028 --level warn --tail 50`,
        expected: `(JSON lines of job 028 at level warn or above, at most 50 of them)`,
        assert: "Only job 028, warn or above, at most 50 lines",
        notes:
          "`--level` means this level or above: error, warn, info, debug, trace. The default level is `info`. For debug lines set `log_level` in `project.toml` or `FACTORY_LOG=debug`.",
        why: "All filters combine (AND). The tail is applied last.",
        source:
          "factory-user-guide.md § 6.12 factory log; factory-user-guide.md § 10 Recipes (Understand why a job stopped)",
      },
      {
        id: "E5",
        title: "Filter by job with the full id",
        surface: "terminal",
        phase: "observe",
        flag: "trap",
        precondition: "Job 028 has logged lines.",
        command: `factory --project my-app log --job 028`,
        expected: `(JSON lines of job 028)`,
        assert: "Lines of job 028 appear when the id is written as 028",
        notes:
          "Use the full id, for example `028`, not `28`. The guide gives this rule for `factory log --job`.",
        why: "The guide states that `--job` needs the full id. Everywhere else `1`, `01` and `001` all mean job `001`.",
        related: [{ id: "E4", rel: "diverges" }],
        source:
          "factory-user-guide.md § 6.12 factory log; factory-user-guide.md § 2 The ideas you need (glossary)",
      },
      {
        id: "E6",
        title: "Match a family of events and keep every line",
        surface: "terminal",
        phase: "observe",
        precondition: "The project has run at least one task.",
        command: `factory --project my-app log --event task --tail 0`,
        expected: `(all lines whose event is task or starts with task., for example task.start)`,
        assert: "Event task matches task.start, and --tail 0 keeps all lines",
        notes: "`--tail 0` means all lines. The default is 100.",
        why: "`--event` matches the name itself, or any name that starts with it plus a dot.",
        related: [{ id: "E3", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.12 factory log",
      },
      {
        id: "E7",
        title: "See the token use of one job",
        surface: "terminal",
        phase: "observe",
        precondition: "Job 028 has run at least once.",
        command: `factory --project my-app tokens 028`,
        expected: `totals: input, cache_creation, output, cache_read and weighted <n> (<pct>% of <budget>)
by role: builder, reviewer, committer, message and kb, each with runs, raw and weighted
runs: one row per agent run, with the role, provider, round, turns and weighted tokens`,
        assert: "Totals, a by-role block and one row per agent run",
        notes:
          "The line comparing against a job 001 baseline uses a fixed figure from another project. A beginner can ignore it.",
        why: "`weighted = input + cache_creation + output + cache_read / 10`.",
        source: "factory-user-guide.md § 6.10 factory tokens",
      },
      {
        id: "E8",
        title: "See prompt-cache use for the whole project",
        surface: "terminal",
        phase: "observe",
        precondition: "The project has run at least one job.",
        command: `factory --project my-app tokens --cache --days 7`,
        expected: `(per-role cache use, then a list of resumed runs of the last 7 days where the cache had expired)`,
        assert: "Per-role cache use and a list of resumed runs",
        notes:
          "`--cache` cannot be combined with a job id. `--days` needs `--cache`, and N is at least 1. You can skip this at first.",
        why: "It is useful for tuning cost.",
        source: "factory-user-guide.md § 6.10 factory tokens",
      },
      {
        id: "E9",
        title: "Compare the cost of every job",
        surface: "terminal",
        phase: "observe",
        precondition: "The project has at least one job.",
        command: `factory --project my-app costs`,
        expected: `(a table with the columns JOB, STAGE, GROUP, TASKS, LINES, FILES, WEIGHTED, RAW, RUNS, one row per job, then statistics per size group)`,
        assert: "A job table with nine columns, then group statistics",
        notes:
          "`GROUP` is `small` under 12 KB, `medium` up to 25 KB and `complex` above that. `-` means no plan. `~` marks an approximate figure and `no data` means no runs yet.",
        why: "The group table shows the median and range for merged jobs. The same history is behind the estimate printed by `factory new`.",
        after:
          "It writes `costs.json` into the project state folder. Nothing else changes.",
        source: "factory-user-guide.md § 6.11 factory costs",
      },
      {
        id: "E10",
        title: "Preview the follow-up tickets of a merged job",
        surface: "terminal",
        phase: "observe",
        precondition:
          "Job 028 is merged, `gh` is installed and logged in, and the feature text names an issue such as #76.",
        command: `factory --project my-app tickets 028 --dry-run`,
        expected: `(a printout of what would happen; nothing is changed)`,
        assert: "A preview prints and no issue is created",
        notes:
          "It skips, with exit code 0, when the job is not merged, is not the last plan of a series, or the feature text has no issue references. That rule is written for the spec follow-ups and the parent issues. `factory tickets` also files one ticket for each waived blocker (guide § 6.5) and one ticket titled `Minor review findings from job <id>: <slug>`. The minor-findings ticket runs for every plan of a series.",
        why: "`--dry-run` prints what would happen and changes nothing.",
        related: [{ id: "E11", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.13 factory tickets; factory-user-guide.md § 6.5 factory approve",
      },
      {
        id: "E11",
        title: "File the follow-up tickets for real",
        surface: "terminal",
        phase: "merge",
        precondition:
          "The dry run in E10 showed what you expect. Run this against a scratch repository first.",
        command: `factory --project my-app tickets 028`,
        expected: `(the follow-ups listed in the spec are filed as new GitHub issues; the parent issues named in the feature text are commented on and closed; one ticket is filed for each waived blocker; open [minor] findings of the last review become one ticket titled Minor review findings from job <id>: <slug>)`,
        assert: "New GitHub issues exist for the spec follow-ups",
        notes:
          "It is safe to run twice. Set `tickets_on_merge = true` in the project settings to run it after every merge. The default is off. The waived-blocker tickets and the minor-findings ticket are filed after a merge whether or not `tickets_on_merge` is set, and `minor_tickets = false` turns the minor-findings ticket off.",
        why: "A marker file in the job folder stops any call from repeating, and `minors.json` in the job folder records the minor-findings ticket number so a re-run files nothing twice. It never changes the merge result.",
        after:
          "New issues exist on GitHub, and the parent issues are commented on and closed. A marker file is left in the job folder, and `minors.json` records the minor-findings ticket.",
        related: [{ id: "E10", rel: "depends" }],
        source: "factory-user-guide.md § 6.13 factory tickets; factory-user-guide.md § 6.5 factory approve",
      },
      {
        id: "E12",
        title: "Refresh the knowledge base for a merged job",
        surface: "terminal",
        phase: "merge",
        precondition:
          "Job 028 is merged, `kb_path` is set in the project settings and the project is not busy.",
        command: `factory --project my-app kb-refresh --job 028`,
        expected: `(the guide does not give the output line; the result goes to the ledger of job 028)`,
        assert: "The refresh result is in the ledger of job 028",
        notes:
          "`--job` is required. It runs the refresh now and ignores the stop marker.",
        after: "The result is written to the ledger of job 028.",
        source: "factory-user-guide.md § 6.14 factory kb-refresh",
      },
      {
        id: "E13",
        title: "Find out why a job stopped",
        surface: "terminal",
        phase: "observe",
        precondition: "Job 028 stopped in needs-human.",
        command: `factory --project my-app log --job 028 --event park`,
        expected: `(JSON lines of job 028 whose event is park, with fields such as job_id, stage and reason)`,
        assert: "The park lines of job 028 show the reason",
        notes:
          "Fields on each line include `ts`, `level`, `scope`, `project`, `event`, `msg`, and event-specific fields such as `job_id`, `stage`, `reason`, `sha` or `duration_ms`.",
        related: [{ id: "D1", rel: "contrasts" }],
        source: "factory-user-guide.md § 6.12 factory log",
      },
    ],
  },
]
