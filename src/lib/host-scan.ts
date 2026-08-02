/**
 * Keep broad host scans from killing the agent turn (timeouts / maxBuffer),
 * while still returning enough output for debugging (model greps, file scans).
 */

/** Prefer longer budgets for filesystem / package work */
export function hostTimeoutMs(command: string, fallback = 120_000): number {
  const c = command || "";
  if (/\b(find|locate|updatedb|grep\s+-R|rg\s|ag\s|fd\s)\b/i.test(c)) {
    return 240_000;
  }
  if (/\b(du\s|ncdu|tree\b|rsync|tar\s|zip\b|unzip)\b/i.test(c)) {
    return 180_000;
  }
  if (/\b(npm|pnpm|yarn|cargo|go\s+build|make\b|cmake|pip|pacman|yay|paru)\b/i.test(c)) {
    return 300_000;
  }
  if (/\b(git\s+(clone|fetch|pull)|docker|podman)\b/i.test(c)) {
    return 240_000;
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
    cmd = cmd.replace(/\bfind\b(\s+)/i, "find$1-maxdepth 5 ");
    if (!/-maxdepth\b/i.test(cmd)) {
      cmd = cmd.replace(/\bfind\b/i, "find -maxdepth 5");
    }
    bounded = true;
    notes.push("added -maxdepth 5");
  }

  // Cap tree depth
  if (isTree && !/-L\b|--max-depth/i.test(cmd)) {
    cmd = `${cmd} -L 4`;
    bounded = true;
    notes.push("tree -L 4");
  }

  // Cap huge output — generous line budget so greps on built assets aren't useless
  const hasHead =
    /\|\s*head\b/i.test(cmd) ||
    /\|\s*tail\b/i.test(cmd) ||
    /\bhead\s+-/i.test(cmd);

  if ((isFind || isGrepR || isTree || isDu) && !hasHead) {
    // 2000 lines ≈ enough for multi-file grep hits without flooding context
    cmd = `( ${cmd} ) 2>/dev/null | head -n 2000`;
    bounded = true;
    notes.push("piped to head -2000");
  }

  // Hard overall time inside shell as backup (GNU timeout)
  if ((isFind || isGrepR) && !/\btimeout\b/.test(cmd)) {
    cmd = `timeout 200s bash -lc ${JSON.stringify(cmd)}`;
    bounded = true;
    notes.push("timeout 200s");
  }

  return {
    command: cmd,
    bounded,
    note: notes.length ? notes.join(", ") : undefined,
  };
}

/**
 * Clip host tool output for the model context (keep head + tail).
 * Default 96KB so long greps still reach the model usefully.
 */
export function clipHostOutput(text: string, max = 96_000): string {
  const s = String(text || "");
  if (s.length <= max) return s;
  const head = Math.floor(max * 0.7);
  const tail = max - head - 100;
  return (
    s.slice(0, head) +
    `\n\n… [truncated ${s.length - max} chars of host output — head+tail kept] …\n\n` +
    s.slice(-Math.max(0, tail))
  );
}
