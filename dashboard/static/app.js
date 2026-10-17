"use strict";

(function () {
  const POLL_MS = 1000;
  let lastState = null;
  let lastOk = null;
  let inFlight = false;
  let lastLevel = null;
  let toastTimer = null;
  // "unknown" until /api/projects answers: 200 means home mode, 404 means a single root.
  let mode = "unknown";
  let projects = [];
  let selected = null;
  const STORE_KEY = "factory-project";

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
    for (const fill of document.querySelectorAll(".tokens-fill, .role-seg")) {
      const width = `${fill.getAttribute("data-pct") || 0}%`;
      if (fill.style.width !== width) fill.style.width = width;
    }
  }

  function stateUrl() {
    return mode === "home" ? `/api/projects/${encodeURIComponent(selected)}/state` : "/api/state";
  }

  function parkUrl(id) {
    const base = mode === "home" ? `/api/projects/${encodeURIComponent(selected)}` : "/api";
    return `${base}/jobs/${encodeURIComponent(id)}/park`;
  }

  // A banner raised by polling goes away by itself once the poll works again.
  let pollBanner = false;

  function showBanner(message, fromPoll) {
    pollBanner = Boolean(fromPoll);
    byId("banner-text").textContent = message;
    byId("banner").hidden = false;
  }

  function hideBanner() {
    pollBanner = false;
    byId("banner").hidden = true;
  }

  function currentProject() {
    return projects.find((project) => project.name === selected) || null;
  }

  function renderProjects() {
    setHtml("project-tabs", FactoryProjects.renderProjectTabs(projects, selected));
    setHtml("project-note", FactoryProjects.renderProjectNote(currentProject()));
  }

  // The buttons depend on the stage, so they are set on each job before the board is drawn.
  function withActions(state) {
    if (mode !== "home" || !state || !Array.isArray(state.jobs)) return state;
    for (const job of state.jobs) job.actions = FactoryProjects.jobActions(job);
    return state;
  }

  function render(state, error) {
    const view = state || lastState;
    if (view) {
      setHtml("summary", FactoryBoard.renderSummary(view));
      setHtml("board", FactoryBoard.renderBoard(view));
      setHtml("costs", FactoryBoard.renderCostPanel(view.costs, view.generated_at));
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
      const response = await fetch(parkUrl(id), { cache: "no-store" });
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
    if (target.closest("[data-banner-close]")) {
      hideBanner();
      return;
    }
    const tab = target.closest("[data-project]");
    if (tab) {
      selectTab(tab.getAttribute("data-project") || "");
      return;
    }
    const act = target.closest("[data-act]");
    if (act) {
      startAction(act.getAttribute("data-act") || "", act.getAttribute("data-act-job") || "", act);
      return;
    }
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

  // Settles the mode on the first answer. Returns false while the answer is still unknown.
  async function loadProjects() {
    if (mode === "root") return true;
    let response;
    try {
      response = await fetch("/api/projects", { cache: "no-store" });
    } catch (error) {
      return false;
    }
    if (response.status === 404 && mode === "unknown") {
      mode = "root";
      return true;
    }
    let data = null;
    try {
      data = await response.json();
    } catch (error) {
      data = null;
    }
    if (!response.ok) {
      showBanner(FactoryProjects.errorText(data, response.status), true);
      return false;
    }
    if (mode !== "home") enterHomeMode();
    projects = Array.isArray(data) ? data : [];
    const next = FactoryProjects.selectProject(projects, selected);
    if (next !== selected) {
      selected = next;
      lastState = null;
    }
    if (selected) storeSelected(selected);
    renderProjects();
    return true;
  }

  function storeSelected(name) {
    try {
      sessionStorage.setItem(STORE_KEY, name);
    } catch (error) {
      // Storage may be blocked. The choice then lasts until the page reloads.
    }
  }

  function enterHomeMode() {
    mode = "home";
    try {
      selected = sessionStorage.getItem(STORE_KEY);
    } catch (error) {
      selected = null;
    }
    document.title = "Software factory";
    byId("project-suffix").hidden = true;
    byId("newjob").hidden = false;
  }

  function selectTab(name) {
    if (!name || name === selected) return;
    selected = name;
    storeSelected(name);
    lastState = null;
    hideBanner();
    renderProjects();
    tick();
  }

  async function tick() {
    if (inFlight) return;
    inFlight = true;
    let state;
    try {
      if (!(await loadProjects())) {
        render(null, "the project list is not available");
        return;
      }
      if (mode === "home" && !selected) {
        render(null, null);
        return;
      }
      const response = await fetch(stateUrl(), { cache: "no-store" });
      if (!response.ok) {
        let data = null;
        try {
          data = await response.json();
        } catch (error) {
          data = null;
        }
        if (mode === "home") showBanner(FactoryProjects.errorText(data, response.status), true);
        throw new Error(`HTTP ${response.status}`);
      }
      state = withActions(await response.json());
    } catch (error) {
      render(null, error.message);
      return;
    } finally {
      inFlight = false;
    }
    lastState = state;
    lastOk = state.generated_at;
    if (pollBanner) hideBanner();
    render(state, null);
  }

  // Writes go through the dashboard server, which adds the token. The page never holds one.
  async function postApi(path, body) {
    const options = { method: "POST", headers: { "Content-Type": "application/json" } };
    if (body !== undefined) options.body = JSON.stringify(body);
    const response = await fetch(path, options);
    let data = null;
    try {
      data = await response.json();
    } catch (error) {
      data = null;
    }
    if (!response.ok) throw new Error(FactoryProjects.errorText(data, response.status));
    return data;
  }

  function jobPath(id, action) {
    return `/api/projects/${encodeURIComponent(selected)}/jobs/${encodeURIComponent(id)}/${action}`;
  }

  async function runWrite(button, work, done) {
    if (button) button.disabled = true;
    try {
      const data = await work();
      hideBanner();
      showToast(done(data));
      tick();
      return true;
    } catch (error) {
      showBanner(error.message);
      return false;
    } finally {
      if (button) button.disabled = false;
    }
  }

  const actDialog = byId("act-dialog");
  let actKind = "";
  let actJob = "";

  function startAction(kind, id, button) {
    if (mode !== "home" || !selected || !FactoryProjects.isSafeId(id)) return;
    if (kind === "approve") {
      runWrite(button, () => postApi(jobPath(id, "approve")), () => `Approved job ${id}`);
      return;
    }
    if (kind !== "rework" && kind !== "cancel") return;
    actKind = kind;
    actJob = id;
    byId("act-title").textContent = kind === "rework" ? `Rework job ${id}` : `Cancel job ${id}`;
    byId("act-note-label").firstChild.textContent = kind === "rework" ? "Note for the builder " : "Reason (optional) ";
    byId("act-force-label").hidden = kind !== "cancel";
    byId("act-note").value = "";
    byId("act-force").checked = false;
    actDialog.showModal();
    byId("act-note").focus();
  }

  byId("act-close").addEventListener("click", () => actDialog.close());
  byId("act-submit").addEventListener("click", async () => {
    const note = byId("act-note").value;
    let body;
    if (actKind === "rework") body = FactoryProjects.buildReworkBody(note);
    else body = FactoryProjects.buildCancelBody(note, byId("act-force").checked);
    if (body.error) {
      showBanner(body.error);
      return;
    }
    const kind = actKind;
    const id = actJob;
    const ok = await runWrite(byId("act-submit"), () => postApi(jobPath(id, kind), body), () => (kind === "rework" ? `Sent job ${id} back` : `Cancelled job ${id}`));
    if (ok) actDialog.close();
  });

  byId("nj-submit").addEventListener("click", async () => {
    const body = FactoryProjects.buildCreateBody({
      spec: byId("nj-spec").value,
      plans: byId("nj-plans").value,
      model: byId("nj-model").value,
      slug: byId("nj-slug").value,
      run: byId("nj-run").checked,
      series: byId("nj-series").checked,
    });
    if (body.error) {
      showBanner(body.error);
      return;
    }
    const path = `/api/projects/${encodeURIComponent(selected)}/jobs`;
    const ok = await runWrite(byId("nj-submit"), () => postApi(path, body), (data) => `Created job ${data && data.job ? data.job : ""}`.trim());
    if (ok) {
      byId("nj-spec").value = "";
      byId("nj-plans").value = "";
      byId("nj-slug").value = "";
    }
  });

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
