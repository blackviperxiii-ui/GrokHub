/**
 * Renderer client for self-modification + factory restore.
 */

export type SelfModInfo = {
  ok: boolean;
  root?: string;
  selfModDir?: string;
  snapshots?: Array<{
    id: string;
    createdAt?: number;
    kind?: string;
    note?: string;
    fileCount?: number;
  }>;
  journal?: string;
};

function api() {
  return typeof window !== "undefined" ? window.grokhubDesktop?.selfmod : undefined;
}

export async function selfModInfo(): Promise<SelfModInfo> {
  const s = api();
  if (!s?.info) return { ok: false };
  return s.info();
}

export async function selfModList(rel?: string) {
  const s = api();
  if (!s?.list) return { ok: false, error: "Desktop self-mod unavailable" };
  return s.list(rel);
}

export async function selfModRead(rel: string) {
  const s = api();
  if (!s?.read) return { ok: false, error: "Desktop self-mod unavailable" };
  return s.read(rel);
}

export async function selfModWrite(
  rel: string,
  content: string,
  opts?: { note?: string; snapshot?: boolean },
) {
  const s = api();
  if (!s?.write) return { ok: false, error: "Desktop self-mod unavailable" };
  return s.write(rel, content, opts);
}

export async function selfModPatch(
  rel: string,
  find: string,
  replace: string,
  opts?: { replaceAll?: boolean; note?: string },
) {
  const s = api();
  if (!s?.patch) return { ok: false, error: "Desktop self-mod unavailable" };
  return s.patch(rel, find, replace, opts);
}

export async function selfModSnapshot(note?: string) {
  const s = api();
  if (!s?.snapshot) return { ok: false, error: "Desktop self-mod unavailable" };
  return s.snapshot(note);
}

export async function selfModRestore(id: string) {
  const s = api();
  if (!s?.restore) return { ok: false, error: "Desktop self-mod unavailable" };
  return s.restore(id);
}

export async function selfModJournal(limit = 40) {
  const s = api();
  if (!s?.journal) return { ok: true, entries: [] };
  return s.journal(limit);
}

export async function factoryReinstall(opts?: {
  wipeMemory?: boolean;
  clearSelfMod?: boolean;
  token?: string;
}) {
  const g = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (!g?.factoryReinstall) {
    // fall back to normal apply with force
    if (g?.applyUpdate) {
      return g.applyUpdate({ force: true, factory: true, ...opts });
    }
    return { ok: false, error: "Factory reinstall requires the desktop app" };
  }
  return g.factoryReinstall(opts || {});
}

/** Parse SELF_MOD commands from model output */
export type SelfModCmd =
  | { kind: "read"; path: string }
  | { kind: "list"; path: string }
  | { kind: "write"; path: string; content: string }
  | { kind: "patch"; path: string; find: string; replace: string }
  | { kind: "snapshot"; note: string };

/**
 * Lines:
 * SELF_MOD: list src/components
 * SELF_MOD: read src/lib/version.ts
 * SELF_MOD: write path/to/file
 * <<<CONTENT
 * ...file body...
 * CONTENT>>>
 * SELF_MOD: patch path/to/file
 * <<<FIND
 * old
 * FIND>>>
 * <<<REPLACE
 * new
 * REPLACE>>>
 * SELF_MOD: snapshot note text
 */
export function extractSelfModCommands(text: string): SelfModCmd[] {
  const out: SelfModCmd[] = [];
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] || "";
    const m = line.match(/^\s*SELF_MOD:\s*(list|read|write|patch|snapshot)\s*(.*)$/i);
    if (!m) continue;
    const op = m[1]!.toLowerCase();
    const rest = (m[2] || "").trim();
    if (op === "list") {
      out.push({ kind: "list", path: rest || "src" });
      continue;
    }
    if (op === "read") {
      if (rest) out.push({ kind: "read", path: rest });
      continue;
    }
    if (op === "snapshot") {
      out.push({ kind: "snapshot", note: rest || "agent" });
      continue;
    }
    if (op === "write") {
      // collect CONTENT block after
      let content = "";
      let j = i + 1;
      if ((lines[j] || "").trim() === "<<<CONTENT") {
        j++;
        const buf: string[] = [];
        while (j < lines.length && (lines[j] || "").trim() !== "CONTENT>>>") {
          buf.push(lines[j]!);
          j++;
        }
        content = buf.join("\n");
        i = j;
      }
      if (rest) out.push({ kind: "write", path: rest, content });
      continue;
    }
    if (op === "patch") {
      let find = "";
      let replace = "";
      let j = i + 1;
      while (j < lines.length) {
        const L = (lines[j] || "").trim();
        if (L === "<<<FIND") {
          j++;
          const buf: string[] = [];
          while (j < lines.length && (lines[j] || "").trim() !== "FIND>>>") {
            buf.push(lines[j]!);
            j++;
          }
          find = buf.join("\n");
          j++;
          continue;
        }
        if (L === "<<<REPLACE") {
          j++;
          const buf: string[] = [];
          while (j < lines.length && (lines[j] || "").trim() !== "REPLACE>>>") {
            buf.push(lines[j]!);
            j++;
          }
          replace = buf.join("\n");
          j++;
          break;
        }
        if (/^\s*SELF_MOD:/i.test(lines[j] || "")) break;
        j++;
      }
      i = j - 1;
      if (rest && find) out.push({ kind: "patch", path: rest, find, replace });
    }
  }
  return out.slice(0, 6);
}

export function stripSelfModCommands(text: string): string {
  // remove SELF_MOD lines and content blocks for display
  let out = text;
  out = out.replace(/^\s*SELF_MOD:.*$/gim, "");
  out = out.replace(/<<<CONTENT\n[\s\S]*?\nCONTENT>>>/g, "");
  out = out.replace(/<<<FIND\n[\s\S]*?\nFIND>>>/g, "");
  out = out.replace(/<<<REPLACE\n[\s\S]*?\nREPLACE>>>/g, "");
  return out.replace(/\n{3,}/g, "\n\n").trim();
}
