"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const board = require("../static/board.js");

function makeJob(overrides) {
  return Object.assign(
    {
      id: "001",
      slug: "fix-import",
      created: "2026-10-13",
      feature: "Fix the import bug",
      branch: "factory/001-fix-import",
      stage: "build",
      round: 0,
      reason: null,
      stage_since: 100,
      seconds_in_stage: 75,
      reviews: { security: "pending", ux: "pending", ui: "pending", code: "pending" },
      agents: [],
    },
    overrides,
  );
}

function makeState(jobs, extra) {
  return Object.assign(
    { generated_at: 200, board_error: null, jobs, unassigned_agents: [] },
    extra,
  );
}

test("escapeHtml escapes the five dangerous characters and tolerates null", () => {
  assert.equal(board.escapeHtml(`<a href="x">'&'</a>`), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
  assert.equal(board.escapeHtml(null), "");
  assert.equal(board.escapeHtml(undefined), "");
});

test("formatAge", () => {
  assert.equal(board.formatAge(-5), "0s");
  assert.equal(board.formatAge(Number.NaN), "0s");
  assert.equal(board.formatAge(12.9), "12s");
  assert.equal(board.formatAge(180), "3m");
  assert.equal(board.formatAge(3900), "1h 05m");
  assert.equal(board.formatAge(2 * 86400 + 5), "2d");
});

test("clock gives hh:mm:ss", () => {
  assert.match(board.clock(1760000000), /^\d\d:\d\d:\d\d$/);
});

test("reviewChip shows role and state as text and whitelists the class", () => {
  const chip = board.reviewChip("security", "pass");
  assert.match(chip, /chip-pass/);
  assert.match(chip, />security pass</);
  const odd = board.reviewChip("code", '"><script>');
  assert.match(odd, /chip-pending/);
  assert.doesNotMatch(odd, /<script>/);
});

test("agentLine shows the live tool for a running agent", () => {
  const html = board.agentLine({
    agent_type: "factory-builder",
    status: "running",
    last_tool: "Edit",
    last_target: "src/App.tsx",
    tool_count: 14,
  });
  assert.match(html, /agent-running/);
  assert.match(html, />builder</);
  assert.match(html, /Edit src\/App\.tsx/);
  assert.match(html, /14 tools/);
  assert.match(html, />running</);
});

test("agentLine uses the singular for one tool call", () => {
  const html = board.agentLine({ agent_type: "factory-builder", status: "running", tool_count: 1 });
  assert.match(html, />1 tool</);
});

test("agentLine shows the last message for a finished agent and survives odd status", () => {
  const done = board.agentLine({ agent_type: "factory-ui-reviewer", status: "done", last_message: "VERDICT: APPROVE" });
  assert.match(done, /agent-done/);
  assert.match(done, /VERDICT: APPROVE/);
  const odd = board.agentLine({ agent_type: "x", status: '"><script>' });
  assert.match(odd, /agent-done/);
  assert.doesNotMatch(odd, /<script>/);
});

test("renderCard escapes the feature text", () => {
  const html = board.renderCard(makeJob({ feature: "<img src=x onerror=alert(1)>" }));
  assert.doesNotMatch(html, /<img/);
  assert.match(html, /&lt;img src=x onerror=alert\(1\)&gt;/);
});

test("renderCard shows id, slug, date, branch, round and age", () => {
  const html = board.renderCard(makeJob({ round: 1 }));
  assert.match(html, /001/);
  assert.match(html, /fix-import/);
  assert.match(html, /2026-10-13/);
  assert.match(html, /factory\/001-fix-import/);
  assert.match(html, /round 1\/2/);
  assert.match(html, /1m in stage/);
});

test("renderCard shows review chips only from the review stage on", () => {
  assert.doesNotMatch(board.renderCard(makeJob({ stage: "build" })), /class="chip /);
  const review = board.renderCard(makeJob({ stage: "review", reviews: { security: "pass", ux: "skipped", ui: "skipped", code: "changes" } }));
  assert.match(review, /security pass/);
  assert.match(review, /ux skipped/);
  assert.match(review, /code changes/);
});

test("renderCard for needs-human shows the reason and the commands for that job", () => {
  const html = board.renderCard(makeJob({ id: "003", stage: "needs-human", reason: "touches the <trust gate>" }));
  assert.match(html, /touches the &lt;trust gate&gt;/);
  assert.match(html, /factory approve 003/);
  assert.match(html, /factory rework 003 &lt;note&gt;/);
});

test("renderCard copes with missing optional fields", () => {
  const html = board.renderCard({ id: "009", stage: "spec" });
  assert.match(html, /009/);
});

test("renderBoard has the six stage columns in order with counts", () => {
  const state = makeState([makeJob({ stage: "build" }), makeJob({ id: "002", stage: "build" }), makeJob({ id: "003", stage: "spec" })]);
  const html = board.renderBoard(state);
  const titles = [...html.matchAll(/<h2>([A-Za-z ]+)</g)].map((m) => m[1]);
  assert.deepEqual(titles, ["Spec", "Build", "Review", "Approve", "Merged", "Needs human"]);
  assert.match(html, /column-build[\s\S]*?<span class="count">2</);
  assert.match(html, /class="empty"/);
});

test("renderBoard shows only the 10 newest merged jobs", () => {
  const jobs = [];
  for (let i = 1; i <= 12; i += 1) {
    jobs.push(makeJob({ id: String(i).padStart(3, "0"), stage: "merged", stage_since: i }));
  }
  const html = board.renderBoard(makeState(jobs));
  assert.equal((html.match(/<article class="card/g) || []).length, 10);
  assert.match(html, /data-job="012"/);
  assert.doesNotMatch(html, /data-job="001"/);
  assert.match(html, /<span class="count">12</);
});

test("renderBoard reports jobs with an unknown stage", () => {
  const html = board.renderBoard(makeState([makeJob({ stage: "teleporting" })]));
  assert.match(html, /1 job with an unknown stage/);
});

test("renderUnassigned lists agents without a job and nothing otherwise", () => {
  assert.equal(board.renderUnassigned(makeState([])), "");
  const state = makeState([], { unassigned_agents: [{ agent_type: "factory-approver", status: "running" }] });
  assert.match(board.renderUnassigned(state), /approver/);
});

test("the Recent activity feed is gone", () => {
  assert.equal(board.renderFeed, undefined);
  const html = board.renderBoard(makeState([makeJob({ stage: "build" })])) + board.renderUnassigned(makeState([]));
  assert.doesNotMatch(html, /Recent activity|class="feed"/);
});

test("every stage and the other agents are card panels", () => {
  const state = makeState([makeJob({ stage: "build" })], {
    unassigned_agents: [{ agent_type: "factory-approver", status: "running" }],
  });
  const boardHtml = board.renderBoard(state);
  assert.equal((boardHtml.match(/<section class="column panel /g) || []).length, 6);
  assert.match(board.renderUnassigned(state), /^<section class="panel">[\s\S]*<\/section>$/);
});

test("renderSummary asks for the user first when a job needs them", () => {
  const html = board.renderSummary(
    makeState([
      makeJob({ id: "003", stage: "needs-human", reason: "touches the <trust gate>", feature: "Rename <b>panel</b>" }),
      makeJob({ id: "004", stage: "build" }),
    ]),
  );
  assert.match(html, /summary-attention/);
  assert.match(html, />1 job needs you</);
  assert.match(html, /1 more job is still running\./);
  assert.match(html, /touches the &lt;trust gate&gt;/);
  assert.match(html, /Rename &lt;b&gt;panel&lt;\/b&gt;/);
  assert.doesNotMatch(html, /<b>/);
  assert.match(html, /factory approve 003/);
  assert.match(html, /factory rework 003 &lt;note&gt;/);
});

test("renderSummary uses the plural forms and lists at most three jobs", () => {
  const jobs = [1, 2, 3, 4].map((n) => makeJob({ id: `00${n}`, stage: "needs-human" }));
  const html = board.renderSummary(makeState(jobs));
  assert.match(html, />4 jobs need you</);
  assert.match(html, /Nothing else is running\./);
  assert.equal((html.match(/<li>/g) || []).length, 3);
});

test("renderSummary says nothing needs the user while jobs run", () => {
  const running = makeJob({ stage: "build", agents: [{ status: "running" }, { status: "done" }] });
  const html = board.renderSummary(makeState([running, makeJob({ id: "002", stage: "review" })]));
  assert.match(html, /summary-running/);
  assert.match(html, />2 jobs are in progress</);
  assert.match(html, /1 agent is working\. Nothing needs you right now\./);
  const quiet = board.renderSummary(makeState([makeJob({ stage: "approve" })]));
  assert.match(quiet, />1 job is in progress</);
  assert.match(quiet, /No agent is active at the moment/);
});

test("renderSummary does not count waiting plans as running", () => {
  const parked = [2, 3].map((n) => makeJob({ id: `00${n}`, stage: "spec" }));
  const needs = board.renderSummary(makeState([makeJob({ stage: "needs-human" }), ...parked]));
  assert.match(needs, /2 plans are waiting\./);
  assert.doesNotMatch(needs, /still running/);
  assert.doesNotMatch(needs, /Nothing else is running/);
  const building = board.renderSummary(makeState([makeJob({ stage: "build" }), ...parked]));
  assert.match(building, />1 job is in progress</);
  assert.match(building, /2 plans are waiting\./);
  const idle = board.renderSummary(makeState([makeJob({ id: "001", stage: "merged" }), ...parked]));
  assert.match(idle, /summary-idle/);
  assert.match(idle, />2 plans are waiting</);
  assert.match(idle, /factory run &lt;id&gt;/);
  assert.doesNotMatch(idle, /All caught up/);
});

test("a cancelled job in the needs-you list offers only cleanup", () => {
  const html = board.renderSummary(makeState([makeJob({ stage: "needs-human", cancelled: true })]));
  assert.match(html, /factory cleanup 001/);
  assert.doesNotMatch(html, /factory approve/);
  assert.doesNotMatch(html, /factory rework/);
});

test("the merged column keeps only the last spec cards but counts all", () => {
  const specs = Array.from({ length: 13 }, (_, n) => makeSpec({ series_id: `S${n + 1}`, stage: "merged" }));
  const html = board.renderBoard(makeState([], { specs }));
  assert.match(html, /column-merged"><h2>Merged<span class="sub">[^<]*<\/span><span class="count">13<\/span>/);
  assert.equal((html.match(/data-spec="/g) || []).length, 10);
  assert.match(html, /data-spec="S13"/);
  assert.doesNotMatch(html, /data-spec="S3"/);
  board.taskChoice.clear();
});

test("renderSummary covers an empty board and a finished one", () => {
  const empty = board.renderSummary(makeState([]));
  assert.match(empty, /summary-idle/);
  assert.match(empty, />No jobs yet</);
  assert.match(empty, /\/factory &lt;feature&gt;/);
  const done = board.renderSummary(makeState([makeJob({ stage: "merged" }), makeJob({ id: "002", stage: "merged" })]));
  assert.match(done, />All caught up</);
  assert.match(done, /2 jobs merged\./);
});

test("renderSummary ignores a job with an unknown stage when counting running jobs", () => {
  const html = board.renderSummary(makeState([makeJob({ stage: "teleporting" })]));
  assert.doesNotMatch(html, /in progress/);
});

test("headerStatus", () => {
  assert.equal(board.headerStatus(makeState([]), null, 200).level, "ok");
  assert.match(board.headerStatus(makeState([]), null, 200).text, /^Live, updated \d\d:\d\d:\d\d$/);
  assert.equal(board.headerStatus(null, "HTTP 500", null).text, "Server unreachable");
  assert.match(board.headerStatus(null, "boom", 150).text, /^Server unreachable, last update \d\d:\d\d:\d\d$/);
  assert.equal(board.headerStatus(null, "boom", 150).level, "error");
  const warn = board.headerStatus(makeState([], { board_error: "board.json is invalid: x" }), null, 200);
  assert.equal(warn.level, "warn");
  assert.equal(warn.text, "board.json is invalid: x");
  assert.equal(board.headerStatus(null, null, null).level, "warn");
  assert.equal(board.headerStatus(null, null, null).text, "Connecting");
});

test("renderCard shows a segmented progress bar under the title", () => {
  const plan = {
    title: "Prove mTLS",
    goal: "Close the issue with evidence.",
    tasks: [
      { number: 1, title: "First", status: "done" },
      { number: 2, title: "Second", status: "active" },
      { number: 3, title: "Third", status: "bogus" },
    ],
  };
  const html = board.renderCard(makeJob({ plan }));
  assert.match(html, /1\/3 tasks done/);
  assert.equal((html.match(/class="seg seg-/g) || []).length, 3);
  assert.match(html, /seg-pending/);
  assert.ok(html.indexOf("class=\"feature\"") < html.indexOf("class=\"progress\""));
  assert.match(html, /Task 2<\/span> Second/);
  assert.match(html, /Close the issue with evidence\./);
});

test("renderCard without a plan says the plan is being written in the spec stage", () => {
  assert.match(board.renderCard(makeJob({ stage: "spec" })), /Plan is being written/);
  assert.match(board.renderCard(makeJob({ stage: "build" })), /No task list found/);
});

test("renderCard escapes plan text and keeps running agents open", () => {
  const plan = { title: "<b>x</b>", goal: null, tasks: [{ number: 1, title: "<i>t</i>", status: "pending" }] };
  const agents = [
    { agent_id: "a", agent_type: "factory-builder", status: "running", started_ts: 1, tool_count: 1 },
    { agent_id: "b", agent_type: "agent", status: "done", started_ts: 1, ended_ts: 2, tool_count: 0 },
  ];
  const html = board.renderCard(makeJob({ plan, agents }));
  assert.doesNotMatch(html, /<b>x<\/b>|<i>t<\/i>/);
  assert.match(html, /<details class="agents-box"[^>]* open>/);
  assert.match(html, /1 working, 1 finished/);
});

const TASKS_PLAN = {
  title: "Plan one",
  goal: "Do the thing.",
  tasks: [
    { number: 1, title: "First", status: "done" },
    { number: 2, title: "Second", status: "active" },
  ],
};

test("the plan task list is a collapsible section with a done count in its summary", () => {
  const html = board.renderCard(makeJob({ plan: TASKS_PLAN }));
  assert.match(html, /<details class="tasks-box" data-tasks="001" open>/);
  assert.match(html, /<summary>Tasks <span class="agents-sum">1\/2 done<\/span><\/summary>/);
  // The title and goal stay outside the collapsible part, the list is inside it.
  assert.ok(html.indexOf('class="plan-title"') < html.indexOf('class="tasks-box"'));
  assert.ok(html.indexOf('class="tasks-box"') < html.indexOf('<ol class="tasks">'));
  assert.match(html, /Task 2<\/span> Second/);
});

test("the task list is open for a running job, closed once merged, and the user's choice wins", () => {
  board.taskChoice.clear();
  assert.match(board.renderCard(makeJob({ stage: "build", plan: TASKS_PLAN })), /class="tasks-box"[^>]* open>/);
  assert.doesNotMatch(board.renderCard(makeJob({ stage: "merged", plan: TASKS_PLAN })), /class="tasks-box"[^>]* open>/);
  board.taskChoice.set("001", false);
  assert.doesNotMatch(board.renderCard(makeJob({ stage: "build", plan: TASKS_PLAN })), /class="tasks-box"[^>]* open>/);
  board.taskChoice.set("001", true);
  assert.match(board.renderCard(makeJob({ stage: "merged", plan: TASKS_PLAN })), /class="tasks-box"[^>]* open>/);
  board.taskChoice.clear();
});

test("a plan with a title but no tasks has no empty collapsible section", () => {
  const html = board.renderCard(makeJob({ plan: { title: "Only a title", goal: null, tasks: [] } }));
  assert.doesNotMatch(html, /tasks-box/);
});

function chipsOf(html) {
  const found = html.match(/<div class="chips">([\s\S]*?)<\/div>/);
  return found ? found[1] : "";
}

test("review chips show only the roles the job has, in the usual order", () => {
  const only = chipsOf(board.renderCard(makeJob({ stage: "review", reviews: { code: "pass" } })));
  assert.match(only, /code/);
  assert.doesNotMatch(only, /security|\bux\b|\bui\b/);
  // A v1 job still shows all four, and a map in another key order is shown in the usual order.
  const four = chipsOf(
    board.renderCard(makeJob({ stage: "review", reviews: { code: "pass", ui: "skipped", security: "pass", ux: "skipped" } })),
  );
  assert.ok(four.indexOf("security") < four.indexOf("ux"));
  assert.ok(four.indexOf("ux") < four.indexOf("ui"));
  assert.ok(four.indexOf("ui") < four.indexOf("code"));
  // A job with no review state yet still shows the one review that will run.
  const none = chipsOf(board.renderCard(makeJob({ stage: "review", reviews: {} })));
  assert.match(none, /code/);
  assert.doesNotMatch(none, /security/);
});

test("a long agent message that was cut mid-word starts at a whole word with an ellipsis", () => {
  const cut = "g was committed. " + "word ".repeat(60); // exactly the length the driver keeps, so a cut is likely
  const text = cut.slice(0, 300);
  const agent = { agent_id: "a", agent_type: "builder", status: "done", started_ts: 1, ended_ts: 2, tool_count: 1, last_message: text };
  const html = board.renderCard(makeJob({ agents: [agent] }));
  assert.match(html, /… was committed\./);
  assert.doesNotMatch(html, /[>\s]g was committed/);
  const short = { ...agent, last_message: "Done and committed." };
  assert.match(board.renderCard(makeJob({ agents: [short] })), /Done and committed\./);
  assert.doesNotMatch(board.renderCard(makeJob({ agents: [short] })), /… Done/);
});

test("backticks in plan text become code, and the text inside stays escaped", () => {
  const plan = {
    title: "Use `criterion` here",
    goal: null,
    tasks: [{ number: 1, title: "criterion and the `resolve` bench", status: "pending" }, { number: 2, title: "a `<b>x</b>` b", status: "pending" }],
  };
  const html = board.renderCard(makeJob({ plan }));
  assert.match(html, /criterion and the <code>resolve<\/code> bench/);
  assert.match(html, /Use <code>criterion<\/code> here/);
  assert.match(html, /<code>&lt;b&gt;x&lt;\/b&gt;<\/code>/);
  assert.doesNotMatch(html, /`/);
});

test("the column subtitles describe the v3 flow, not the v1 agents", () => {
  const subs = board.STAGES.map((stage) => stage.sub).join(" | ");
  assert.doesNotMatch(subs, /Spec writer|Four reviewers|Approver/);
  assert.match(board.STAGES.find((stage) => stage.key === "review").sub, /check/i);
});

test("renderCard hides time in stage for merged and needs-human cards", () => {
  assert.match(board.renderCard(makeJob({ stage: "build" })), /in stage/);
  assert.doesNotMatch(board.renderCard(makeJob({ stage: "merged" })), /in stage/);
  assert.doesNotMatch(board.renderCard(makeJob({ stage: "needs-human", reason: "x" })), /in stage/);
});

test("durationsLine shows per-stage times and marks the current one", () => {
  const html = board.durationsLine(
    makeJob({
      stage_durations: [
        { stage: "spec", seconds: 120, current: false },
        { stage: "build", seconds: 1860, current: true },
      ],
    }),
  );
  assert.match(html, /Spec 2m/);
  assert.match(html, /Build 31m/);
  assert.match(html, /duration-now/);
});

test("durationsLine ignores bad entries and shows nothing without data", () => {
  assert.equal(board.durationsLine(makeJob({})), "");
  const html = board.durationsLine(
    makeJob({
      stage_durations: [
        { stage: "<script>", seconds: 5 },
        { stage: "build", seconds: Number.NaN },
        { stage: "review", seconds: 60 },
      ],
    }),
  );
  assert.doesNotMatch(html, /script|NaN/);
  assert.match(html, /Review 1m/);
});

test("seriesChip shows plan k of N and hides for a single plan or bad numbers", () => {
  assert.match(board.seriesChip({ index: 2, total: 4 }), />plan 2 of 4</);
  assert.equal(board.seriesChip({ index: 1, total: 1 }), "");
  assert.equal(board.seriesChip(null), "");
  assert.equal(board.seriesChip({ index: "x", total: "y" }), "");
  assert.match(board.seriesChip({ index: 9, total: 4 }), />plan 4 of 4</);
  assert.match(board.seriesChip({ index: 0, total: 4 }), />plan 1 of 4</);
});

test("renderCard puts the series chip in the card head", () => {
  assert.match(board.renderCard(makeJob({ series: { index: 2, total: 3 } })), /plan 2 of 3/);
  assert.doesNotMatch(board.renderCard(makeJob({})), /class="series"/);
});

test("tokensBar shows weighted tokens, a bounded bar and a warning level", () => {
  assert.equal(board.tokensBar(makeJob({})), "");
  const half = board.tokensBar(makeJob({ token_view: { weighted: 1500000, raw: 9000000, budget: 3000000, fraction: 0.5 } }));
  assert.match(half, /1\.5M of 3M/);
  assert.match(half, /data-pct="50"/);
  assert.match(half, /tokens-ok/);
  const high = board.tokensBar(makeJob({ token_view: { weighted: 2500000, raw: 0, budget: 3000000, fraction: 0.83 } }));
  assert.match(high, /tokens-warn/);
  const over = board.tokensBar(makeJob({ token_view: { weighted: 9, raw: 0, budget: 3, fraction: 7 } }));
  assert.match(over, /data-pct="100"/);
  assert.match(over, /tokens-stop/);
  const bad = board.tokensBar(makeJob({ token_view: { weighted: "x", raw: 0, budget: 0, fraction: Number.NaN } }));
  assert.doesNotMatch(bad, /NaN|undefined/);
});

test("copyChip is a button that carries the escaped command", () => {
  const html = board.copyChip('factory rework 003 <note> "x"');
  assert.match(html, /^<button type="button" class="cmd" data-copy="/);
  assert.match(html, /data-copy="factory rework 003 &lt;note&gt; &quot;x&quot;"/);
  assert.doesNotMatch(html, /<note>/);
});

test("needs-human cards and the summary use copy chips for approve, rework and cancel", () => {
  const card = board.renderCard(makeJob({ id: "003", stage: "needs-human", reason: "r" }));
  assert.match(card, /data-copy="factory approve 003"/);
  assert.match(card, /data-copy="factory rework 003 &lt;note&gt;"/);
  assert.match(card, /data-copy="factory cancel 003"/);
  const summary = board.renderSummary(makeState([makeJob({ id: "004", stage: "needs-human", reason: "r" })]));
  assert.match(summary, /data-copy="factory approve 004"/);
});

test("renderLogNotice reports skipped log lines only when there are some", () => {
  assert.equal(board.renderLogNotice(makeState([])), "");
  assert.match(board.renderLogNotice(makeState([], { skipped_lines: 3 })), /3 event log lines skipped/);
  assert.match(board.renderLogNotice(makeState([], { skipped_lines: 1 })), /1 event log line skipped/);
  assert.match(board.renderLogNotice(makeState([], { skipped_lines: 2 })), /^<p class="warning">/);
});

test("clock shows dashes for a missing or invalid timestamp", () => {
  assert.equal(board.clock(undefined), "--:--:--");
  assert.equal(board.clock("soon"), "--:--:--");
});

test("render functions accept a missing state", () => {
  assert.doesNotThrow(() => board.renderBoard(null));
  assert.doesNotThrow(() => board.renderLogNotice(null));
  assert.doesNotThrow(() => board.renderSummary(null));
});

test("a job with an unknown stage is listed, not only counted", () => {
  const html = board.renderBoard(makeState([makeJob({ id: "007", stage: "limbo" })]));
  assert.match(html, /007/);
});

test("renderCard escapes slug, branch and reason", () => {
  const evil = "<img src=x onerror=alert(1)>";
  const html = board.renderCard(makeJob({ slug: evil, branch: evil, stage: "needs-human", reason: evil }), 200);
  assert.doesNotMatch(html, /<img/);
});

test("announcement is short and depends only on the level text", () => {
  assert.equal(board.announcement({ level: "ok", text: "Live, updated 10:00:01" }), "Dashboard is live");
  assert.equal(board.announcement({ level: "error", text: "Server unreachable" }), "Server unreachable");
});

test("a plan card shows which spec it belongs to", () => {
  const job = makeJob({ stage: "build", series: { index: 2, total: 5, id: "S2" } });
  const html = board.renderCard(job, 200, [{ series_id: "S2", title: "Baseline <b>benches</b>" }]);
  assert.match(html, />spec S2</);
  assert.match(html, /title="Spec S2: Baseline &lt;b&gt;benches&lt;\/b&gt;"/);
  assert.match(html, /plan 2 of 5/);
});

test("the spec chip works without spec titles and is left out for old jobs and one-plan series", () => {
  const chip = board.specChip({ index: 1, total: 3, id: "S9" }, undefined);
  assert.match(chip, />spec S9</);
  assert.match(chip, /title="Spec S9"/);
  assert.equal(board.specChip({ index: 1, total: 3 }, []), "");
  assert.equal(board.specChip({ index: 1, total: 1, id: "S1" }, []), "");
  assert.equal(board.specChip(null, []), "");
});

test("a plan card that has not started hides the empty sections and keeps the task list open", () => {
  const plan = { title: "Collection benches", goal: null, tasks: [{ number: 1, title: "the hook", status: "pending" }] };
  const extra = {
    series: { index: 2, total: 5, id: "S2" },
    plan,
    token_view: { weighted: 1, raw: 1, budget: 10, fraction: 0.1 },
    agents: [{ agent_id: "a", agent_type: "factory-builder", status: "running", tool_count: 1 }],
    worktree: "/tmp/wt",
  };
  const waiting = board.renderCard(makeJob(Object.assign({ stage: "spec" }, extra)), 200);
  assert.doesNotMatch(waiting, /Branch and files/);
  assert.doesNotMatch(waiting, /tokens-text/);
  assert.doesNotMatch(waiting, /Agent activity/);
  assert.match(waiting, /Task 1/);
  assert.match(waiting, /waiting/);
  assert.match(waiting, /<details class="tasks-box"[^>]* open/);
  const running = board.renderCard(makeJob(Object.assign({ stage: "build" }, extra)), 200);
  assert.match(running, /Branch and files/);
  assert.match(running, /tokens-text/);
  assert.match(running, /Agent activity/);
});

function makeSpec(overrides) {
  return Object.assign(
    {
      series_id: "S2",
      title: "Baseline benchmarks",
      goal: "Measure the hot paths.",
      stage: "spec",
      plans: [
        { job_id: "001", index: 1, title: "Harness", state: "merged" },
        { job_id: "002", index: 2, title: "Benches", state: "building" },
        { job_id: "003", index: 3, title: null, state: "waiting" },
      ],
    },
    overrides,
  );
}

test("the spec card shows the id, title, goal, plan rows and the done count", () => {
  const html = board.renderSpecCard(makeSpec());
  assert.match(html, /data-spec="S2"/);
  assert.match(html, />S2</);
  assert.match(html, /Baseline benchmarks/);
  assert.match(html, /Measure the hot paths/);
  assert.match(html, />3 plans</);
  assert.match(html, /1\/3 plans done/);
  assert.match(html, /Plan 1<\/span> Harness/);
  assert.match(html, /Plan 3<\/span>/);
  assert.match(html, /task-done/);
  assert.match(html, /task-active/);
  assert.match(html, /task-pending/);
  assert.match(html, />merged</);
  assert.match(html, />building</);
  assert.match(html, />waiting</);
  assert.equal((html.match(/class="seg seg-/g) || []).length, 3);
});

test("the spec card escapes titles and treats an unknown plan state as waiting", () => {
  const html = board.renderSpecCard(
    makeSpec({
      title: "<img src=x onerror=alert(1)>",
      plans: [{ job_id: "001", index: 1, title: "<script>x</script>", state: "exploding" }],
    }),
  );
  assert.doesNotMatch(html, /<img|<script>/);
  assert.match(html, /&lt;script&gt;/);
  assert.match(html, /task-pending/);
  assert.match(html, />waiting</);
});

test("a cancelled plan row and a needs-human row get their own look", () => {
  const html = board.renderSpecCard(
    makeSpec({
      stage: "needs-human",
      plans: [
        { job_id: "001", index: 1, title: "A", state: "cancelled" },
        { job_id: "002", index: 2, title: "B", state: "needs-human" },
      ],
    }),
  );
  assert.match(html, /task-cancelled/);
  assert.match(html, />cancelled</);
  assert.match(html, />needs human</);
  assert.match(html, /card-needs-human/);
});

test("the plan list is open while the series runs and closed once it merged, unless the user chose", () => {
  assert.match(board.renderSpecCard(makeSpec()), /<details class="tasks-box" data-tasks="spec-S2" open>/);
  assert.doesNotMatch(board.renderSpecCard(makeSpec({ stage: "merged" })), /data-tasks="spec-S2" open/);
  board.taskChoice.set("spec-S2", false);
  assert.doesNotMatch(board.renderSpecCard(makeSpec()), /data-tasks="spec-S2" open/);
  board.taskChoice.delete("spec-S2");
});

test("the board puts a spec card in its stage column and counts it", () => {
  const state = makeState([makeJob({ stage: "spec", id: "002" })], { specs: [makeSpec()] });
  const html = board.renderBoard(state);
  assert.match(html, /column-spec"><h2>Spec<span class="sub">[^<]*<\/span><span class="count">2<\/span>/);
  assert.match(html, /data-spec="S2"/);
  const merged = board.renderBoard(makeState([], { specs: [makeSpec({ stage: "merged" })] }));
  assert.match(merged, /column-merged"><h2>Merged<span class="sub">[^<]*<\/span><span class="count">1<\/span>[\s\S]*data-spec="S2"/);
});

test("a state from an older server without specs still renders the board", () => {
  const html = board.renderBoard(makeState([makeJob({ stage: "build" })]));
  assert.match(html, /column-build/);
  assert.doesNotMatch(html, /data-spec=/);
});

test("a needs-human card with details has a button, one without has none", () => {
  const withPark = board.renderCard(makeJob({ stage: "needs-human", reason: "why", has_park: true }), 200, []);
  assert.match(withPark, /<button type="button" class="cmd park-open" data-park="001">See what went wrong<\/button>/);
  const without = board.renderCard(makeJob({ stage: "needs-human", reason: "why" }), 200, []);
  assert.doesNotMatch(without, /data-park=/);
  const building = board.renderCard(makeJob({ stage: "build", has_park: true }), 200, []);
  assert.doesNotMatch(building, /data-park=/);
});

test("the card never contains the popup, so a repaint cannot close it", () => {
  const html = board.renderCard(makeJob({ stage: "needs-human", reason: "why", has_park: true }), 200, []);
  assert.doesNotMatch(html, /<dialog|park-text/);
});

test("parkKindLabel and parkKindClass cover every kind", () => {
  const red = ["checks-failed", "changes-remain", "builder-stopped", "commit-failed"];
  const amber = ["risky-diff", "diff-too-large", "budget-stopped", "builder-committed", "nothing-changed", "other"];
  for (const kind of red) assert.equal(board.parkKindClass(kind), "chip-changes", kind);
  for (const kind of amber) assert.equal(board.parkKindClass(kind), "chip-warn", kind);
  assert.equal(board.parkKindClass("something-new"), "chip-warn");
  assert.equal(board.parkKindLabel("checks-failed"), "Checks failed");
  assert.equal(board.parkKindLabel("other"), "Needs a human");
  assert.equal(board.parkKindLabel("something-new"), "Needs a human");
});

test("markLines marks the lines that matter and escapes the text", () => {
  const html = board.markLines(
    "error[E0432]: unresolved import\n  --> src/a.rs\ntest a::b ... FAILED\nthread 'x' panicked at src/c.rs:1\n[blocker] src/d.rs wrong\nFAIL cargo check\nall fine",
  );
  const hits = html.match(/class="ln ln-hit"/g) || [];
  assert.equal(hits.length, 5);
  assert.match(html, /<span class="ln">all fine<\/span>/);
  assert.match(html, /<span class="ln">  --&gt; src\/a.rs<\/span>/);
});

test("markLines escapes markup and keeps empty lines at full height", () => {
  const html = board.markLines('<script>alert("x")</script>\n\nlast\n');
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /&lt;script&gt;alert\(&quot;x&quot;\)&lt;\/script&gt;/);
  assert.match(html, /<span class="ln"> <\/span>/);
  // The trailing newline does not add a phantom last line.
  assert.equal((html.match(/class="ln/g) || []).length, 3);
});

test("markLines renders thousands of lines", () => {
  const text = Array.from({ length: 5000 }, (_, i) => `line ${i}`).join("\n");
  assert.equal((board.markLines(text).match(/class="ln/g) || []).length, 5000);
});

test("renderParkDialog shows the kind, reason, parts and next commands", () => {
  const job = makeJob({ stage: "needs-human", reason: "fallback" });
  const view = {
    kind: "checks-failed",
    reason: "checks failed after 2 fix rounds",
    at: 100,
    round: 3,
    next: ["factory rework 001 <note>"],
    parts: [
      { title: "Failing checks", source: "checks.md", text: "FAIL cargo check\nerror: boom", truncated: false },
      { title: "Cut log", source: "log", text: "tail", truncated: true },
    ],
  };
  const html = board.renderParkDialog(job, view, 220);
  assert.match(html, /<h2 id="park-title" class="park-title">checks failed after 2 fix rounds<\/h2>/);
  assert.match(html, /class="chip chip-changes">Checks failed</);
  assert.match(html, /parked 2m ago/);
  assert.match(html, /Failing checks/);
  assert.match(html, /checks\.md/);
  assert.match(html, /class="ln ln-hit">FAIL cargo check/);
  assert.match(html, /The start of this text was cut/);
  assert.match(html, /What to do next/);
  assert.match(html, /data-copy="factory rework 001 &lt;note&gt;"/);
  assert.match(html, /data-park-close/);
  assert.match(html, /class="park-note" role="status" hidden/);
});

test("renderParkDialog escapes the reason and survives missing fields", () => {
  const html = board.renderParkDialog(makeJob({ reason: null }), { reason: "<img onerror=x>" }, 10);
  assert.doesNotMatch(html, /<img/);
  assert.match(html, /&lt;img onerror=x&gt;/);
  assert.match(html, /No evidence was recorded for this park/);
  assert.doesNotMatch(html, /What to do next/);
  const bare = board.renderParkDialog(makeJob({ reason: null }), {}, 10);
  assert.match(bare, /Needs a human/);
});

test("parkMessage offers a retry only when asked", () => {
  const job = makeJob({});
  assert.match(board.parkMessage(job, "Loading.", false), /Loading\./);
  assert.doesNotMatch(board.parkMessage(job, "Loading.", false), /data-park-retry/);
  assert.match(board.parkMessage(job, "It failed.", true), /data-park-retry/);
  assert.match(board.parkMessage(job, "<b>", false), /&lt;b&gt;/);
});

const costs = {
  generated_at: 100,
  groups: {
    small: { n: 4, n_approx: 4, median: 280000, low: 220000, high: 370000, roles: { builder: 62 }, corrected: null },
    medium: { n: 2, n_approx: 1, median: 300000, low: 250000, high: 350000, roles: {}, corrected: { n: 1, median: 350000, low: 350000, high: 350000 } },
    all: { n: 6, n_approx: 5, median: 290000, low: 220000, high: 370000, roles: { builder: 62 }, corrected: null },
  },
  roles: { builder: 62, committer: 20, reviewer: 10, message: 8 },
};

test("the cost panel lists a row per group in order with median, range and notes", () => {
  const html = board.renderCostPanel(costs, 160);
  assert.match(html, /<h2[^>]*>Cost per job<\/h2>/);
  const order = ["small", "medium", "all"].map((g) => html.indexOf(`>${g}<`));
  assert.ok(order[0] > 0 && order[0] < order[1] && order[1] < order[2], html);
  assert.doesNotMatch(html, />complex</);
  assert.match(html, /280K/);
  assert.match(html, /220K to 370K/);
  assert.match(html, /4 of 4 approximate/);
  assert.match(html, /1 of 2 approximate/);
  assert.match(html, /few jobs/);
  assert.match(html, /updated 1m ago/);
  assert.match(html, /<div class="cost-head"><h2>Cost per job<\/h2><span class="cost-updated">updated 1m ago<\/span><\/div>/);
});

test("approximate medians carry a tilde with an explanation", () => {
  const html = board.renderCostPanel(costs, 160);
  assert.match(html, /280K <span class="approx" title="[^"]+">~<\/span>/);
});

test("corrected figures are listed when a group has them", () => {
  const html = board.renderCostPanel(costs, 160);
  assert.match(html, /Exact counts only/);
  assert.match(html, /medium: median 350K \(1 job\)/);
});

test("the role bar has a segment and a legend entry per known role", () => {
  const html = board.roleBar({ builder: 62, committer: 20, reviewer: 10, message: 8 });
  assert.equal((html.match(/class="role-seg role-(builder|committer|reviewer|message)" data-pct="\d+"/g) || []).length, 4);
  assert.match(html, /builder 62%/);
  assert.match(html, /role="img"/);
  assert.match(html, /aria-label="[^"]*builder 62%[^"]*"/);
});

test("an unknown or hostile role name is escaped and gets no class from file text", () => {
  const html = board.roleBar({ '<img onerror=x>': 40, builder: 60 });
  assert.doesNotMatch(html, /<img/);
  assert.match(html, /&lt;img onerror=x&gt; 40%/);
  assert.match(html, /class="role-seg role-other" data-pct="40"/);
  assert.equal(board.roleBar({}), "");
  assert.equal(board.roleBar(null), "");
});

test("group names are escaped and an unknown group is still listed after the known ones", () => {
  const html = board.renderCostPanel({ generated_at: 1, groups: { "<b>x</b>": { n: 1, n_approx: 0, median: 5, low: 5, high: 5, roles: {} } }, roles: {} }, 2);
  assert.doesNotMatch(html, /<b>x/);
  assert.match(html, /&lt;b&gt;x&lt;\/b&gt;/);
});

test("a group named like an object property renders as plain text", () => {
  const html = board.renderCostPanel({ generated_at: 1, groups: JSON.parse('{"constructor":{"n":1,"n_approx":0,"median":5,"low":5,"high":5,"roles":{}}}'), roles: {} }, 2);
  assert.match(html, /<th scope="row">constructor<\/th>/);
  assert.doesNotMatch(html, /function|native code/);
});

test("role shares are rounded and clamped, and a non-finite share is skipped", () => {
  const html = board.roleBar({ builder: 33.4, reviewer: Infinity, committer: 250 });
  assert.match(html, /builder 33%/);
  assert.match(html, /data-pct="33"/);
  assert.doesNotMatch(html, /reviewer|Infinity/);
  assert.match(html, /committer 100%/);
  assert.match(html, /data-pct="100"/);
});

test("the empty panel invites an action and does not throw on missing data", () => {
  for (const empty of [null, undefined, {}, { groups: {}, roles: {} }, { groups: { small: { n: 0 } }, roles: {} }]) {
    const html = board.renderCostPanel(empty, 10);
    assert.match(html, /No cost history yet/);
    assert.match(html, /data-copy="factory costs"/);
  }
});

test("an unmerged card with an estimate shows the range, group and caveats", () => {
  const job = makeJob({ stage: "build", estimate: { group: "small", median: 280000, low: 220000, high: 370000, n: 4, n_approx: 4 } });
  const html = board.renderCard(job, 200, []);
  assert.match(html, /<p class="estimate">Estimated 220K to 370K weighted \(small, 4 jobs, 4 approximate\)<\/p>/);
  const few = makeJob({ stage: "spec", estimate: { group: "all", median: 10, low: 10, high: 10, n: 1, n_approx: 0 } });
  assert.match(board.renderCard(few, 200, []), /Estimated 10 to 10 weighted \(all sizes, 1 job, few jobs\)/);
});

test("a card without an estimate shows no estimate line", () => {
  const html = board.renderCard(makeJob({ stage: "build" }), 200, []);
  assert.doesNotMatch(html, /class="estimate"/);
  assert.equal(board.estimateLine(makeJob({})), "");
  assert.equal(board.estimateLine(makeJob({ estimate: "lots" })), "");
});

test("a merged card compares the actual cost with the estimate range", () => {
  const est = { group: "small", median: 280000, low: 220000, high: 370000, n: 4, n_approx: 0 };
  const within = board.estimateLine(makeJob({ stage: "merged", estimate: est, token_view: { weighted: 300000 } }));
  assert.match(within, /Cost 300K, estimate 220K to 370K: within the range/);
  assert.match(board.estimateLine(makeJob({ stage: "merged", estimate: est, token_view: { weighted: 220000 } })), /within the range/);
  assert.match(board.estimateLine(makeJob({ stage: "merged", estimate: est, token_view: { weighted: 370000 } })), /within the range/);
  assert.match(board.estimateLine(makeJob({ stage: "merged", estimate: est, token_view: { weighted: 370001 } })), /above the range/);
  assert.match(board.estimateLine(makeJob({ stage: "merged", estimate: est, token_view: { weighted: 100 } })), /below the range/);
  const noActual = board.estimateLine(makeJob({ stage: "merged", estimate: est }));
  assert.match(noActual, /Estimated 220K to 370K/);
  assert.doesNotMatch(noActual, /Cost /);
});
