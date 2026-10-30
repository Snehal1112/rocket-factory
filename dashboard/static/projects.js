"use strict";

// Pure helpers for the project tabs, the New job form and the job actions. No DOM access here.
(function (root) {
  const ESCAPES = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
  const MODEL = /^[A-Za-z0-9._-]{1,64}$/;
  const SLUG = /^[a-z0-9][a-z0-9-]{0,63}$/;
  const NOTE_MAX = 200;

  function escapeHtml(value) {
    return String(value === null || value === undefined ? "" : value).replace(/[&<>"']/g, (char) => ESCAPES[char]);
  }

  function byName(list) {
    return (Array.isArray(list) ? list.slice() : []).sort((a, b) => String(a.name).localeCompare(String(b.name)));
  }

  function renderProjectTabs(list, selected) {
    const items = byName(list);
    if (items.length === 0) return "";
    const tabs = items
      .map((project) => {
        const disabled = project.status === "disabled";
        const busy = project.status === "busy";
        const classes = `tab${disabled ? " tab-disabled" : ""}${busy ? " tab-busy" : ""}`;
        const reason = project.reason ? ` title="${escapeHtml(project.reason)}"` : "";
        const flag = disabled ? ` <span class="tab-flag">disabled: ${escapeHtml(project.reason || "cannot take work")}</span>` : busy ? ` <span class="tab-flag">busy</span>` : "";
        const on = project.name === selected;
        return (
          `<button type="button" role="tab" class="${classes}" data-project="${escapeHtml(project.name)}"` +
          ` aria-selected="${on ? "true" : "false"}"${reason}>${escapeHtml(project.name)}${flag}</button>`
        );
      })
      .join("");
    return `<div class="tabs" role="tablist" aria-label="Projects">${tabs}</div>`;
  }

  // Keeps the current choice while it still exists, else the first project that can take work.
  function selectProject(list, current) {
    const items = byName(list);
    if (items.length === 0) return null;
    if (current && items.some((project) => project.name === current)) return current;
    const usable = items.find((project) => project.status !== "disabled");
    return (usable || items[0]).name;
  }

  function renderProjectNote(project) {
    if (!project || project.status !== "disabled") return "";
    return `<p class="warning">${escapeHtml(project.name)} cannot take work: ${escapeHtml(project.reason || "no reason given")}</p>`;
  }

  function isSafeId(id) {
    return typeof id === "string" && /^[0-9]{1,12}$/.test(id);
  }

  function absolutePath(value) {
    return value.startsWith("/") && !value.split("/").includes("..");
  }

  // Returns the request body, or { error } with a message for the user.
  function buildCreateBody(form) {
    const spec = String(form.spec || "").trim();
    const plans = String(form.plans || "").trim();
    const model = String(form.model || "").trim();
    const slug = String(form.slug || "").trim();
    if (!spec) return { error: "Enter the spec path." };
    if (!plans) return { error: "Enter the plans folder." };
    if (!absolutePath(spec)) return { error: "The spec path must be absolute and hold no '..'." };
    if (!absolutePath(plans)) return { error: "The plans folder must be absolute and hold no '..'." };
    if (model && !MODEL.test(model)) return { error: "The model may only use letters, digits, '.', '_' and '-' (64 at most)." };
    if (slug && !SLUG.test(slug)) return { error: "The slug uses lowercase letters, digits and '-', starts with a letter or digit (64 at most)." };
    const body = { spec, plans };
    if (model) body.model = model;
    if (slug) body.slug = slug;
    body.run = form.run !== false;
    body.series = form.series !== false;
    return body;
  }

  function buildReworkBody(note) {
    const text = String(note || "").trim();
    if (!text) return { error: "Write a note for the builder." };
    if (/[\r\n]/.test(text)) return { error: "The note must be one line." };
    if (text.length > NOTE_MAX) return { error: `The note is ${text.length} characters, ${NOTE_MAX} at most.` };
    return { note: text };
  }

  function buildWaiveBody(reason) {
    const text = String(reason || "").trim();
    if (!text) return { error: "Write the reason for merging with open blockers." };
    if (/[\r\n]/.test(text)) return { error: "The reason must be one line." };
    if (text.length > NOTE_MAX) return { error: `The reason is ${text.length} characters, ${NOTE_MAX} at most.` };
    return { waive_reason: text };
  }

  function buildCancelBody(reason, force) {
    const body = {};
    const text = String(reason || "").trim();
    if (text) body.reason = text;
    if (force) body.force = true;
    return body;
  }

  // The buttons a job card shows, in order.
  function jobActions(job) {
    if (!job || job.cancelled === true) return [];
    if (job.stage === "needs-human") return ["approve", "rework", "cancel"];
    if (job.stage === "approve") return ["approve", "cancel"];
    if (job.stage === "merged") return [];
    return ["cancel"];
  }

  function errorText(data, status) {
    const error = data && data.error;
    if (error && typeof error.code === "string") return `${error.code}: ${error.message || ""}`.trim();
    return `HTTP ${status}`;
  }

  // A reply belongs to the project its request was made for. After a tab switch it is dropped.
  function acceptReply(startedFor, selected) {
    return startedFor === selected;
  }

  // The API path of a job action, built from the project the button carries. Null for unsafe parts.
  function jobPathFor(project, id, action) {
    if (typeof project !== "string" || !/^[A-Za-z0-9._-]{1,64}$/.test(project) || project === "." || project === "..") return null;
    if (!isSafeId(id)) return null;
    if (action !== "approve" && action !== "rework" && action !== "cancel") return null;
    return `/api/projects/${encodeURIComponent(project)}/jobs/${id}/${action}`;
  }

  function count(value) {
    return String(Math.floor(value)).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  }

  // "2026-10-18 00:00 UTC" from an RFC 3339 time, or "" when it is not a time.
  function utcText(value) {
    if (typeof value !== "string") return "";
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return "";
    return `${date.toISOString().slice(0, 16).replace("T", " ")} UTC`;
  }

  // The daily budget strip from a /api/usage reply. Global: it never depends on the selected project.
  // The text is built from numbers and fixed words only, so it is safe to show with textContent.
  function budgetStrip(usage) {
    const none = { text: "", level: "none" };
    const budget = usage && usage.dailyBudget;
    if (!budget || typeof budget !== "object") return none;
    const { limit, used } = budget;
    if (typeof limit !== "number" || typeof used !== "number") return none;
    if (!Number.isFinite(limit) || !Number.isFinite(used) || limit <= 0 || used < 0) return none;
    const percent = Math.floor((used * 100) / limit);
    const reset = utcText(budget.resetsAt);
    const until = reset ? ` until ${reset}` : "";
    if (used >= limit) {
      return { text: `Daily budget used up: ${count(used)} of ${count(limit)} tokens. New runs are waiting${until}.`, level: "stop" };
    }
    const level = used * 100 >= limit * 80 ? "warn" : "ok";
    const resets = reset ? `, resets ${reset}` : "";
    return { text: `Daily budget: ${count(used)} of ${count(limit)} tokens used (${percent}%)${resets}.`, level };
  }

  // A version is shown only when it looks like one, so a strange reply never reaches the header.
  function versionText(value) {
    return typeof value === "string" && /^[0-9A-Za-z.+-]{1,32}$/.test(value) ? value : null;
  }

  // The header version from a /api/version reply. The text names the version that runs the jobs:
  // factoryd's in home mode, else the dashboard's. The title lists every version and why one is missing.
  function versionLabel(reply) {
    const none = { text: "", title: "", warn: false };
    if (!reply || typeof reply !== "object") return none;
    const dashboard = versionText(reply.dashboard);
    if (!dashboard) return none;
    const factoryd = versionText(reply.factoryd);
    const factory = versionText(reply.factory);
    const parts = [`dashboard ${dashboard}`];
    if (factoryd) parts.push(`factoryd ${factoryd}`);
    if (factory) parts.push(`factory ${factory}`);
    const known = [dashboard, factoryd, factory].filter(Boolean);
    const warn = known.some((v) => v !== known[0]);
    const notes = [];
    if (warn) notes.push("The versions differ. Restart factoryd and the dashboard after an update.");
    if (typeof reply.factoryError === "string") notes.push(`factory: ${reply.factoryError}`);
    if (typeof reply.apiError === "string") notes.push(reply.apiError);
    const title = [parts.join(", "), ...notes].join("\n");
    return { text: `v${factoryd || dashboard}`, title, warn };
  }

  const api = {
    versionLabel,
    budgetStrip,
    acceptReply,
    jobPathFor,
    renderProjectTabs,
    renderProjectNote,
    selectProject,
    isSafeId,
    buildCreateBody,
    buildReworkBody,
    buildWaiveBody,
    buildCancelBody,
    jobActions,
    errorText,
  };

  if (typeof module !== "undefined" && module.exports) {
    module.exports = api;
  } else {
    root.FactoryProjects = api;
  }
})(typeof window !== "undefined" ? window : globalThis);
