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

test("buildCancelBody keeps reason and force only when set", () => {
  assert.deepEqual(projects.buildCancelBody("", false), {});
  assert.deepEqual(projects.buildCancelBody(" why ", true), { reason: "why", force: true });
});
