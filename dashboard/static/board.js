"use strict";

(function (root) {
  const STAGES = [
    { key: "spec", title: "Spec", sub: "Planned in your session" },
    { key: "build", title: "Build", sub: "Builder writes the code" },
    { key: "review", title: "Review", sub: "Checks run, then one reviewer" },
    { key: "approve", title: "Approve", sub: "Merging the reviewed commit" },
    { key: "merged", title: "Merged", sub: "Landed on main" },
    { key: "needs-human", title: "Needs human", sub: "Waiting for your call" },
  ];
  const ROLES = ["security", "ux", "ui", "code"];
  const ROLE_LABEL = { security: "security", ux: "ux", ui: "ui", code: "code" };

  // The roles that review this job, in the usual order. A v3 job has only `code`, and a job with no
  // review state yet shows the one review that will run.
  function reviewRoles(job) {
    const have = Object.keys((job && job.reviews) || {});
    const known = ROLES.filter((role) => have.includes(role));
    return known.length ? known : ["code"];
  }
  const MAX_ROUNDS = 2;
  const REVIEW_STATES = ["pending", "running", "pass", "changes", "skipped"];
  const AGENT_STATUSES = ["running", "stale", "done"];
  const CHIP_STAGES = ["review", "approve", "needs-human", "merged"];
  const MERGED_LIMIT = 10;
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

  // The driver keeps only the last 300 characters of a message, so a long one starts in the middle of
  // a word. Drop the broken word and mark the cut.
  const CUT_LENGTH = 300;
  function cleanSnippet(text) {
    const value = String(text || "");
    if (value.length < CUT_LENGTH) return value;
    const space = value.search(/\s/);
    if (space < 0) return `… ${value}`;
    return `… ${value.slice(space).trim()}`;
  }

  function agentLine(agent, now) {
    const status = AGENT_STATUSES.includes(agent.status) ? agent.status : "done";
    let detail = "";
    if (status !== "done" && agent.last_tool) {
      const target = agent.last_target ? ` ${agent.last_target}` : "";
      detail = `${agent.last_tool}${target}`;
    } else if (status === "done" && agent.last_message) {
      detail = truncate(cleanSnippet(agent.last_message), 400);
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

  // Plan text is Markdown, so `name` becomes <code>. The text is escaped before the tags are added.
  function inlineCode(text) {
    return escapeHtml(text).replace(/`([^`]+)`/g, "<code>$1</code>");
  }

  function planSection(job) {
    const plan = job.plan;
    if (!plan) return "";
    const goal = plan.goal ? `<p class="plan-goal">${inlineCode(plan.goal)}</p>` : "";
    const tasks = (plan.tasks || [])
      .map((task) => {
        const status = taskStatus(task);
        return (
          `<li class="task task-${status}"><span class="task-mark" aria-hidden="true"></span>` +
          `<span class="task-title"><span class="task-num">Task ${Number(task.number) || 0}</span> ${inlineCode(task.title)}</span>` +
          `<span class="task-state">${TASK_LABEL[status]}</span></li>`
        );
      })
      .join("");
    const heading = plan.title ? `<p class="plan-title">${inlineCode(plan.title)}</p>` : "";
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

  // How a plan row of the spec card looks. Each state maps to the task row class and the segment class.
  const SPEC_ROW = {
    waiting: { row: "pending", seg: "pending", label: "waiting" },
    building: { row: "active", seg: "active", label: "building" },
    review: { row: "review", seg: "review", label: "in review" },
    "needs-human": { row: "active", seg: "active", label: "needs human" },
    merged: { row: "done", seg: "done", label: "merged" },
    cancelled: { row: "cancelled", seg: "pending", label: "cancelled" },
  };

  function specRow(plan) {
    return Object.prototype.hasOwnProperty.call(SPEC_ROW, plan.state) ? SPEC_ROW[plan.state] : SPEC_ROW.waiting;
  }

  // One card for a spec and its plans. It reuses the task list markup, with plans where tasks would be.
  function renderSpecCard(spec) {
    const plans = spec.plans || [];
    const id = escapeHtml(spec.series_id);
    const stage = STAGES.some((s) => s.key === spec.stage) ? spec.stage : "spec";
    const done = plans.filter((plan) => plan.state === "merged").length;
    const segs = plans.map((plan) => `<span class="seg seg-${specRow(plan).seg}"></span>`).join("");
    const rows = plans
      .map((plan) => {
        const look = specRow(plan);
        const title = plan.title ? ` ${inlineCode(plan.title)}` : "";
        return (
          `<li class="task task-${look.row}"><span class="task-mark" aria-hidden="true"></span>` +
          `<span class="task-title"><span class="task-num">Plan ${Number(plan.index) || 0}</span>${title}</span>` +
          `<span class="task-state">${look.label}</span></li>`
        );
      })
      .join("");
    const key = `spec-${spec.series_id}`;
    const open = (taskChoice.has(key) ? taskChoice.get(key) : stage !== "merged") ? " open" : "";
    const goal = spec.goal ? `<p class="plan-goal">${inlineCode(spec.goal)}</p>` : "";
    return (
      `<article class="card card-${stage}" data-spec="${id}">` +
      `<header class="card-head"><span class="job-id">${id}</span><span class="card-stage">Spec</span>` +
      `<span class="series">${plans.length} ${plural(plans.length, "plan", "plans")}</span></header>` +
      `<p class="feature">${escapeHtml(spec.title || `Spec ${spec.series_id}`)}</p>` +
      `<div class="progress"><span class="bar" role="img" aria-label="${done} of ${plans.length} plans done">${segs}</span>` +
      `<span class="progress-label">${done}/${plans.length} ${plural(plans.length, "plan", "plans")} done</span></div>` +
      `<div class="plan">${goal}` +
      `<details class="tasks-box" data-tasks="${escapeHtml(key)}"${open}>` +
      `<summary>Plans <span class="agents-sum">${done}/${plans.length} done</span></summary>` +
      `<ol class="tasks">${rows}</ol></details></div>` +
      `</article>`
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

  // Says which spec a plan belongs to. Jobs made before series ids, and one-plan series, have none.
  function specChip(series, specs) {
    if (!series || !series.id) return "";
    const total = Number(series.total);
    if (!Number.isFinite(total) || total < 2) return "";
    const spec = (specs || []).find((item) => item && item.series_id === series.id);
    const label = spec && spec.title ? `Spec ${series.id}: ${spec.title}` : `Spec ${series.id}`;
    return `<span class="series spec-chip" title="${escapeHtml(label)}">spec ${escapeHtml(series.id)}</span>`;
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

  const PARK_LABEL = {
    "checks-failed": "Checks failed",
    "changes-remain": "Changes remain",
    "risky-diff": "Risky diff",
    "diff-too-large": "Diff too large",
    "budget-stopped": "Budget stopped",
    "builder-stopped": "Builder stopped",
    "builder-committed": "Builder committed",
    "nothing-changed": "Nothing changed",
    "commit-failed": "Commit failed",
    other: "Needs a human",
  };
  const PARK_RED = ["checks-failed", "changes-remain", "builder-stopped", "commit-failed"];
  // A line that starts with an error word, ends a failed test, or reports a panic is a line that matters.
  const PARK_HIT = /^\s*(error|FAIL|\[blocker\])|\sFAILED\s*$|panicked at/;

  function parkKindLabel(kind) {
    return PARK_LABEL[kind] || PARK_LABEL.other;
  }

  function parkKindClass(kind) {
    return PARK_RED.includes(kind) ? "chip-changes" : "chip-warn";
  }

  // One block-level span per line. Empty lines hold a space so they keep their height.
  function markLines(text) {
    const lines = String(text === null || text === undefined ? "" : text).split("\n");
    if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
    return lines
      .map((line) => `<span class="ln${PARK_HIT.test(line) ? " ln-hit" : ""}">${line === "" ? " " : escapeHtml(line)}</span>`)
      .join("");
  }

  function parkHeader(job, kindHtml, whenHtml, title) {
    return (
      `<header class="park-head"><div class="park-meta"><span class="job-id">${escapeHtml(job.id)}</span>${kindHtml}${whenHtml}</div>` +
      `<h2 id="park-title" class="park-title">${escapeHtml(title)}</h2>` +
      `<button type="button" class="cmd park-close" data-park-close>Close</button></header>`
    );
  }

  function renderParkPart(part) {
    const cut = part.truncated
      ? `<p class="park-cut">The start of this text was cut. Only the last part is shown.</p>`
      : "";
    return (
      `<section class="park-part"><h3 class="park-part-title">${escapeHtml(part.title)} <span class="park-source">${escapeHtml(part.source)}</span></h3>` +
      cut +
      `<pre class="park-text" tabindex="0" aria-label="${escapeHtml(part.title)}">${markLines(part.text)}</pre></section>`
    );
  }

  function renderParkDialog(job, view, now) {
    const kind = String(view.kind || "other");
    const kindHtml = `<span class="chip ${parkKindClass(kind)}">${escapeHtml(parkKindLabel(kind))}</span>`;
    const whenHtml =
      Number(view.at) > 0
        ? `<time class="park-when">parked ${escapeHtml(formatAge(Math.max(0, now - Number(view.at))))} ago</time>`
        : "";
    const parts = (view.parts || []).map(renderParkPart).join("");
    const next = (view.next || []).map(copyChip).join("");
    return (
      parkHeader(job, kindHtml, whenHtml, view.reason || job.reason || "Needs a human") +
      `<p class="park-note" role="status" hidden>This job is no longer waiting for you. The details below are from when it parked.</p>` +
      `<div class="park-body">${parts || `<p class="park-empty">No evidence was recorded for this park.</p>`}</div>` +
      (next ? `<footer class="park-foot"><p class="park-next-label">What to do next</p><p class="hint">${next}</p></footer>` : "")
    );
  }

  // Loading, empty and error states of the popup.
  function parkMessage(job, text, retry) {
    return (
      parkHeader(job, "", "", "What went wrong") +
      `<div class="park-body"><p class="park-empty">${escapeHtml(text)}</p>` +
      (retry ? `<p><button type="button" class="cmd" data-park-retry>Try again</button></p>` : "") +
      `</div>`
    );
  }

  function parkButton(job) {
    if (job.stage !== "needs-human" || !job.has_park) return "";
    return `<p class="park-link"><button type="button" class="cmd park-open" data-park="${escapeHtml(job.id)}">See what went wrong</button></p>`;
  }

  const GROUP_ORDER = ["small", "medium", "complex", "all"];
  const ROLE_CLASS = ["builder", "committer", "reviewer", "message", "kb"];
  const FEW_JOBS = 3;

  function jobsWord(n) {
    return n === 1 ? "1 job" : `${n} jobs`;
  }

  // The words that tell a reader how far to trust a figure.
  function estimateNotes(group, n, nApprox) {
    const notes = [group === "all" ? "all sizes" : group, jobsWord(n)];
    if (n < FEW_JOBS) notes.push("few jobs");
    if (nApprox > 0) notes.push(`${nApprox} approximate`);
    return notes.join(", ");
  }

  function isEstimate(e) {
    return Boolean(e) && typeof e === "object" && [e.median, e.low, e.high, e.n].every((v) => Number.isFinite(Number(v)));
  }

  // One line on a card: the estimate before the job merges, the actual cost against it afterwards.
  function estimateLine(job) {
    const e = job && job.estimate;
    if (!isEstimate(e)) return "";
    const range = `${formatTokens(e.low)} to ${formatTokens(e.high)}`;
    const actual = job.token_view && Number(job.token_view.weighted);
    if (job.stage === "merged" && Number.isFinite(actual) && actual > 0) {
      const word = actual > e.high ? "above" : actual < e.low ? "below" : "within";
      return `<p class="estimate">Cost ${formatTokens(actual)}, estimate ${range}: ${word} the range</p>`;
    }
    return `<p class="estimate">Estimated ${range} weighted (${escapeHtml(estimateNotes(String(e.group), Number(e.n), Number(e.n_approx) || 0))})</p>`;
  }

  function roleClass(role) {
    return ROLE_CLASS.includes(role) ? `role-${role}` : "role-other";
  }

  // A stacked bar of the role shares and a text legend. Widths are set from data-pct by app.js.
  function roleBar(roles) {
    const entries = Object.entries(roles || {})
      .filter(([, pct]) => Number.isFinite(Number(pct)) && Number(pct) > 0)
      .map(([role, pct]) => [role, Math.max(0, Math.min(100, Math.round(Number(pct))))]);
    if (!entries.length) return "";
    const label = entries.map(([role, pct]) => `${role} ${pct}%`).join(", ");
    const segs = entries
      .map(([role, pct]) => `<span class="role-seg ${roleClass(role)}" data-pct="${pct}"></span>`)
      .join("");
    const legend = entries
      .map(([role, pct]) => `<li><span class="role-key ${roleClass(role)}" aria-hidden="true"></span>${escapeHtml(role)} ${pct}%</li>`)
      .join("");
    return (
      `<div class="role-bar" role="img" aria-label="${escapeHtml(`Share of tokens by role: ${label}`)}">${segs}</div>` +
      `<ul class="role-legend">${legend}</ul>`
    );
  }

  function costRow(name, g) {
    const approx = Number(g.n_approx) > 0;
    const notes = [];
    if (Number(g.n) < FEW_JOBS) notes.push("few jobs");
    if (approx) notes.push(`${Number(g.n_approx)} of ${Number(g.n)} approximate`);
    const mark = approx
      ? ` <span class="approx" title="Output tokens were counted too low for some of these jobs, so the figure is a little low.">~</span>`
      : "";
    return (
      `<tr><th scope="row">${escapeHtml(name)}</th><td>${Number(g.n)}</td>` +
      `<td>${formatTokens(g.median)}${mark}</td><td>${formatTokens(g.low)} to ${formatTokens(g.high)}</td>` +
      `<td class="cost-notes">${escapeHtml(notes.join(", "))}</td></tr>`
    );
  }

  function renderCostPanel(costs, now) {
    const groups = (costs && costs.groups) || {};
    const names = [...GROUP_ORDER.filter((g) => groups[g]), ...Object.keys(groups).filter((g) => !GROUP_ORDER.includes(g))];
    const rows = names.filter((name) => groups[name] && Number(groups[name].n) > 0);
    if (!rows.length) {
      return (
        `<h2>Cost per job</h2><p class="cost-empty">No cost history yet. Run ${copyChip("factory costs")} after a job merges, ` +
        `or wait for the next merge.</p>`
      );
    }
    const age =
      costs && Number(costs.generated_at) > 0 && Number.isFinite(Number(now))
        ? `<span class="cost-updated">updated ${escapeHtml(formatAge(Math.max(0, Number(now) - Number(costs.generated_at))))} ago</span>`
        : "";
    const corrected = rows
      .filter((name) => groups[name].corrected && Number(groups[name].corrected.n) > 0)
      .map((name) => {
        const c = groups[name].corrected;
        return `<li>${escapeHtml(name)}: median ${formatTokens(c.median)} (${jobsWord(Number(c.n))})</li>`;
      });
    return (
      `<div class="cost-head"><h2>Cost per job</h2>${age}</div>` +
      `<div class="cost-scroll"><table class="cost-table"><caption class="sr-only">Weighted tokens per finished job, by plan size</caption>` +
      `<thead><tr><th scope="col">size</th><th scope="col">jobs</th><th scope="col">median</th><th scope="col">range</th><th scope="col">notes</th></tr></thead>` +
      `<tbody>${rows.map((name) => costRow(name, groups[name])).join("")}</tbody></table></div>` +
      (corrected.length ? `<p class="cost-note">Exact counts only</p><ul class="cost-corrected">${corrected.join("")}</ul>` : "") +
      `<h3 class="cost-sub">Where the tokens go</h3>${roleBar(costs && costs.roles)}`
    );
  }

  const ACTION_LABELS = { approve: "Approve", rework: "Rework", cancel: "Cancel" };

  // Buttons for home mode. The page sets job.actions; without it a card has no buttons.
  function actionButtons(job) {
    const list = (Array.isArray(job.actions) ? job.actions : []).filter((name) => ACTION_LABELS[name]);
    if (list.length === 0) return "";
    const id = escapeHtml(job.id);
    const project = escapeHtml(job.project);
    const buttons = list
      .map((name) => `<button type="button" class="act act-${name}" data-act="${name}" data-act-job="${id}" data-act-project="${project}">${ACTION_LABELS[name]}</button>`)
      .join("");
    return `<div class="job-actions">${buttons}</div>`;
  }

  function renderCard(job, now, specs) {
    const id = escapeHtml(job.id);
    const stage = STAGES.some((s) => s.key === job.stage) ? job.stage : "spec";
    const stageTitle = (STAGES.find((s) => s.key === stage) || STAGES[0]).title;
    const chips = CHIP_STAGES.includes(job.stage)
      ? `<div class="chips">${reviewRoles(job).map((role) => reviewChip(role, (job.reviews || {})[role])).join("")}</div>`
      : "";
    const reason =
      job.stage === "needs-human"
        ? `<p class="reason">${escapeHtml(job.reason || "needs a human")}</p>` +
          parkButton(job) +
          `<p class="hint">${copyChip(`factory approve ${job.id}`)}${copyChip(`factory rework ${job.id} <note>`)}${copyChip(`factory cancel ${job.id}`)}</p>`
        : "";
    const showAge = job.stage !== "merged" && job.stage !== "needs-human";
    // A plan that has not started has no time, tokens, agents or branch yet, so those sections stay out.
    const parked = job.stage === "spec";
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
      specChip(job.series, specs) +
      `<time class="job-date">${escapeHtml(job.created)}</time></header>` +
      `<p class="feature">${escapeHtml(job.feature)}</p>` +
      progressBar(job) +
      `<div class="meta"><span class="round" title="builder rounds used">` +
      `<span class="pips" aria-hidden="true">${roundPips(job.round)}</span>round ${Number(job.round) || 0}/${MAX_ROUNDS}</span>` +
      (showAge ? `<span class="age">${formatAge(job.seconds_in_stage)} in stage</span>` : "") +
      `</div>` +
      planSection(job) +
      estimateLine(job) +
      (parked ? "" : durationsLine(job)) +
      (parked ? "" : tokensBar(job)) +
      chips +
      (parked ? "" : agentsSection(job, job.agents || [], now)) +
      (parked ? "" : facts) +
      reason +
      actionButtons(job) +
      `</article>`
    );
  }

  function renderBoard(state) {
    state = state || {};
    const jobs = state.jobs || [];
    const known = new Set(STAGES.map((s) => s.key));
    const unknown = jobs.filter((job) => !known.has(job.stage)).length;
    const specs = state.specs || [];
    const columns = STAGES.map((stage) => {
      let items = jobs.filter((job) => job.stage === stage.key);
      let specCards = specs.filter((spec) => spec && spec.stage === stage.key);
      const total = items.length + specCards.length;
      if (stage.key === "merged") {
        // Spec cards have no timestamp, so keep the last ones the server sent.
        specCards = specCards.slice(-MERGED_LIMIT);
        items = items
          .slice()
          .sort((a, b) => (b.stage_since || 0) - (a.stage_since || 0))
          .slice(0, MERGED_LIMIT);
      }
      const cards =
        items.length || specCards.length
          ? specCards.map(renderSpecCard).join("") +
            items.map((job) => renderCard(job, state.generated_at, specs)).join("")
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
        strays.map((job) => renderCard(job, state.generated_at, state.specs)).join("") +
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

  // A warning for log lines the server could not read, so a broken driver or a schema change is visible.
  function renderLogNotice(state) {
    const skipped = Number((state || {}).skipped_lines) || 0;
    if (!skipped) return "";
    return `<p class="warning">${skipped} event log ${plural(skipped, "line", "lines")} skipped</p>`;
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
    // A cancelled job can be neither approved nor reworked, only cleaned up.
    const actions =
      job.cancelled === true
        ? copyChip(`factory cleanup ${job.id}`)
        : `${copyChip(`factory approve ${job.id}`)}${copyChip(`factory rework ${job.id} <note>`)}`;
    return (
      `<li><span class="job-id">${id}</span>` +
      `<span class="summary-feature">${escapeHtml(job.feature)}</span>` +
      `<span class="summary-reason">${escapeHtml(job.reason || "Needs a human")}</span>` +
      `<span class="summary-actions">${actions}</span></li>`
    );
  }

  // The first thing on the page: does anything need the user right now?
  function renderSummary(state) {
    state = state || {};
    const jobs = state.jobs || [];
    const needs = jobs.filter((job) => job.stage === "needs-human");
    // Jobs parked at spec are plans that have not started, so they are not running.
    const waiting = jobs.filter((job) => job.stage === "spec").length;
    const active = jobs.filter(
      (job) =>
        job.stage !== "merged" &&
        job.stage !== "needs-human" &&
        job.stage !== "spec" &&
        STAGES.some((s) => s.key === job.stage),
    );
    const waitNote = waiting ? ` ${waiting} ${plural(waiting, "plan is", "plans are")} waiting.` : "";
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
        ? `${active.length} more ${plural(active.length, "job is", "jobs are")} still running.${waitNote}`
        : waiting
          ? waitNote.trim()
          : "Nothing else is running.";
      list = `<ul class="summary-jobs">${needs.slice(0, 3).map(summaryJob).join("")}</ul>`;
    } else if (active.length) {
      tone = "running";
      title = `${active.length} ${plural(active.length, "job is", "jobs are")} in progress`;
      detail = working
        ? `${working} ${plural(working, "agent is", "agents are")} working. Nothing needs you right now.`
        : "No agent is active at the moment. Nothing needs you right now.";
      detail += waitNote;
    } else if (waiting) {
      tone = "idle";
      title = `${waiting} ${plural(waiting, "plan is", "plans are")} waiting`;
      detail = "Run the next plan with <code>factory run &lt;id&gt;</code>.";
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
    actionButtons,
    renderSpecCard,
    renderSummary,
    renderBoard,
    renderUnassigned,
    renderLogNotice,
    headerStatus,
    copyChip,
    renderCostPanel,
    estimateLine,
    roleBar,
    parkKindLabel,
    parkKindClass,
    markLines,
    renderParkDialog,
    parkMessage,
    parkButton,
    seriesChip,
    specChip,
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
