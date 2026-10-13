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
    { generated_at: 200, board_error: null, jobs, unassigned_agents: [], recent: [] },
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

test("renderUnassigned and renderFeed", () => {
  assert.equal(board.renderUnassigned(makeState([])), "");
  const state = makeState([], {
    unassigned_agents: [{ agent_type: "factory-approver", status: "running" }],
    recent: [{ ts: 150, kind: "tool", agent_type: "factory-builder", tool: "Read", target: "CLAUDE.md" }],
  });
  assert.match(board.renderUnassigned(state), /approver/);
  assert.match(board.renderFeed(state), /builder/);
  assert.match(board.renderFeed(state), /Read CLAUDE\.md/);
  assert.match(board.renderFeed(makeState([])), /No agent activity yet/);
});

test("every stage, the other agents and the live feed are card panels", () => {
  const state = makeState([makeJob({ stage: "build" })], {
    unassigned_agents: [{ agent_type: "factory-approver", status: "running" }],
    recent: [{ ts: 150, kind: "tool", agent_type: "factory-builder", tool: "Read", target: "CLAUDE.md" }],
  });
  const boardHtml = board.renderBoard(state);
  assert.equal((boardHtml.match(/<section class="column panel /g) || []).length, 6);
  assert.match(board.renderUnassigned(state), /^<section class="panel">[\s\S]*<\/section>$/);
  assert.match(board.renderFeed(state), /^<section class="panel">[\s\S]*<\/section>$/);
  assert.match(board.renderFeed(makeState([])), /^<section class="panel">[\s\S]*<\/section>$/);
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
  const quiet = board.renderSummary(makeState([makeJob({ stage: "spec" })]));
  assert.match(quiet, />1 job is in progress</);
  assert.match(quiet, /No agent is active at the moment/);
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

test("renderFeed reports skipped log lines only when there are some", () => {
  assert.doesNotMatch(board.renderFeed(makeState([])), /skipped/);
  assert.match(board.renderFeed(makeState([], { skipped_lines: 3 })), /3 event log lines skipped/);
  assert.match(board.renderFeed(makeState([], { skipped_lines: 1, recent: [{ ts: 1, kind: "tool" }] })), /1 event log line skipped/);
});

test("announcement is short and depends only on the level text", () => {
  assert.equal(board.announcement({ level: "ok", text: "Live, updated 10:00:01" }), "Dashboard is live");
  assert.equal(board.announcement({ level: "error", text: "Server unreachable" }), "Server unreachable");
});
