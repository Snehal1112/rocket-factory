"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const projects = require("../static/projects.js");

const LIST = [
  { name: "zeta", status: "disabled", reason: "project.toml is broken" },
  { name: "alpha", status: "ready", reason: null },
  { name: "mid", status: "busy", reason: "a job is running" },
];

test("renderProjectTabs orders by name and marks the selected tab", () => {
  const html = projects.renderProjectTabs(LIST, "mid");
  const names = [...html.matchAll(/data-project="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(names, ["alpha", "mid", "zeta"]);
  assert.match(html, /data-project="mid"[^>]*aria-selected="true"/);
  assert.match(html, /data-project="alpha"[^>]*aria-selected="false"/);
});

test("renderProjectTabs marks a disabled project with its reason", () => {
  const html = projects.renderProjectTabs(LIST, "alpha");
  assert.match(html, /tab-disabled/);
  assert.match(html, /project\.toml is broken/);
  assert.equal((html.match(/tab-disabled/g) || []).length, 1);
});

test("renderProjectTabs escapes names and reasons", () => {
  const html = projects.renderProjectTabs([{ name: "a\"<b>", status: "disabled", reason: "<script>x</script>" }], null);
  assert.ok(!html.includes("<script>"));
  assert.ok(!html.includes("<b>"));
});

test("renderProjectTabs gives nothing for no projects", () => {
  assert.equal(projects.renderProjectTabs([], null), "");
  assert.equal(projects.renderProjectTabs(null, null), "");
});

test("the selected project survives a poll refresh", () => {
  assert.equal(projects.selectProject(LIST, "mid"), "mid");
  const refreshed = LIST.map((p) => ({ ...p }));
  assert.equal(projects.selectProject(refreshed, "mid"), "mid");
});

test("selectProject falls back to the first ready project by name", () => {
  assert.equal(projects.selectProject(LIST, null), "alpha");
  assert.equal(projects.selectProject(LIST, "gone"), "alpha");
  assert.equal(projects.selectProject([{ name: "b", status: "disabled" }, { name: "a", status: "disabled" }], null), "a");
  assert.equal(projects.selectProject([], "x"), null);
});

test("renderProjectNote explains a project that cannot take work", () => {
  assert.match(projects.renderProjectNote(LIST[0]), /project\.toml is broken/);
  assert.equal(projects.renderProjectNote(LIST[1]), "");
  assert.equal(projects.renderProjectNote(null), "");
});

const FORM = { spec: "/a/spec.md", plans: "/a/plans", model: "", slug: "", run: true, series: true };

test("buildCreateBody returns the body with run and series", () => {
  assert.deepEqual(projects.buildCreateBody(FORM), { spec: "/a/spec.md", plans: "/a/plans", run: true, series: true });
});

test("buildCreateBody keeps model and slug only when given", () => {
  const body = projects.buildCreateBody({ ...FORM, model: " sonnet ", slug: "my-job", run: false, series: false });
  assert.deepEqual(body, { spec: "/a/spec.md", plans: "/a/plans", model: "sonnet", slug: "my-job", run: false, series: false });
});

test("buildCreateBody refuses empty fields and relative paths with a message", () => {
  for (const bad of [
    { spec: "" },
    { plans: "  " },
    { spec: "spec.md" },
    { plans: "./plans" },
    { spec: "/a/../b" },
    { model: "bad model" },
    { model: "x".repeat(65) },
    { slug: "Upper" },
    { slug: "-lead" },
    { slug: "a".repeat(65) },
  ]) {
    const result = projects.buildCreateBody({ ...FORM, ...bad });
    assert.equal(typeof result.error, "string", JSON.stringify(bad));
    assert.ok(result.error.length > 0);
    assert.equal(result.spec, undefined);
  }
});

test("isSafeId accepts digits only", () => {
  assert.equal(projects.isSafeId("001"), true);
  assert.equal(projects.isSafeId("12"), true);
  for (const bad of ["", "1a", "../1", "1/2", " 1", "1 ", null, undefined, 5]) {
    assert.equal(projects.isSafeId(bad), false, String(bad));
  }
});

test("jobActions follows the stage and the cancelled flag", () => {
  assert.deepEqual(projects.jobActions({ stage: "needs-human" }), ["approve", "rework", "cancel"]);
  assert.deepEqual(projects.jobActions({ stage: "approve" }), ["approve", "cancel"]);
  assert.deepEqual(projects.jobActions({ stage: "build" }), ["cancel"]);
  assert.deepEqual(projects.jobActions({ stage: "merged" }), []);
  assert.deepEqual(projects.jobActions({ stage: "needs-human", cancelled: true }), []);
  assert.deepEqual(projects.jobActions({ stage: "needs-human", park_resume: "review" }), ["retry", "approve", "cancel"]);
  assert.deepEqual(projects.jobActions({ stage: "needs-human", park_resume: "build", park_kind: "series-waiting" }), ["retry", "cancel"]);
  assert.deepEqual(projects.jobActions({ stage: "needs-human", park_resume: "build", park_kind: "machine-waiting" }), ["retry", "rework", "cancel"]);
  assert.deepEqual(projects.jobActions({ stage: "needs-human", park_resume: "build", park_kind: "builder-stopped" }), ["retry", "rework", "cancel"]);
  assert.deepEqual(projects.jobActions({ stage: "needs-human", park_resume: "" }), ["approve", "rework", "cancel"]);
  assert.deepEqual(projects.jobActions({ stage: "needs-human", park_resume: 5 }), ["approve", "rework", "cancel"]);
  assert.deepEqual(projects.jobActions({ stage: "needs-human", park_resume: "sync", cancelled: true }), []);
});

test("jobActions offers rework on a checks-failed park with a resume", () => {
  const job = { stage: "needs-human", park_resume: "rereview", park_kind: "checks-failed" };
  assert.deepEqual(projects.jobActions(job), ["retry", "rework", "cancel"]);
  const other = { stage: "needs-human", park_resume: "rereview", park_kind: "review-failed" };
  assert.deepEqual(projects.jobActions(other), ["retry", "approve", "cancel"]);
});

test("a reply for a stale project is dropped", () => {
  assert.equal(projects.acceptReply("alpha", "alpha"), true);
  assert.equal(projects.acceptReply("alpha", "beta"), false);
  assert.equal(projects.acceptReply(null, null), true);
  assert.equal(projects.acceptReply("alpha", null), false);
});

test("jobPathFor builds the path from the button's own project", () => {
  assert.equal(projects.jobPathFor("p", "001", "retry"), "/api/projects/p/jobs/001/retry");
  assert.equal(projects.jobPathFor("alpha", "003", "approve"), "/api/projects/alpha/jobs/003/approve");
  assert.equal(projects.jobPathFor("beta", "003", "cancel"), "/api/projects/beta/jobs/003/cancel");
  // A job id of project alpha is never built against another project: the project argument decides.
  assert.notEqual(projects.jobPathFor("alpha", "003", "rework"), projects.jobPathFor("beta", "003", "rework"));
});

test("jobPathFor refuses unsafe parts", () => {
  for (const args of [["", "1", "approve"], ["..", "1", "approve"], ["a/b", "1", "approve"], ["a", "x", "approve"], ["a", "1", "delete"], [null, "1", "approve"], ["a", "../1", "approve"]]) {
    assert.equal(projects.jobPathFor(...args), null, JSON.stringify(args));
  }
});

test("action buttons carry their project", () => {
  const board = require("../static/board.js");
  const html = board.actionButtons({ id: "003", project: "alpha", actions: ["approve", "cancel"] });
  assert.equal((html.match(/data-act-project="alpha"/g) || []).length, 2);
});

test("errorText shows the stable code and the message", () => {
  assert.equal(projects.errorText({ error: { code: "conflict", message: "busy" } }, 409), "conflict: busy");
  assert.equal(projects.errorText({ error: { code: "token_rejected", message: "m" } }, 502), "token_rejected: m");
  assert.equal(projects.errorText(null, 500), "HTTP 500");
  assert.equal(projects.errorText({ nope: 1 }, 400), "HTTP 400");
});

test("buildNoteBody needs a note for rework and trims it", () => {
  assert.deepEqual(projects.buildReworkBody(" fix it "), { note: "fix it" });
  assert.equal(typeof projects.buildReworkBody("").error, "string");
  assert.equal(typeof projects.buildReworkBody("a\nb").error, "string");
  assert.equal(typeof projects.buildReworkBody("x".repeat(201)).error, "string");
});

test("buildWaiveBody refuses empty and multi-line reasons", () => {
  assert.deepEqual(projects.buildWaiveBody(" known flake "), { waive_reason: "known flake" });
  assert.equal(typeof projects.buildWaiveBody("").error, "string");
  assert.equal(typeof projects.buildWaiveBody("a\nb").error, "string");
  assert.equal(typeof projects.buildWaiveBody("x".repeat(201)).error, "string");
});

test("buildCancelBody keeps reason and force only when set", () => {
  assert.deepEqual(projects.buildCancelBody("", false), {});
  assert.deepEqual(projects.buildCancelBody(" why ", true), { reason: "why", force: true });
});

test("cancelTitle names the later plans of a series", () => {
  assert.equal(projects.cancelTitle({ id: "030" }), "Cancel job 030");
  assert.equal(projects.cancelTitle({ id: "030", series: { id: "S7", index: 1, total: 5 } }), "Cancel job 030 and the later plans of S7");
  assert.equal(projects.cancelTitle({ id: "034", series: { id: "S7", index: 5, total: 5 } }), "Cancel job 034");
  assert.equal(projects.cancelTitle({ id: "040", series: { index: 1, total: 2 } }), "Cancel job 040");
});

const budget = (limit, used, resetsAt = "2026-10-18T00:00:00Z") => ({ days: 14, projects: [], dailyBudget: { limit, used, resetsAt } });

test("budgetStrip is none without a dailyBudget", () => {
  assert.equal(projects.budgetStrip({ days: 14, projects: [] }).level, "none");
  assert.equal(projects.budgetStrip(null).level, "none");
  assert.equal(projects.budgetStrip({ dailyBudget: null }).level, "none");
  assert.equal(projects.budgetStrip(budget(0, 5)).level, "none");
  assert.equal(projects.budgetStrip(budget("x", 5)).level, "none");
  assert.equal(projects.budgetStrip(budget(100, -1)).level, "none");
  assert.equal(projects.budgetStrip({ days: 1 }).text, "");
});

test("budgetStrip levels follow the share used", () => {
  assert.equal(projects.budgetStrip(budget(100, 79)).level, "ok");
  assert.equal(projects.budgetStrip(budget(100, 0)).level, "ok");
  assert.equal(projects.budgetStrip(budget(100, 80)).level, "warn");
  assert.equal(projects.budgetStrip(budget(100, 99)).level, "warn");
  assert.equal(projects.budgetStrip(budget(100, 100)).level, "stop");
  assert.equal(projects.budgetStrip(budget(100, 250)).level, "stop");
});

test("budgetStrip formats counts and shows the reset time in UTC", () => {
  const strip = projects.budgetStrip(budget(5000000, 1234567));
  assert.match(strip.text, /1,234,567/);
  assert.match(strip.text, /5,000,000/);
  assert.match(strip.text, /24%/);
  assert.match(strip.text, /2026-10-18 00:00 UTC/);
});

test("budgetStrip says new runs are waiting at the limit", () => {
  const strip = projects.budgetStrip(budget(100, 100));
  assert.match(strip.text, /New runs are waiting/);
  assert.match(strip.text, /2026-10-18 00:00 UTC/);
});

test("budgetStrip text holds numbers and a fixed phrase only", () => {
  const hostile = projects.budgetStrip(budget(100, 50, "<img src=x onerror=1>"));
  assert.ok(!/[<>"&]/.test(hostile.text));
  assert.ok(!hostile.text.includes("img"));
  const odd = projects.budgetStrip({ dailyBudget: { limit: "<b>100</b>", used: "<i>1</i>", resetsAt: "<x>" } });
  assert.equal(odd.level, "none");
  assert.ok(!/[<>]/.test(odd.text));
});

test("budgetStrip is global: it takes only the usage reply, never a project", () => {
  assert.equal(projects.budgetStrip.length, 1);
});

test("versionLabel shows the dashboard version in root mode", () => {
  const label = projects.versionLabel({ dashboard: "0.1.6" });
  assert.deepEqual(label, { text: "v0.1.6", title: "dashboard 0.1.6", warn: false });
});

test("versionLabel shows the factoryd version and lists all three in home mode", () => {
  const label = projects.versionLabel({ dashboard: "0.1.6", factoryd: "0.1.6", factory: "0.1.6" });
  assert.equal(label.text, "v0.1.6");
  assert.equal(label.title, "dashboard 0.1.6, factoryd 0.1.6, factory 0.1.6");
  assert.equal(label.warn, false);
});

test("versionLabel warns when the versions differ", () => {
  const label = projects.versionLabel({ dashboard: "0.1.6", factoryd: "0.1.7", factory: "0.1.6" });
  assert.equal(label.text, "v0.1.7");
  assert.equal(label.warn, true);
  assert.match(label.title, /The versions differ/);
});

test("versionLabel adds why a version is missing", () => {
  const label = projects.versionLabel({ dashboard: "0.1.6", apiError: "factoryd is down", factoryError: "no binary" });
  assert.equal(label.text, "v0.1.6");
  assert.equal(label.warn, false);
  assert.match(label.title, /factory: no binary/);
  assert.match(label.title, /factoryd is down/);
});

test("versionLabel ignores replies without a sane dashboard version", () => {
  assert.equal(projects.versionLabel(null).text, "");
  assert.equal(projects.versionLabel({}).text, "");
  assert.equal(projects.versionLabel({ dashboard: "<b>x</b>" }).text, "");
  assert.equal(projects.versionLabel({ dashboard: "0.1.6", factoryd: "<i>" }).text, "v0.1.6");
});

test("jobActions drops approve while open changes wait on a missing review", () => {
  const blocked = {
    stage: "needs-human",
    last_review: { verdict: "changes", blockers: 3 },
    reviews: { code: "changes", security: "failed" },
  };
  assert.equal(projects.approveBlocked(blocked), true);
  assert.deepEqual(projects.jobActions(blocked), ["rework", "cancel"]);
  assert.deepEqual(projects.jobActions({ ...blocked, park_resume: "review" }), ["retry", "cancel"]);
  assert.deepEqual(projects.jobActions({ ...blocked, stage: "approve" }), ["cancel"]);
  const passed = { ...blocked, reviews: { code: "changes", security: "pass" } };
  assert.equal(projects.approveBlocked(passed), false);
  assert.deepEqual(projects.jobActions(passed), ["approve", "rework", "cancel"]);
  assert.deepEqual(projects.jobActions({ ...passed, park_resume: "review" }), ["retry", "approve", "cancel"]);
});
