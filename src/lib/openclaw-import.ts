/**
 * OpenClaw workspace → GrokHub import.
 *
 * OpenClaw layout (docs.openclaw.ai):
 *   ~/.openclaw/workspace/
 *     AGENTS.md, SOUL.md, USER.md, IDENTITY.md, TOOLS.md,
 *     HEARTBEAT.md, MEMORY.md, BOOT.md, BOOTSTRAP.md
 *     skills/<name>/SKILL.md
 *     memory/YYYY-MM-DD.md
 *
 * We import skills + persona/context files. Credentials and sqlite state are skipped.
 */

import type { Agent, Automation, Skill } from "./types";

export type OpenClawMarkdownFile = {
  name: string;
  relativePath: string;
  content: string;
};

export type OpenClawSkillFile = {
  dirName: string;
  relativePath: string;
  content: string;
};

export type OpenClawWorkspaceRaw = {
  root: string;
  files: OpenClawMarkdownFile[];
  skills: OpenClawSkillFile[];
  /** Optional openclaw.json snippet if found next to workspace */
  configHint?: string | null;
};

export type OpenClawImportResult = {
  root: string;
  skills: Skill[];
  agents: Agent[];
  automations: Automation[];
  /** Injected into Grok system prompt */
  contextBundle: string;
  identityName: string | null;
  identityEmoji: string | null;
  filesImported: string[];
  warnings: string[];
};

const CORE_FILES = [
  "AGENTS.md",
  "SOUL.md",
  "USER.md",
  "IDENTITY.md",
  "TOOLS.md",
  "HEARTBEAT.md",
  "MEMORY.md",
  "BOOT.md",
  "BOOTSTRAP.md",
] as const;

function uid(prefix: string): string {
  return `${prefix}_${Math.random().toString(36).slice(2, 10)}`;
}

function slugify(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 48) || "skill";
}

/** Parse YAML-ish frontmatter between --- fences. */
export function parseFrontmatter(md: string): {
  meta: Record<string, string | boolean>;
  body: string;
} {
  const text = md.replace(/^\uFEFF/, "");
  if (!text.startsWith("---")) {
    return { meta: {}, body: text.trim() };
  }
  const end = text.indexOf("\n---", 3);
  if (end < 0) return { meta: {}, body: text.trim() };
  const raw = text.slice(3, end).trim();
  const body = text.slice(end + 4).trim();
  const meta: Record<string, string | boolean> = {};
  for (const line of raw.split("\n")) {
    const m = /^([A-Za-z0-9_-]+)\s*:\s*(.*)$/.exec(line.trim());
    if (!m) continue;
    const key = m[1]!;
    let val = m[2]!.trim();
    if (
      (val.startsWith('"') && val.endsWith('"')) ||
      (val.startsWith("'") && val.endsWith("'"))
    ) {
      val = val.slice(1, -1);
    }
    if (val === "true") meta[key] = true;
    else if (val === "false") meta[key] = false;
    else meta[key] = val;
  }
  return { meta, body };
}

export function skillFromSkillMd(
  content: string,
  dirName: string,
): Skill | null {
  const { meta, body } = parseFrontmatter(content);
  const name =
    (typeof meta.name === "string" && meta.name) ||
    dirName ||
    "Imported skill";
  const description =
    (typeof meta.description === "string" && meta.description) ||
    body.slice(0, 140).replace(/\s+/g, " ") ||
    "Imported from OpenClaw";
  if (!body && !meta.name) return null;
  const slashBase = slugify(String(name));
  return {
    id: uid("ocskill"),
    name: String(name),
    description: String(description),
    kind: "custom",
    enabled: meta["disable-model-invocation"] === true ? false : true,
    slash: `/${slashBase}`,
    instructions: body || String(description),
    runs: 0,
  };
}

function extractIdentity(content: string): {
  name: string | null;
  emoji: string | null;
  role: string | null;
} {
  const { meta, body } = parseFrontmatter(content);
  let name =
    (typeof meta.name === "string" && meta.name) ||
    (typeof meta.Name === "string" && meta.Name) ||
    null;
  let emoji =
    (typeof meta.emoji === "string" && meta.emoji) ||
    (typeof meta.Emoji === "string" && meta.Emoji) ||
    null;
  let role =
    (typeof meta.vibe === "string" && meta.vibe) ||
    (typeof meta.role === "string" && meta.role) ||
    null;

  // Plain markdown fallbacks
  const nameLine = /(?:^|\n)#\s*(.+)/.exec(body);
  if (!name && nameLine) name = nameLine[1]!.trim();
  const nameField = /(?:name|agent)\s*[:—-]\s*(.+)/i.exec(body);
  if (!name && nameField) name = nameField[1]!.trim().slice(0, 64);
  const emojiField = /emoji\s*[:—-]\s*(\S+)/i.exec(body);
  if (!emoji && emojiField) emoji = emojiField[1]!;

  return { name, emoji, role };
}

