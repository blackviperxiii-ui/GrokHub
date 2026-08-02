/**
 * Bound project folder — injects a light summary into agent context.
 */
import { hostListDir, hostReadFile } from "./host-client";

export type ProjectWorkspace = {
  path: string;
  name: string;
  summary: string;
  boundAt: number;
  /** Optional thread scope */
  threadId?: string | null;
};

export function projectNameFromPath(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || p || "Project";
}

export async function buildProjectSummary(root: string): Promise<string> {
  const lines: string[] = [`# Project: ${projectNameFromPath(root)}`, `Path: ${root}`, ""];
  try {
    const listing = await hostListDir(root);
    const entries = (listing.entries || []).slice(0, 40);
    lines.push("## Top-level");
    for (const e of entries) {
      lines.push(`- ${e.isDir ? "📁" : "📄"} ${e.name}`);
    }
  } catch (e) {
    lines.push(`_(Could not list dir: ${e instanceof Error ? e.message : "error"})_`);
  }

  // Common project markers
  for (const rel of [
    "README.md",
    "package.json",
    "AGENTS.md",
    "CLAUDE.md",
    ".grokhub/PROJECT.md",
  ]) {
    try {
      const r = await hostReadFile(
        root.replace(/\/$/, "") + "/" + rel,
        12_000,
      );
      if (r?.content) {
        lines.push("", `## ${rel}`, r.content.slice(0, 4_000));
      }
    } catch {
      /* skip */
    }
  }
  return lines.join("\n").slice(0, 14_000);
}

export function projectContextBlock(ws: ProjectWorkspace | null | undefined): string {
  if (!ws?.path) return "";
  return [
    "## Bound project workspace",
    `Working directory preference: ${ws.path}`,
    "Prefer HOST_CMD with cwd in this tree when editing or inspecting files.",
    "",
    ws.summary?.slice(0, 8_000) || `Project: ${ws.name}`,
  ].join("\n");
}
