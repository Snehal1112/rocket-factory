"use strict";
process.env.TZ = "UTC";

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
      ],
    },
    overrides,
  );
}

function makeDraft(overrides) {
  return Object.assign(
    {
      slug: "dark-mode",
      feature: "Add a dark mode",
      stage: "interviewing",
      spec_file: "docs/specs/dark-mode.md",
      created: "2026-10-18",
      updated_at: 100,
      seconds_since_update: 90,
      stale: false,
    },
    overrides,
  );
}

test("formatStamp prints the local date and time and is empty for bad input", () => {
  assert.equal(board.formatStamp(0), "");
  assert.equal(board.formatStamp(1792490800), "2026-10-20 10:06");
  assert.equal(board.formatStamp(NaN), "");
  assert.equal(board.formatStamp("5"), "");
  assert.equal(board.formatStamp(-1), "");
});

test("a job header shows the stamp of stage_since", () => {
  const html = board.renderCard(makeJob({ stage_since: 1792490800 }));
  assert.match(html, /2026-10-20 10:06/);
  assert.match(html, /title="Last stage change"/);
});

test("a job without a usable stage_since shows its created date", () => {
  const html = board.renderCard(makeJob({ stage_since: undefined, created: "2026-10-13" }));
  assert.match(html, /2026-10-13/);
});

test("a spec card header shows its stamp, and none without stage_since", () => {
  const withStamp = board.renderSpecCard(makeSpec({ stage_since: 1792490800 }));
  assert.match(withStamp, /<time class="job-date"[^>]*>2026-10-20 10:06<\/time>/);
  const without = board.renderSpecCard(makeSpec({ stage_since: undefined }));
  assert.doesNotMatch(without, /job-date/);
});

test("a draft header shows updated_at and falls back to created", () => {
  const stamped = board.renderDraftCard(makeDraft({ updated_at: 1792490800 }));
  assert.match(stamped, /2026-10-20 10:06/);
  assert.match(stamped, /title="Last change"/);
  const fallback = board.renderDraftCard(makeDraft({ updated_at: undefined, created: "2026-10-18" }));
  assert.match(fallback, /2026-10-18/);
});

function column(html, key) {
  const start = html.indexOf(`column panel column-${key}"`);
  const next = html.indexOf('<section class="column', start + 1);
  return html.slice(start, next === -1 ? undefined : next);
}

test("a merged spec card sorts among the merged jobs by time", () => {
  const jobs = [
    makeJob({ id: "001", stage: "merged", stage_since: 100 }),
    makeJob({ id: "002", stage: "merged", stage_since: 300 }),
  ];
  const specs = [makeSpec({ series_id: "S9", stage: "merged", stage_since: 200 })];
  const html = board.renderBoard(makeState(jobs, { specs }));
  const at = (needle) => html.indexOf(needle);
  assert.ok(at('data-job="002"') < at('data-spec="S9"'));
  assert.ok(at('data-spec="S9"') < at('data-job="001"'));
});

test("the merged column keeps the 10 newest cards of any kind and counts all", () => {
  const jobs = Array.from({ length: 8 }, (_, n) =>
    makeJob({ id: String(n + 1).padStart(3, "0"), stage: "merged", stage_since: n + 1 }),
  );
  const specs = Array.from({ length: 5 }, (_, n) =>
    makeSpec({ series_id: `S${n + 1}`, stage: "merged", stage_since: n + 9 }),
  );
  const html = board.renderBoard(makeState(jobs, { specs }));
  const merged = column(html, "merged");
  assert.equal((merged.match(/<article class="card/g) || []).length, 10);
  assert.match(merged, /<span class="count">13</);
  assert.ok(merged.indexOf('data-spec="S5"') < merged.indexOf('data-spec="S4"'));
  assert.doesNotMatch(merged, /data-job="001"/);
  board.taskChoice.clear();
});

test("columns other than merged are not capped", () => {
  const jobs = Array.from({ length: 12 }, (_, n) =>
    makeJob({ id: String(n + 1).padStart(3, "0"), stage: "build", stage_since: n + 1 }),
  );
  const build = column(board.renderBoard(makeState(jobs)), "build");
  assert.equal((build.match(/<article class="card/g) || []).length, 12);
});

test("equal times keep the later entry first", () => {
  const jobs = [
    makeJob({ id: "001", stage: "build", stage_since: 50 }),
    makeJob({ id: "002", stage: "build", stage_since: 50 }),
  ];
  const html = board.renderBoard(makeState(jobs));
  assert.ok(html.indexOf('data-job="002"') < html.indexOf('data-job="001"'));
});

test("a job with no stage_since sorts last and shows its created date", () => {
  const jobs = [
    makeJob({ id: "001", stage: "build", stage_since: undefined, created: "2026-10-13" }),
    makeJob({ id: "002", stage: "build", stage_since: 50 }),
  ];
  const html = board.renderBoard(makeState(jobs));
  assert.ok(html.indexOf('data-job="002"') < html.indexOf('data-job="001"'));
  assert.match(html, /2026-10-13/);
});

test("a non-numeric stage_since counts as 0", () => {
  const jobs = [
    makeJob({ id: "001", stage: "build", stage_since: "99" }),
    makeJob({ id: "002", stage: "build", stage_since: 5 }),
  ];
  const html = board.renderBoard(makeState(jobs));
  assert.ok(html.indexOf('data-job="002"') < html.indexOf('data-job="001"'));
});

test("every column sorts specs and jobs together", () => {
  const jobs = [makeJob({ id: "001", stage: "needs-human", stage_since: 10 })];
  const specs = [makeSpec({ series_id: "S7", stage: "needs-human", stage_since: 20 })];
  const col = column(board.renderBoard(makeState(jobs, { specs })), "needs-human");
  assert.ok(col.indexOf('data-spec="S7"') < col.indexOf('data-job="001"'));
  board.taskChoice.clear();
});

test("drafts stay first in the spec column, show updated_at, fall back to created, and are counted", () => {
  const jobs = [makeJob({ id: "001", stage: "spec", stage_since: 40 })];
  const specs = [makeSpec({ series_id: "S7", stage: "spec", stage_since: 50 })];
  const drafts = [makeDraft({ updated_at: 1792490800 })];
  const col = column(board.renderBoard(makeState(jobs, { specs, drafts })), "spec");
  const draftAt = col.indexOf("dark-mode");
  assert.ok(draftAt >= 0);
  assert.ok(draftAt < col.indexOf('data-spec="S7"'));
  assert.ok(col.indexOf('data-spec="S7"') < col.indexOf('data-job="001"'));
  assert.match(col, /<span class="count">3</);
  assert.match(col, /2026-10-20 10:06/);
  board.taskChoice.clear();
});
