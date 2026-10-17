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

  const api = {
    renderProjectTabs,
    renderProjectNote,
    selectProject,
    isSafeId,
    buildCreateBody,
    buildReworkBody,
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
