"use strict";

(function () {
  const POLL_MS = 1000;
  let lastState = null;
  let lastOk = null;

  function byId(id) {
    return document.getElementById(id);
  }

  const shown = {};

  function setHtml(id, html) {
    if (shown[id] === html) return;
    shown[id] = html;
    byId(id).innerHTML = html;
  }

  function render(state, error) {
    const view = state || lastState;
    if (view) {
      setHtml("board", FactoryBoard.renderBoard(view));
      setHtml("others", FactoryBoard.renderUnassigned(view));
      setHtml("feed", FactoryBoard.renderFeed(view));
    }
    const header = FactoryBoard.headerStatus(state, error, lastOk);
    byId("status").className = `status status-${header.level}`;
    byId("status-text").textContent = header.text;
    byId("board").classList.toggle("is-stale", Boolean(error));
  }

  async function tick() {
    try {
      const response = await fetch("/api/state", { cache: "no-store" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const state = await response.json();
      lastState = state;
      lastOk = state.generated_at;
      render(state, null);
    } catch (error) {
      render(null, error.message);
    }
  }

  tick();
  setInterval(tick, POLL_MS);
})();
