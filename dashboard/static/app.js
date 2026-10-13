"use strict";

(function () {
  const POLL_MS = 1000;
  let lastState = null;
  let lastOk = null;
  let inFlight = false;
  let lastLevel = null;
  let toastTimer = null;

  function byId(id) {
    return document.getElementById(id);
  }

  const shown = {};

  function setText(el, text) {
    if (el.textContent !== text) el.textContent = text;
  }

  function setHtml(id, html) {
    if (shown[id] === html) return;
    shown[id] = html;
    const template = document.createElement("template");
    template.innerHTML = html;
    FactoryPatch.patchChildren(byId(id), template.content);
  }

  // Bar widths go through the CSSOM, because the CSP forbids inline style attributes.
  function applyBarWidths() {
    for (const fill of document.querySelectorAll(".tokens-fill")) {
      const width = `${fill.getAttribute("data-pct") || 0}%`;
      if (fill.style.width !== width) fill.style.width = width;
    }
  }

  function render(state, error) {
    const view = state || lastState;
    if (view) {
      setHtml("summary", FactoryBoard.renderSummary(view));
      setHtml("board", FactoryBoard.renderBoard(view));
      setHtml("others", FactoryBoard.renderUnassigned(view));
      setHtml("notice", FactoryBoard.renderLogNotice(view));
      applyBarWidths();
    }
    const header = FactoryBoard.headerStatus(state, error, lastOk);
    const className = `status status-${header.level}`;
    if (byId("status").className !== className) byId("status").className = className;
    setText(byId("status-text"), header.text);
    if (header.level !== lastLevel) {
      lastLevel = header.level;
      setText(byId("announce"), FactoryBoard.announcement(header));
    }
    byId("board").classList.toggle("is-stale", Boolean(error));
  }

  function showToast(message) {
    const toast = byId("toast");
    toast.textContent = message;
    toast.hidden = false;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast.hidden = true;
      toast.textContent = "";
    }, 2000);
  }

  function legacyCopy(text) {
    const area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    let ok = false;
    try {
      ok = document.execCommand("copy");
    } catch (error) {
      ok = false;
    }
    area.remove();
    return ok;
  }

  async function copyText(text) {
    let ok = false;
    try {
      await navigator.clipboard.writeText(text);
      ok = true;
    } catch (error) {
      ok = legacyCopy(text);
    }
    showToast(ok ? `Copied ${text}` : "Copy failed. Select the command by hand.");
  }

  document.addEventListener("click", (event) => {
    const source = event.target instanceof Element ? event.target.closest("[data-copy]") : null;
    if (source) copyText(source.getAttribute("data-copy") || "");
  });

  async function tick() {
    if (inFlight) return;
    inFlight = true;
    let state;
    try {
      const response = await fetch("/api/state", { cache: "no-store" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      state = await response.json();
    } catch (error) {
      render(null, error.message);
      return;
    } finally {
      inFlight = false;
    }
    lastState = state;
    lastOk = state.generated_at;
    render(state, null);
  }

  // Remember which agent lists and task lists the user opened or closed, because the board is redrawn every second.
  byId("board").addEventListener(
    "toggle",
    (event) => {
      const box = event.target;
      if (!box.matches) return;
      if (box.matches("details[data-tasks]")) {
        FactoryBoard.taskChoice.set(box.getAttribute("data-tasks"), box.open);
        return;
      }
      if (!box.matches("details[data-agents]")) return;
      const id = box.getAttribute("data-agents");
      if (box.open) FactoryBoard.openAgents.add(id);
      else FactoryBoard.openAgents.delete(id);
    },
    true,
  );

  tick();
  setInterval(tick, POLL_MS);
})();
