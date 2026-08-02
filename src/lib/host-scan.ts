/**
 * Keep broad host scans from killing the agent turn (timeouts / maxBuffer).
 */

/** Prefer longer budgets for filesystem / package work */
export function hostTimeoutMs(command: string, fallback = 90_000): number {
  const c = command || "";
  if (/\b(find|locate|updatedb|grep\s+-R|rg\s|ag\s|fd\s)\b/i.test(c)) {
    return 180_000;
  }
  if (/\b(du\s|ncdu|tree\b|rsync|tar\s|zip\b|unzip)\b/i.test(c)) {
    return 150_000;
  }
  if (/\b(npm|pnpm|yarn|cargo|go\s+build|make\b|cmake|pip|pacman|yay|paru)\b/i.test(c)) {
    return 240_000;
  }
  if (/\b(git\s+(clone|fetch|pull)|docker|podman)\b/i.test(c)) {
    return 180_000;
  }
  return fallback;
}

/**
 * Bound runaway scans so they finish and return useful partial output
 * instead of hanging until timeout / filling maxBuffer.
 */
export function boundHostScanCommand(command: string): {
  command: string;
  bounded: boolean;
  note?: string;
} {
  let cmd = String(command || "").trim();
  if (!cmd) return { command: cmd, bounded: false };

  const isFind = /\bfind\b/i.test(cmd);
  const isGrepR = /\bgrep\s+-[a-zA-Z]*R/i.test(cmd) || /\brg\b/.test(cmd);
  const isTree = /\btree\b/i.test(cmd);
  const isDu = /\bdu\b/i.test(cmd);

  if (!isFind && !isGrepR && !isTree && !isDu) {
    return { command: cmd, bounded: false };
  }

  let bounded = false;
  const notes: string[] = [];

  // Avoid scanning huge trees without maxdepth
  if (isFind && !/-maxdepth\b/i.test(cmd)) {
    // insert after first path-ish token if present: find PATH → find PATH -maxdepth 4
    // simple: append early after `find`
    cmd = cmd.replace(/\bfind\b(\s+)/i, "find$1-maxdepth 4 ");
    // if that put maxdepth in wrong place for `find -name`, still OK for GNU find (options can reorder mostly)
    // Safer pattern: if still no maxdepth (weird), force prefix
    if (!/-maxdepth\b/i.test(cmd)) {
      cmd = cmd.replace(/\bfind\b/i, "find -maxdepth 4");
    }
    bounded = true;
    notes.push("added -maxdepth 4");
  }

  // Skip heavy dirs commonly bloating scans
  if (isFind && !/-name\s+node_modules|-path\s+.*node_modules|prune/i.test(cmd)) {
    // only if not already pruning
    if (!cmd.includes("node_modules") || !/prune/i.test(cmd)) {
      // soft: user may have intended to search node_modules; skip auto-prune if node_modules is the target
      if (!/node_modules\s*$/i.test(cmd) && !/node_modules['"]?\s*\)/i.test(cmd)) {
        /* leave as-is; maxdepth is the main win */
      }
    }
  }

  // Cap tree depth
  if (isTree && !/-L\b|--max-depth/i.test(cmd)) {
    cmd = `${cmd} -L 3`;
    bounded = true;
    notes.push("tree -L 3");
  }

  // Ensure line cap for huge output (don't double-pipe head if already limited)
  const hasHead =
    /\|\s*head\b/i.test(cmd) ||
    /\|\s*tail\b/i.test(cmd) ||
    /\bhead\s+-/i.test(cmd) ||
    /-printf\b/.test(cmd) && /\|\s*head/i.test(cmd);

  if ((isFind || isGrepR || isTree || isDu) && !hasHead) {
    // Use bash pipeline; host runs via shell
    cmd = `( ${cmd} ) 2>/dev/null | head -n 200`;
    bounded = true;
    notes.push("piped to head -200");
  }

  // Hard overall time inside shell as backup (GNU timeout)
  if ((isFind || isGrepR) && !/\btimeout\b/.test(cmd)) {
    cmd = `timeout 150s bash -lc ${JSON.stringify(cmd)}`;
    bounded = true;
    notes.push("timeout 150s");
  }

  return {
    command: cmd,
    bounded,
    note: notes.length ? notes.join(", ") : undefined,
  };
}

/** Clip host tool output for the model context (keep head + tail). */
export function clipHostOutput(text: string, max = 24_000): string {
  const s = String(text || "");
  if (s.length <= max) return s;
  const head = Math.floor(max * 0.65);
  const tail = max - head - 80;
  return (
    s.slice(0, head) +
    `\n\n… [truncated ${s.length - max} chars of host output] …\n\n` +
    s.slice(-Math.max(0, tail))
  );
}
