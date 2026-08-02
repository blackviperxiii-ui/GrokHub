/**
 * Always-on agent core — durable job queue in userData.
 * Scheduler ticks; UI drains jobs via IPC. Survives window hide.
 */
const fs = require("node:fs");
const path = require("node:path");

let app = null;
try {
  app = require("electron").app;
} catch {
  /* optional */
}

const FILE = "agent-queue.json";
const MAX_JOBS = 80;

/** @type {{ version: number, paused: boolean, level: number, jobs: any[], updatedAt: number }} */
let state = { version: 1, paused: false, level: 1, jobs: [], updatedAt: 0 };
let tickTimer = null;
/** @type {((payload: any) => void) | null} */
let onDue = null;

function userDir() {
  if (app) {
    try {
      if (app.isReady && app.isReady()) return app.getPath("userData");
      // before ready — still often works
      return app.getPath("userData");
    } catch {
      /* fall */
    }
  }
  const home = process.env.HOME || process.env.USERPROFILE || "/tmp";
  return path.join(home, ".config", "GrokHub");
}

function filePath() {
  return path.join(userDir(), FILE);
}

function load() {
  try {
    const raw = fs.readFileSync(filePath(), "utf8");
    const data = JSON.parse(raw);
    if (data && typeof data === "object") {
      state = {
        version: 1,
        paused: Boolean(data.paused),
        level: Math.min(4, Math.max(0, Number(data.level) || 1)),
        jobs: Array.isArray(data.jobs) ? data.jobs : [],
        updatedAt: Number(data.updatedAt) || 0,
      };
      // Reset stuck running
      state.jobs = state.jobs.map((j) =>
        j && j.status === "running" ? { ...j, status: "queued" } : j,
      );
    }
  } catch {
    /* fresh */
  }
}

function save() {
  try {
    fs.mkdirSync(userDir(), { recursive: true });
    state.updatedAt = Date.now();
    const tmp = filePath() + ".tmp";
    fs.writeFileSync(tmp, JSON.stringify(state, null, 0));
    fs.renameSync(tmp, filePath());
  } catch (e) {
    console.error("[agent-core] save failed", e);
  }
}

function snapshot() {
  return {
    ok: true,
    paused: state.paused,
    level: state.level,
    jobs: state.jobs.slice(0, MAX_JOBS),
    updatedAt: state.updatedAt,
    due: state.jobs.filter((j) => j.status === "queued" || j.status === "waiting_user").slice(0, 20),
  };
}

function enqueue(job) {
  if (!job || typeof job !== "object") return snapshot();
  const id = job.id || `job_${Date.now().toString(36)}`;
  const now = Date.now();
  const next = {
    ...job,
    id,
    status: job.status || "queued",
    createdAt: job.createdAt || now,
    updatedAt: now,
  };
  state.jobs = [next, ...state.jobs.filter((j) => j.id !== id)].slice(0, MAX_JOBS);
  save();
  return snapshot();
}

function sync(payload) {
  if (!payload || typeof payload !== "object") return snapshot();
  if (typeof payload.paused === "boolean") state.paused = payload.paused;
  if (payload.level != null) state.level = Math.min(4, Math.max(0, Number(payload.level) || 1));
  if (Array.isArray(payload.jobs)) {
    // Prefer longer list / newer updated
    state.jobs = payload.jobs.slice(0, MAX_JOBS);
  }
  save();
  return snapshot();
}

function setPaused(v) {
  state.paused = Boolean(v);
  save();
  return snapshot();
}

function approve(id, grant) {
  state.jobs = state.jobs.map((j) => {
    if (j.id !== id) return j;
    return {
      ...j,
      approval: grant ? "granted" : "denied",
      needsApproval: false,
      status: grant ? "queued" : "cancelled",
      updatedAt: Date.now(),
    };
  });
  save();
  return snapshot();
}

function tick() {
  if (state.paused || state.level < 2) return;
  const due = state.jobs.filter((j) => j.status === "queued" || j.status === "waiting_user");
  if (due.length && typeof onDue === "function") {
    try {
      onDue({ type: "due", count: due.length, jobs: due.slice(0, 5) });
    } catch {
      /* ignore */
    }
  }
}

function start({ intervalMs = 15000, onDueJobs } = {}) {
  load();
  if (onDueJobs) onDue = onDueJobs;
  if (tickTimer) clearInterval(tickTimer);
  tickTimer = setInterval(() => tick(), intervalMs);
  tick();
  return snapshot();
}

function stop() {
  if (tickTimer) clearInterval(tickTimer);
  tickTimer = null;
}

module.exports = {
  load,
  save,
  start,
  stop,
  snapshot,
  enqueue,
  sync,
  setPaused,
  approve,
  tick,
  getState: () => state,
};
