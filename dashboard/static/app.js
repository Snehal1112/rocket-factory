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
      syncParkNote();
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

  const dialog = byId("park-dialog");
  const parkContent = byId("park-content");
  let parkJobId = null;
  let parkOpener = null;

  // A modal dialog sits above everything, so the toast moves into it while it is open.
  function showToast(message) {
    const toast = byId("toast");
    const host = dialog.open ? dialog : document.querySelector(".page");
    if (toast.parentElement !== host) host.appendChild(toast);
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

  function findJob(id) {
    return lastState && lastState.jobs ? lastState.jobs.find((job) => job.id === id) : null;
  }

  function nowSeconds() {
    return lastState ? lastState.generated_at : Date.now() / 1000;
  }

  // Tells the user when the job moved on while the popup was open.
  function syncParkNote() {
    if (!dialog.open || !parkJobId) return;
    const job = findJob(parkJobId);
    const note = parkContent.querySelector(".park-note");
    if (note) note.hidden = !job || job.stage === "needs-human";
  }

  // Replacing the content drops the focused node, so focus goes back into the dialog.
  function setParkContent(html) {
    parkContent.innerHTML = html;
    if (dialog.contains(document.activeElement)) return;
    const close = parkContent.querySelector("[data-park-close]");
    (close || dialog).focus();
  }

  async function loadPark(id) {
    const job = findJob(id) || { id, reason: null };
    try {
      const response = await fetch(`/api/jobs/${encodeURIComponent(id)}/park`, { cache: "no-store" });
      if (parkJobId !== id) return;
      if (response.status === 404) {
        setParkContent(FactoryBoard.parkMessage(job, "No details were recorded for this job.", false));
        return;
      }
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const view = await response.json();
      if (parkJobId !== id) return;
      setParkContent(FactoryBoard.renderParkDialog(job, view, nowSeconds()));
      syncParkNote();
      const hit = parkContent.querySelector(".ln-hit");
      if (hit) hit.scrollIntoView({ block: "center" });
    } catch (error) {
      if (parkJobId !== id) return;
      setParkContent(FactoryBoard.parkMessage(
        job,
        `The details could not be loaded (${error.message}). Check that the server is running, then try again.`,
        true,
      ));
    }
  }

  function openPark(id, opener) {
    const job = findJob(id) || { id, reason: null };
    parkJobId = id;
    parkOpener = opener || null;
    setParkContent(FactoryBoard.parkMessage(job, "Loading the details.", false));
    if (!dialog.open) dialog.showModal();
    loadPark(id);
  }

  dialog.addEventListener("close", () => {
    const id = parkJobId;
    parkJobId = null;
    const toast = byId("toast");
    if (toast.parentElement !== document.querySelector(".page")) document.querySelector(".page").appendChild(toast);
    toast.hidden = true;
    // The card is redrawn every second, so find the button again when the old node is gone.
    const opener = parkOpener && document.contains(parkOpener) ? parkOpener : document.querySelector(`[data-park="${id}"]`);
    if (opener) opener.focus();
    parkOpener = null;
  });

  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    if (!target) return;
    const copy = target.closest("[data-copy]");
    if (copy) {
      copyText(copy.getAttribute("data-copy") || "");
      return;
    }
    const open = target.closest("[data-park]");
    if (open) {
      openPark(open.getAttribute("data-park") || "", open);
      return;
    }
    if (target.closest("[data-park-close]")) {
      dialog.close();
      return;
    }
    if (target.closest("[data-park-retry]")) {
      if (parkJobId) openPark(parkJobId, parkOpener);
      return;
    }
    // A click on the backdrop lands on the dialog element itself.
    if (target === dialog) dialog.close();
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
