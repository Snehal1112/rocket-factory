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

test("a card shows a provider chip only when provider is set", () => {
  const withChip = board.renderCard(makeJob({ provider: "codex" }));
  assert.match(withChip, /chip-provider[^>]*>codex</);
  assert.doesNotMatch(board.renderCard(makeJob({})), /chip-provider/);
});

test("the_provider_chip_escapes_its_text", () => {
  const html = board.renderCard(makeJob({ provider: "<img src=x>" }));
  assert.match(html, /&lt;img/);
  assert.doesNotMatch(html, /<img/);
});

test("an_empty_provider_shows_no_chip", () => {
  assert.equal(board.providerChip(""), "");
  assert.equal(board.providerChip(null), "");
  assert.doesNotMatch(board.renderCard(makeJob({ provider: "" })), /chip-provider/);
});
