/**
 * Lightweight perf/debug helpers for the Electron main process.
 * Pure logic is unit-testable without Electron.
 */

function parseTrace(env = process.env) {
  const debug = env.GROKHUB_DEBUG === "1" || env.GROKHUB_DEBUG === "true";
  const raw = String(env.GROKHUB_TRACE || "")
    .toLowerCase()
    .split(/[,\s]+/)
    .filter(Boolean);
  const all = debug || raw.includes("all");
  return {
    debug: debug || raw.length > 0,
    boot: all || raw.includes("boot"),
    ipc: all || raw.includes("ipc"),
    stream: all || raw.includes("stream"),
    host: all || raw.includes("host"),
  };
}

function createBootTimeline(now = () => Date.now()) {
  const t0 = now();
  const phases = [];
  return {
    t0,
    mark(phase, extra) {
      const ms = now() - t0;
      const row = { phase: String(phase), ms, ...(extra && typeof extra === "object" ? extra : {}) };
      phases.push(row);
      return row;
    },
    snapshot() {
      return { t0, elapsedMs: now() - t0, phases: phases.slice() };
    },
  };
}

/**
 * Coalesce stream deltas: flush on maxWaitMs or when buffer exceeds maxChars.
 * flush(fn) sends accumulated text and clears.
 */
function createDeltaCoalescer(opts = {}) {
  const maxWaitMs = opts.maxWaitMs ?? 24;
  const maxChars = opts.maxChars ?? 48;
  const schedule =
    opts.schedule ||
    ((fn, ms) => {
      const id = setTimeout(fn, ms);
      return () => clearTimeout(id);
    });
  let buf = "";
  let cancel = null;
  let deltaCount = 0;
  let charCount = 0;
  let flushCount = 0;

  const clearTimer = () => {
    if (cancel) {
      try {
        cancel();
      } catch {
        /* ignore */
      }
      cancel = null;
    }
  };

  const flush = (send) => {
    clearTimer();
    if (!buf) return;
    const out = buf;
    buf = "";
    flushCount += 1;
    try {
      send(out);
    } catch {
      /* ignore */
    }
  };

  return {
    push(piece, send) {
      const s = piece == null ? "" : String(piece);
      if (!s) return;
      deltaCount += 1;
      charCount += s.length;
      buf += s;
      if (buf.length >= maxChars) {
        flush(send);
        return;
      }
      if (!cancel) {
        cancel = schedule(() => flush(send), maxWaitMs);
      }
    },
    /** Force remaining buffer out (stream end / abort). */
    flush(send) {
      flush(send);
    },
    stats() {
      return { deltaCount, charCount, flushCount, pending: buf.length };
    },
    resetStats() {
      deltaCount = 0;
      charCount = 0;
      flushCount = 0;
    },
  };
}

function createIpcMetrics() {
  const counters = {
    invokes: 0,
    errors: 0,
    slow: 0,
    streamDeltas: 0,
    streamFlushes: 0,
  };
  const slowLog = [];
  return {
    counters,
    recordInvoke(channel, ms, ok) {
      counters.invokes += 1;
      if (!ok) counters.errors += 1;
      if (ms >= 100) {
        counters.slow += 1;
        slowLog.push({ channel, ms, ts: Date.now() });
        if (slowLog.length > 40) slowLog.shift();
      }
    },
    recordStream(stats) {
      if (!stats) return;
      counters.streamDeltas += stats.deltaCount || 0;
      counters.streamFlushes += stats.flushCount || 0;
    },
    snapshot() {
      return {
        ...counters,
        slowRecent: slowLog.slice(-10),
      };
    },
  };
}

module.exports = {
  parseTrace,
  createBootTimeline,
  createDeltaCoalescer,
  createIpcMetrics,
};
