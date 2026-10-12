"use strict";

(function (root) {
  const STAGES = [
    { key: "spec", title: "Spec", sub: "Spec writer writes the plan" },
    { key: "build", title: "Build", sub: "Builder writes the code" },
    { key: "review", title: "Review", sub: "Four reviewers check it" },
    { key: "approve", title: "Approve", sub: "Approver decides" },
    { key: "merged", title: "Merged", sub: "Landed on main" },
    { key: "needs-human", title: "Needs human", sub: "Waiting for your call" },
  ];
  const ROLES = ["security", "ux", "ui", "code"];
  const ROLE_LABEL = { security: "sec", ux: "ux", ui: "ui", code: "code" };
  const REVIEW_STATES = ["pending", "running", "pass", "changes", "skipped"];
  const AGENT_STATUSES = ["running", "stale", "done"];
  const CHIP_STAGES = ["review", "approve", "needs-human", "merged"];
  const MERGED_LIMIT = 10;
  const FEED_LIMIT = 15;
  const ESCAPES = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };

  function escapeHtml(value) {
    return String(value === null || value === undefined ? "" : value).replace(
      /[&<>"']/g,
      (char) => ESCAPES[char],
    );
  }

  function truncate(text, max) {
    const value = String(text === null || text === undefined ? "" : text);
    return value.length > max ? `${value.slice(0, max - 1)}…` : value;
  }

  function formatAge(seconds) {
    if (!Number.isFinite(seconds) || seconds < 0) return "0s";
    if (seconds < 60) return `${Math.floor(seconds)}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    if (seconds < 86400) {
      const hours = Math.floor(seconds / 3600);
      const minutes = Math.floor((seconds % 3600) / 60);
      return `${hours}h ${String(minutes).padStart(2, "0")}m`;
    }
    return `${Math.floor(seconds / 86400)}d`;
  }

  function clock(ts) {
    return new Date(ts * 1000).toTimeString().slice(0, 8);
  }

  function shortType(agentType) {
    return String(agentType || "agent").replace(/^factory-/, "");
  }

  function reviewChip(role, state) {
    const safeState = REVIEW_STATES.includes(state) ? state : "pending";
    const label = ROLE_LABEL[role] || "?";
    return `<span class="chip chip-${safeState}" title="${escapeHtml(role)}: ${safeState}">${label} ${safeState}</span>`;
  }

  function agentLine(agent) {
    const status = AGENT_STATUSES.includes(agent.status) ? agent.status : "done";
    let detail = "";
    if (status !== "done" && agent.last_tool) {
      const target = agent.last_target ? ` ${truncate(agent.last_target, 48)}` : "";
      detail = `${agent.last_tool}${target}`;
    } else if (status === "done" && agent.last_message) {
      detail = truncate(agent.last_message, 60);
    }
    const count = agent.tool_count
      ? `${agent.tool_count} ${agent.tool_count === 1 ? "tool" : "tools"}`
      : "";
    return (
      `<li class="agent agent-${status}"><span class="dot dot-${status}" aria-hidden="true"></span>` +
      `<span class="agent-kind">${escapeHtml(shortType(agent.agent_type))}</span>` +
      `<span class="agent-status">${status}</span>` +
      (detail ? `<span class="agent-detail">${escapeHtml(detail)}</span>` : "") +
      (count ? `<span class="agent-count">${escapeHtml(count)}</span>` : "") +
      `</li>`
    );
  }

  function renderCard(job) {
    const id = escapeHtml(job.id);
    const stage = STAGES.some((s) => s.key === job.stage) ? job.stage : "spec";
    const chips = CHIP_STAGES.includes(job.stage)
      ? `<div class="chips">${ROLES.map((role) => reviewChip(role, (job.reviews || {})[role])).join("")}</div>`
      : "";
    const agents = (job.agents || []).map(agentLine).join("");
    const reason =
      job.stage === "needs-human"
        ? `<p class="reason">${escapeHtml(job.reason || "needs a human")}</p>` +
          `<p class="hint"><code>/factory approve ${id}</code> <code>/factory rework ${id} &lt;note&gt;</code></p>`
        : "";
    return (
      `<article class="card card-${stage}" data-job="${id}">` +
      `<header class="card-head"><span class="job-id">${id}</span>` +
      `<time class="job-date">${escapeHtml(job.created)}</time></header>` +
      `<p class="feature">${escapeHtml(job.feature)}</p>` +
      `<p class="branch">${escapeHtml(job.branch)}</p>` +
      `<div class="meta"><span class="round">round ${Number(job.round) || 0}/2</span>` +
      `<span class="age">${formatAge(job.seconds_in_stage)} in stage</span></div>` +
      chips +
      (agents ? `<ul class="agents">${agents}</ul>` : "") +
      reason +
      `</article>`
    );
  }

  function renderBoard(state) {
    const jobs = state.jobs || [];
    const known = new Set(STAGES.map((s) => s.key));
    const unknown = jobs.filter((job) => !known.has(job.stage)).length;
    const columns = STAGES.map((stage) => {
      let items = jobs.filter((job) => job.stage === stage.key);
      const total = items.length;
      if (stage.key === "merged") {
        items = items
          .slice()
          .sort((a, b) => (b.stage_since || 0) - (a.stage_since || 0))
          .slice(0, MERGED_LIMIT);
      }
      const cards = items.length
        ? items.map(renderCard).join("")
        : `<p class="empty">Nothing here</p>`;
      return (
        `<section class="column panel column-${stage.key}">` +
        `<h2>${stage.title}<span class="sub">${stage.sub}</span><span class="count">${total}</span></h2>` +
        cards +
        `</section>`
      );
    });
    const warning = unknown
      ? `<p class="warning">${unknown} job${unknown === 1 ? "" : "s"} with an unknown stage</p>`
      : "";
    return columns.join("") + warning;
  }

  function renderUnassigned(state) {
    const agents = state.unassigned_agents || [];
    if (!agents.length) return "";
    return (
      `<section class="panel"><h2>Agents without a job</h2>` +
      `<ul class="agents">${agents.map(agentLine).join("")}</ul></section>`
    );
  }

  function renderFeed(state) {
    const recent = (state.recent || []).slice(0, FEED_LIMIT);
    if (!recent.length) {
      return `<section class="panel"><h2>Recent activity</h2><p class="empty">No agent activity yet.</p></section>`;
    }
    const rows = recent.map((event) => {
      const what = [event.tool, event.target].filter(Boolean).join(" ");
      return (
        `<li><time>${clock(event.ts)}</time> ` +
        `<span class="feed-kind">${escapeHtml(event.kind)}</span> ` +
        `<span class="feed-agent">${escapeHtml(shortType(event.agent_type))}</span> ` +
        `<span class="feed-what">${escapeHtml(truncate(what, 70))}</span></li>`
      );
    });
    return `<section class="panel"><h2>Recent activity</h2><ol class="feed">${rows.join("")}</ol></section>`;
  }

  function headerStatus(state, error, lastOk) {
    if (error) {
      const since = lastOk ? `, last update ${clock(lastOk)}` : "";
      return { level: "error", text: `Server unreachable${since}` };
    }
    if (state && state.board_error) return { level: "warn", text: state.board_error };
    if (state) return { level: "ok", text: `Live, updated ${clock(state.generated_at)}` };
    return { level: "warn", text: "Connecting" };
  }

  function plural(count, one, many) {
    return count === 1 ? one : many;
  }

  function summaryJob(job) {
    const id = escapeHtml(job.id);
    return (
      `<li><span class="job-id">${id}</span>` +
      `<span class="summary-feature">${escapeHtml(job.feature)}</span>` +
      `<span class="summary-reason">${escapeHtml(job.reason || "Needs a human")}</span>` +
      `<span class="summary-actions"><code>/factory approve ${id}</code>` +
      `<code>/factory rework ${id} &lt;note&gt;</code></span></li>`
    );
  }

  // The first thing on the page: does anything need the user right now?
  function renderSummary(state) {
    const jobs = state.jobs || [];
    const needs = jobs.filter((job) => job.stage === "needs-human");
    const active = jobs.filter(
      (job) => job.stage !== "merged" && job.stage !== "needs-human" && STAGES.some((s) => s.key === job.stage),
    );
    const merged = jobs.filter((job) => job.stage === "merged").length;
    const working = jobs.reduce(
      (total, job) => total + (job.agents || []).filter((agent) => agent.status === "running").length,
      0,
    );
    let tone = "idle";
    let title = "No jobs yet";
    let detail = "Start one with <code>/factory &lt;feature&gt;</code>.";
    let list = "";
    if (needs.length) {
      tone = "attention";
      title = `${needs.length} ${plural(needs.length, "job needs", "jobs need")} you`;
      detail = active.length
        ? `${active.length} more ${plural(active.length, "job is", "jobs are")} still running.`
        : "Nothing else is running.";
      list = `<ul class="summary-jobs">${needs.slice(0, 3).map(summaryJob).join("")}</ul>`;
    } else if (active.length) {
      tone = "running";
      title = `${active.length} ${plural(active.length, "job is", "jobs are")} in progress`;
      detail = working
        ? `${working} ${plural(working, "agent is", "agents are")} working. Nothing needs you right now.`
        : "No agent is active at the moment. Nothing needs you right now.";
    } else if (jobs.length) {
      tone = "idle";
      title = "All caught up";
      detail = `${merged} ${plural(merged, "job", "jobs")} merged. Start the next one with <code>/factory &lt;feature&gt;</code>.`;
    }
    return (
      `<section class="summary summary-${tone}">` +
      `<h2 class="summary-title">${title}</h2><p class="summary-detail">${detail}</p>${list}</section>`
    );
  }

  const api = {
    STAGES,
    escapeHtml,
    formatAge,
    clock,
    reviewChip,
    agentLine,
    renderCard,
    renderSummary,
    renderBoard,
    renderUnassigned,
    renderFeed,
    headerStatus,
  };

  if (typeof module !== "undefined" && module.exports) {
    module.exports = api;
  } else {
    root.FactoryBoard = api;
  }
})(typeof window !== "undefined" ? window : globalThis);
