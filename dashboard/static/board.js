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
  const ROLE_LABEL = { security: "security", ux: "ux", ui: "ui", code: "code" };
  const MAX_ROUNDS = 2;
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
    if (!Number.isFinite(ts)) return "--:--:--";
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

  function agentLine(agent, now) {
    const status = AGENT_STATUSES.includes(agent.status) ? agent.status : "done";
    let detail = "";
    if (status !== "done" && agent.last_tool) {
      const target = agent.last_target ? ` ${agent.last_target}` : "";
      detail = `${agent.last_tool}${target}`;
    } else if (status === "done" && agent.last_message) {
      detail = truncate(agent.last_message, 400);
    }
    const count = agent.tool_count
      ? `${agent.tool_count} ${agent.tool_count === 1 ? "tool" : "tools"}`
      : "";
    const end = agent.ended_ts || now;
    const span =
      Number.isFinite(agent.started_ts) && Number.isFinite(end) && end >= agent.started_ts
        ? formatAge(end - agent.started_ts)
        : "";
    return (
      `<li class="agent agent-${status}"><span class="dot dot-${status}" aria-hidden="true"></span>` +
      `<span class="agent-kind">${escapeHtml(shortType(agent.agent_type))}</span>` +
      `<span class="agent-status">${status}</span>` +
      (span ? `<span class="agent-time">${escapeHtml(span)}</span>` : "") +
      (count ? `<span class="agent-count">${escapeHtml(count)}</span>` : "") +
      (detail ? `<span class="agent-detail">${escapeHtml(detail)}</span>` : "") +
      `</li>`
    );
  }

  function fact(label, value, extra) {
    if (value === null || value === undefined || value === "") return "";
    return `<dt>${label}</dt><dd${extra ? ` class="${extra}"` : ""}>${escapeHtml(value)}</dd>`;
  }

  function roundPips(round) {
    const done = Math.min(Math.max(Number(round) || 0, 0), MAX_ROUNDS);
    let pips = "";
    for (let i = 0; i < MAX_ROUNDS; i++) {
      pips += `<span class="pip${i < done ? " pip-on" : ""}"></span>`;
    }
    return pips;
  }

  const TASK_STATUSES = ["pending", "active", "review", "done"];
  const TASK_LABEL = { pending: "waiting", active: "building", review: "in review", done: "done" };
  // Job ids whose agent list the user has opened. It survives the one-second re-render.
  const openAgents = new Set();
  // The user's own choice for each job's task list: true is open, false is closed. Without a choice the
  // list is open while the job is running and closed once it has merged.
  const taskChoice = new Map();

  function taskStatus(task) {
    return TASK_STATUSES.includes(task.status) ? task.status : "pending";
  }

  // One segment per task, so the bar reads as "which of the plan's tasks are finished".
  function progressBar(job) {
    const tasks = (job.plan && job.plan.tasks) || [];
    if (!tasks.length) {
      const text = job.stage === "spec" ? "Plan is being written" : "No task list found";
      return `<div class="progress progress-none"><span class="bar" aria-hidden="true"><span class="seg"></span></span><span class="progress-label">${text}</span></div>`;
    }
    const done = tasks.filter((task) => taskStatus(task) === "done").length;
    const segs = tasks.map((task) => `<span class="seg seg-${taskStatus(task)}"></span>`).join("");
    return (
      `<div class="progress"><span class="bar" role="img" aria-label="${done} of ${tasks.length} tasks done">${segs}</span>` +
      `<span class="progress-label">${done}/${tasks.length} ${plural(tasks.length, "task", "tasks")} done</span></div>`
    );
  }

  function planSection(job) {
    const plan = job.plan;
    if (!plan) return "";
    const goal = plan.goal ? `<p class="plan-goal">${escapeHtml(plan.goal)}</p>` : "";
    const tasks = (plan.tasks || [])
      .map((task) => {
        const status = taskStatus(task);
        return (
          `<li class="task task-${status}"><span class="task-mark" aria-hidden="true"></span>` +
          `<span class="task-title"><span class="task-num">Task ${Number(task.number) || 0}</span> ${escapeHtml(task.title)}</span>` +
          `<span class="task-state">${TASK_LABEL[status]}</span></li>`
        );
      })
      .join("");
    const heading = plan.title ? `<p class="plan-title">${escapeHtml(plan.title)}</p>` : "";
    if (!heading && !goal && !tasks) return "";
    return `<div class="plan">${heading}${goal}${tasks ? tasksBox(job, plan.tasks, tasks) : ""}</div>`;
  }

  // The task list folds like the agent list. Only the list folds, the title and goal stay visible.
  function tasksBox(job, taskList, items) {
    const done = taskList.filter((task) => taskStatus(task) === "done").length;
    const open = (taskChoice.has(job.id) ? taskChoice.get(job.id) : job.stage !== "merged") ? " open" : "";
    return (
      `<details class="tasks-box" data-tasks="${escapeHtml(job.id)}"${open}>` +
      `<summary>Tasks <span class="agents-sum">${done}/${taskList.length} done</span></summary>` +
      `<ol class="tasks">${items}</ol></details>`
    );
  }

  function agentsSection(job, agents, now) {
    if (!agents.length) return "";
    const live = agents.filter((agent) => agent.status !== "done");
    const finished = agents.length - live.length;
    const parts = [];
    if (live.length) parts.push(`${live.length} working`);
    if (finished) parts.push(`${finished} finished`);
    const open = (live.length > 0 && job.stage !== "merged") || openAgents.has(job.id) ? " open" : "";
    return (
      `<details class="agents-box" data-agents="${escapeHtml(job.id)}"${open}>` +
      `<summary>Agent activity <span class="agents-sum">${parts.join(", ")}</span></summary>` +
      `<ul class="agents">${agents.map((agent) => agentLine(agent, now)).join("")}</ul></details>`
    );
  }

  function copyChip(command) {
    const safe = escapeHtml(command);
    return `<button type="button" class="cmd" data-copy="${safe}" title="Copy to the clipboard">${safe}</button>`;
  }

  function seriesChip(series) {
    if (!series) return "";
    const total = Number(series.total);
    const index = Number(series.index);
    if (!Number.isFinite(total) || !Number.isFinite(index) || total < 2) return "";
    const k = Math.min(Math.max(index, 1), total);
    return `<span class="series" title="Plan ${k} of ${total} in this series">plan ${k} of ${total}</span>`;
  }

  function durationsLine(job) {
    const known = (job.stage_durations || []).filter(
      (item) => item && STAGES.some((s) => s.key === item.stage) && Number.isFinite(item.seconds),
    );
    if (!known.length) return "";
    const parts = known.map((item) => {
      const title = STAGES.find((s) => s.key === item.stage).title;
      return `<span class="duration${item.current ? " duration-now" : ""}">${title} ${formatAge(item.seconds)}</span>`;
    });
    return `<p class="durations" title="Time spent in each stage">${parts.join("")}</p>`;
  }

  function formatTokens(count) {
    const n = Number(count);
    if (!Number.isFinite(n) || n < 0) return "0";
    if (n >= 1e6) return `${Number((n / 1e6).toFixed(1))}M`;
    if (n >= 1e3) return `${Math.round(n / 1e3)}K`;
    return String(Math.round(n));
  }

  // The bar is capped at 100 percent. The level is also written as text, never colour alone.
  // The width comes from data-pct and is applied by app.js, because the CSP forbids inline styles.
  function tokensBar(job) {
    const view = job.token_view;
    if (!view || !Number.isFinite(view.weighted)) return "";
    const fraction = Number.isFinite(view.fraction) ? Math.min(Math.max(view.fraction, 0), 1) : 0;
    const level = fraction >= 1 ? "stop" : fraction >= 0.8 ? "warn" : "ok";
    const percent = Math.round(fraction * 100);
    const budget = Number.isFinite(view.budget) && view.budget > 0 ? ` of ${formatTokens(view.budget)}` : "";
    return (
      `<div class="tokens tokens-${level}" title="Weighted tokens against the job budget. Raw: ${escapeHtml(formatTokens(view.raw))}">` +
      `<span class="tokens-text">${formatTokens(view.weighted)}${budget} tokens (${percent}%${level === "ok" ? "" : `, ${level}`})</span>` +
      `<span class="tokens-bar" aria-hidden="true"><span class="tokens-fill" data-pct="${percent}"></span></span></div>`
    );
  }

  function announcement(header) {
    return header.level === "ok" ? "Dashboard is live" : header.text;
  }

  function renderCard(job, now) {
    const id = escapeHtml(job.id);
    const stage = STAGES.some((s) => s.key === job.stage) ? job.stage : "spec";
    const stageTitle = (STAGES.find((s) => s.key === stage) || STAGES[0]).title;
    const chips = CHIP_STAGES.includes(job.stage)
      ? `<div class="chips">${ROLES.map((role) => reviewChip(role, (job.reviews || {})[role])).join("")}</div>`
      : "";
    const reason =
      job.stage === "needs-human"
        ? `<p class="reason">${escapeHtml(job.reason || "needs a human")}</p>` +
          `<p class="hint">${copyChip(`factory approve ${job.id}`)}${copyChip(`factory rework ${job.id} <note>`)}${copyChip(`factory cancel ${job.id}`)}</p>`
        : "";
    const showAge = job.stage !== "merged" && job.stage !== "needs-human";
    const facts =
      `<details class="facts-box"><summary>Branch and files</summary><dl class="facts">` +
      fact("branch", job.branch, "fact-path") +
      fact("worktree", job.worktree, "fact-path") +
      fact("plan", job.dir, "fact-path") +
      fact("builder", job.builder_id) +
      fact("slug", job.slug) +
      `</dl></details>`;
    return (
      `<article class="card card-${stage}" data-job="${id}">` +
      `<header class="card-head"><span class="job-id">${id}</span>` +
      `<span class="card-stage">${escapeHtml(stageTitle)}</span>` +
      seriesChip(job.series) +
      `<time class="job-date">${escapeHtml(job.created)}</time></header>` +
      `<p class="feature">${escapeHtml(job.feature)}</p>` +
      progressBar(job) +
      `<div class="meta"><span class="round" title="builder rounds used">` +
      `<span class="pips" aria-hidden="true">${roundPips(job.round)}</span>round ${Number(job.round) || 0}/${MAX_ROUNDS}</span>` +
      (showAge ? `<span class="age">${formatAge(job.seconds_in_stage)} in stage</span>` : "") +
      `</div>` +
      planSection(job) +
      durationsLine(job) +
      tokensBar(job) +
      chips +
      agentsSection(job, job.agents || [], now) +
      facts +
      reason +
      `</article>`
    );
  }

  function renderBoard(state) {
    state = state || {};
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
        ? items.map((job) => renderCard(job, state.generated_at)).join("")
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
    const strays = jobs.filter((job) => !known.has(job.stage));
    const strayColumn = strays.length
      ? `<section class="column panel column-unknown"><h2>Unknown stage<span class="count">${strays.length}</span></h2>` +
        strays.map((job) => renderCard(job, state.generated_at)).join("") +
        `</section>`
      : "";
    return columns.join("") + strayColumn + warning;
  }

  function renderUnassigned(state) {
    const agents = state.unassigned_agents || [];
    if (!agents.length) return "";
    return (
      `<section class="panel"><h2>Agents without a job</h2>` +
      `<ul class="agents">${agents.map((agent) => agentLine(agent, state.generated_at)).join("")}</ul></section>`
    );
  }

  function renderFeed(state) {
    state = state || {};
    const recent = (state.recent || []).slice(0, FEED_LIMIT);
    const skipped = Number(state.skipped_lines) || 0;
    const note = skipped ? `<p class="warning">${skipped} event log ${plural(skipped, "line", "lines")} skipped</p>` : "";
    if (!recent.length) {
      return `<section class="panel"><h2>Recent activity</h2><p class="empty">No agent activity yet.</p>${note}</section>`;
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
    return `<section class="panel"><h2>Recent activity</h2><ol class="feed">${rows.join("")}</ol>${note}</section>`;
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
      `<span class="summary-actions">${copyChip(`factory approve ${job.id}`)}${copyChip(`factory rework ${job.id} <note>`)}</span></li>`
    );
  }

  // The first thing on the page: does anything need the user right now?
  function renderSummary(state) {
    state = state || {};
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
    openAgents,
    taskChoice,
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
    copyChip,
    seriesChip,
    durationsLine,
    tokensBar,
    announcement,
  };

  if (typeof module !== "undefined" && module.exports) {
    module.exports = api;
  } else {
    root.FactoryBoard = api;
  }
})(typeof window !== "undefined" ? window : globalThis);