function clip(s: string, max: number): string {
  if (s.length <= max) return s;
  return `${s.slice(0, max)}\n… [truncated ${s.length - max} chars]`;
}

/**
 * Convert raw OpenClaw tree into GrokHub skills / agents / automations / context.
 */
export function mapOpenClawWorkspace(raw: OpenClawWorkspaceRaw): OpenClawImportResult {
  const warnings: string[] = [];
  const filesImported: string[] = [];
  const byName = new Map(raw.files.map((f) => [f.name.toUpperCase(), f]));

  const skills: Skill[] = [];
  const seenSlash = new Set<string>();
  for (const sk of raw.skills) {
    const skill = skillFromSkillMd(sk.content, sk.dirName);
    if (!skill) {
      warnings.push(`Skipped skill at ${sk.relativePath}`);
      continue;
    }
    // de-dupe slash
    let slash = skill.slash;
    let n = 2;
    while (seenSlash.has(slash)) {
      slash = `${skill.slash}-${n++}`;
    }
    seenSlash.add(slash);
    skill.slash = slash;
    skills.push(skill);
    filesImported.push(sk.relativePath);
  }

  const contextParts: string[] = [
    `OpenClaw workspace imported from: ${raw.root}`,
    "Treat the following as standing agent memory and operating instructions.",
  ];

  for (const name of CORE_FILES) {
    const f = byName.get(name.toUpperCase());
    if (!f?.content?.trim()) continue;
    filesImported.push(f.relativePath || name);
    const budget =
      name === "MEMORY.md" || name === "USER.md"
        ? 4000
        : name === "AGENTS.md" || name === "SOUL.md"
          ? 6000
          : 3000;
    contextParts.push(`### ${name}\n${clip(f.content.trim(), budget)}`);
  }

  // Recent daily memory (if provided as files)
  const daily = raw.files
    .filter((f) => /^memory\/\d{4}-\d{2}-\d{2}\.md$/i.test(f.relativePath.replace(/\\/g, "/")))
    .sort((a, b) => b.relativePath.localeCompare(a.relativePath))
    .slice(0, 2);
  for (const d of daily) {
    filesImported.push(d.relativePath);
    contextParts.push(`### ${d.relativePath}\n${clip(d.content.trim(), 2500)}`);
  }

  const identityFile = byName.get("IDENTITY.MD");
  const id = identityFile
    ? extractIdentity(identityFile.content)
    : { name: null, emoji: null, role: null };

  const soul = byName.get("SOUL.MD");
  const agentsMd = byName.get("AGENTS.MD");

  const agents: Agent[] = [
    {
      id: "openclaw-primary",
      name: id.name || "OpenClaw Agent",
      role:
        id.role ||
        (soul
          ? clip(soul.content.replace(/\s+/g, " "), 80)
          : "Imported OpenClaw persona"),
      model: "Auto",
      status: "idle",
      tasks: 0,
      color: "#3b82f6",
    },
  ];
  if (agentsMd) {
    agents.push({
      id: "openclaw-ops",
      name: "Workspace Ops",
      role: "Follows AGENTS.md operating manual from OpenClaw workspace",
      model: "Build",
      status: "idle",
      tasks: 0,
      color: "#a855f7",
    });
  }

  const automations: Automation[] = [];
  const heartbeat = byName.get("HEARTBEAT.MD");
  if (heartbeat?.content?.trim()) {
    automations.push({
      id: uid("ocauto"),
      name: "OpenClaw Heartbeat",
      instructions: clip(heartbeat.content.trim(), 4000),
      schedule: "daily",
      time: "09:00",
      enabled: true,
      connectorIds: [],
      skillIds: skills.slice(0, 4).map((s) => s.id),
      runCount: 0,
    });
  }
  const boot = byName.get("BOOT.MD");
  if (boot?.content?.trim()) {
    automations.push({
      id: uid("ocauto"),
      name: "OpenClaw Boot checklist",
      instructions: clip(boot.content.trim(), 4000),
      schedule: "once",
      time: "00:00",
      enabled: false,
      connectorIds: [],
      skillIds: [],
      runCount: 0,
    });
  }

  if (!skills.length && !raw.files.length) {
    warnings.push("No OpenClaw files found — check the path points at a workspace folder.");
  }

  return {
    root: raw.root,
    skills,
    agents,
    automations,
    contextBundle: contextParts.join("\n\n"),
    identityName: id.name,
    identityEmoji: id.emoji,
    filesImported: [...new Set(filesImported)],
    warnings,
  };
}

export function defaultOpenClawPaths(home: string): string[] {
  const h = home.replace(/\/$/, "");
  return [
    `${h}/.openclaw/workspace`,
    `${h}/.openclaw/workspace-default`,
    `${h}/openclaw/workspace`,
  ];
}
