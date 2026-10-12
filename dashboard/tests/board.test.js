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
  assert.match(chip, />sec pass</);
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
  assert.match(review, /sec pass/);
  assert.match(review, /ux skipped/);
  assert.match(review, /code changes/);
});

test("renderCard for needs-human shows the reason and the commands for that job", () => {
  const html = board.renderCard(makeJob({ id: "003", stage: "needs-human", reason: "touches the <trust gate>" }));
  assert.match(html, /touches the &lt;trust gate&gt;/);
  assert.match(html, /\/factory approve 003/);
  assert.match(html, /\/factory rework 003 &lt;note&gt;/);
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

test("headerStatus", () => {
  assert.equal(board.headerStatus(makeState([]), null, 200).level, "ok");
  assert.match(board.headerStatus(makeState([]), null, 200).text, /^board\.json · \d\d:\d\d:\d\d$/);
  assert.equal(board.headerStatus(null, "HTTP 500", null).text, "server unreachable");
  assert.match(board.headerStatus(null, "boom", 150).text, /^server unreachable · last update \d\d:\d\d:\d\d$/);
  assert.equal(board.headerStatus(null, "boom", 150).level, "error");
  const warn = board.headerStatus(makeState([], { board_error: "board.json is invalid: x" }), null, 200);
  assert.equal(warn.level, "warn");
  assert.equal(warn.text, "board.json is invalid: x");
  assert.equal(board.headerStatus(null, null, null).level, "warn");
});
