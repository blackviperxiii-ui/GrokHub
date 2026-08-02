import type { GrokModeId } from "./types";
import { modelIdForMode, resolveMode } from "./modes";
import { parseRateLimitHeaders } from "./usage";

export const XAI_BASE = "https://api.x.ai/v1";

/** Map GrokHub modes → xAI model IDs */
export function modelForMode(mode: GrokModeId, prompt = ""): string {
  return modelIdForMode(mode, prompt);
}

export function systemPromptForMode(mode: GrokModeId, prompt = ""): string {
  const base = `You are Grok, running inside GrokHub (a desktop agent control plane on the user's Linux machine).
Help with coding, ops, research, and local machine tasks.
Be direct and practical. Prefer short structured answers with bullets when listing steps.
Do not prefix replies with mode labels like [Fast] or [Auto → …]. Just answer.

## What persists (do not claim otherwise)
GrokHub keeps user data outside the install tree across restarts and in-app updates:
- Chat threads and message history
- Settings, mode, agent prefs, and **persistent memory notes** (Settings → memory / \`/memory\`)
- Imagine gallery media, connectors status, automations, OAuth/session links
When the user asks what you remember, use this conversation history and any "User persistent memory notes" in context. Only say you lack memory if those are empty.

## Desktop host (unsandboxed)
When the Desktop Host connector is LIVE you can act on the real machine (files, shell, apps).
Do NOT invent filesystem listings, command output, or process state.
Put host commands on their OWN line, alone:
HOST_CMD: ls -la "$HOME/Downloads"
Never glue HOST_CMD onto a prose sentence. Prefer one simple command (ls, head, cat, find, stat).
For broad filesystem scans always bound the work:
- use find -maxdepth 3 (or 4) and exclude huge trees when possible
- pipe to head -n 100 (never unbounded find / grep -R on / or $HOME)
- prefer targeted paths (project dir) over whole-home scans
The runtime executes HOST_CMD and returns HOST_RESULT — then summarize clearly for the user.
You may use multiple HOST_CMD rounds if needed. If a scan times out, narrow scope instead of repeating the same broad command.
Be tool-first for real system questions: call HOST_CMD rather than guessing. Skip host tools for pure chat, writing, or code that does not need live machine data.
Do not run destructive commands (rm -rf, disk wipe, credential theft) unless the user clearly requests them; the app may confirm risky commands.

## Workboard (task pins)
When you break work into trackable steps — or the user asks for a plan/todo — pin items the user can approve/stage/dismiss:
WORK_PIN: Short title | optional detail | priority=high
WORK_PIN: Another task | detail
Update existing items (by id or title fragment):
WORK_UPDATE: title-fragment | status=in_progress
WORK_UPDATE: id | status=done
Statuses: proposed (default on pin), approved, staged, in_progress, done, dismissed.
Do not mark done unless the work is actually finished. Keep pins short; user reviews them on the Workboard.

CRITICAL — no fake progress / no stalling:
- NEVER say "I'll check", "I'll probe", "let me investigate", "continuing the deep dive", or "would you like me to start" without also emitting HOST_CMD lines in the SAME reply.
- If the user asks about their system, install, processes, logs, files, or bugs needing local data: emit HOST_CMD immediately (short preface OK).
- Do not ask permission for safe read-only diagnostics — just run them.
- Do not end a turn with only a plan. Commands first, then summarize after HOST_RESULT.

## Connectors
Only use tools listed as LIVE in the connector context below. Do not invent mail, calendar, or Notion results.
For live cloud tools, put a command on its OWN line:
CONNECTOR_CMD: github search_code query:useState repo:facebook/react
CONNECTOR_CMD: github list_issues repo:owner/name
CONNECTOR_CMD: github list_repos
CONNECTOR_CMD: github user
The runtime returns CONNECTOR_RESULT. Summarize for the user; do not invent GitHub data.
Website-linked connectors (Gmail, Notion, etc.) may be marked connected for status only — if tools are not LIVE, say so and suggest the Grok website for those tools.

## Self-modification (optional)
When the user enables self-modification and asks you to change GrokHub itself, you may edit the install tree with:
SELF_MOD: list src/components
SELF_MOD: read src/lib/version.ts
SELF_MOD: write relative/path.ts
<<<CONTENT
// full file body
CONTENT>>>
SELF_MOD: patch relative/path.ts
<<<FIND
exact old text
FIND>>>
<<<REPLACE
new text
REPLACE>>>
SELF_MOD: snapshot note before risky change
Allowed roots: src/, desktop/, scripts/, packaging/, package.json, vite.config.ts, etc. Never touch node_modules, secrets, or user memory.
Always snapshot before multi-file edits. If something breaks, tell the user: Settings → Factory reinstall from GitHub restores stock code (memory can be kept).

## Safety
Refuse criminal activity, malware, exploits, and clear abuse. Decline even if framed as a test.
Long-running interactive TUIs are awkward — prefer non-interactive commands and report status.`;

  const id = resolveMode(mode, prompt);
  switch (id) {
    case "fast":
      return `${base}\nMode: Fast — concise answers, minimal preamble.`;
    case "expert":
      return `${base}\nMode: Expert — reason carefully, surface tradeoffs, cite assumptions.`;
    case "heavy":
      return `${base}\nMode: Heavy (team of experts) — consider multiple angles (ops, research, build, critique), then synthesize a clear recommendation.`;
    case "build":
      return `${base}\nMode: Build — prioritize working code, file paths, and implementable steps. Prefer complete snippets.`;
    default:
      return base;
  }
}

/** Extra system block describing which connectors are actually usable. */
export function connectorContextBlock(
  connectors: Array<{
    id: string;
    name: string;
    status: string;
    tools: string[];
    accountLabel?: string | null;
    liveTools?: boolean;
    source?: string;
  }>,
): string {
  const connected = connectors.filter((c) => c.status === "connected");
  if (!connected.length) {
    return "\n\n## Connector status\nNone connected. User can link Grok website or GitHub token in Settings.";
  }
  const lines = connected.map((c) => {
    const live = c.liveTools || c.id === "github" || c.id === "desktop-host" || c.id === "grok-xai";
    const acct = c.accountLabel ? ` · ${c.accountLabel}` : "";
    const src = c.source ? ` · via ${c.source}` : "";
    return `- ${c.name} (${c.id}): ${live ? "LIVE tools" : "status only (website)"}${acct}${src} · tools: ${c.tools.join(", ")}`;
  });
  return `\n\n## Connector status\n${lines.join("\n")}\nOnly call CONNECTOR_CMD for LIVE tools.`;
}

export type GrokChatMessage = {
  role: "system" | "user" | "assistant";
  content: string;
};

export type GrokChatRequest = {
  /** Console API key (xai-…) */
  apiKey?: string;
  /** OAuth access token from SuperGrok / X Premium device-code login */
  accessToken?: string;
  mode?: GrokModeId;
  messages: GrokChatMessage[];
  model?: string;
  temperature?: number;
  maxTokens?: number;
  signal?: AbortSignal;
  /** OpenClaw workspace / standing agent context */
  workspaceContext?: string;
};

export type GrokChatResult = {
  ok: boolean;
  content?: string;
  model?: string;
  usage?: { prompt_tokens?: number; completion_tokens?: number; total_tokens?: number };
  error?: string;
  status?: number;
  aborted?: boolean;
  rateLimit?: {
    remaining: number | null;
    limit: number | null;
    resetAt: number | null;
  };
};

function resolveBearer(req: GrokChatRequest): { bearer: string; source: "oauth" | "key" | "env" } | null {
  if (req.accessToken?.trim()) {
    return { bearer: req.accessToken.trim(), source: "oauth" };
  }
  if (req.apiKey?.trim()) {
    return { bearer: req.apiKey.trim(), source: "key" };
  }
  const env =
    process.env.XAI_API_KEY?.trim() || process.env.GROK_API_KEY?.trim() || "";
  if (env) return { bearer: env, source: "env" };
  return null;
}

function buildBody(req: GrokChatRequest, stream: boolean) {
  const mode = req.mode ?? "auto";
  const lastUser = [...req.messages].reverse().find((m) => m.role === "user")?.content ?? "";
  const routed = resolveMode(mode, lastUser);
  const model = req.model || modelForMode(mode, lastUser);
  const system =
    systemPromptForMode(mode, lastUser) +
    (req.workspaceContext?.trim()
      ? `\n\n## Imported OpenClaw workspace context\n${req.workspaceContext.trim().slice(0, 24_000)}`
      : "");
  const messages: GrokChatMessage[] = [
    { role: "system", content: system },
    ...req.messages.filter((m) => m.role !== "system"),
  ];
  const temperature =
    req.temperature ??
    (routed === "fast" ? 0.5 : routed === "build" ? 0.4 : routed === "heavy" ? 0.8 : 0.7);
  const max_tokens =
    req.maxTokens ??
    (routed === "heavy" ? 8192 : routed === "build" ? 8192 : routed === "expert" ? 6144 : 4096);
  return {
    model,
    body: {
      model,
      messages,
      temperature,
      max_tokens,
      stream,
    },
    routed,
  };
}

export async function callXaiChat(req: GrokChatRequest): Promise<GrokChatResult> {
  const auth = resolveBearer(req);
  if (!auth) {
    return {
      ok: false,
      status: 401,
      error:
        "Not connected to Grok. Use Settings → Connect with Grok OAuth (SuperGrok / X Premium) or paste an xAI API key.",
    };
  }

  const { model, body } = buildBody(req, false);

  try {
    const res = await fetch(`${XAI_BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${auth.bearer}`,
      },
      body: JSON.stringify(body),
      signal: req.signal,
    });

    const data = (await res.json().catch(() => ({}))) as {
      error?: { message?: string } | string;
      choices?: Array<{ message?: { content?: string } }>;
      model?: string;
      usage?: GrokChatResult["usage"];
    };
    const rateLimit = parseRateLimitHeaders(res.headers);

    if (!res.ok) {
      if (
        res.status === 404 ||
        (typeof data.error === "object" &&
          /model|not found|invalid/i.test(data.error?.message || ""))
      ) {
        if (model === "grok-4.5" || model === "grok-4-5") {
          return callXaiChat({ ...req, model: "grok-4.3" });
        }
        if (model === "grok-4.3") {
          return callXaiChat({ ...req, model: "grok-4" });
        }
        if (model === "grok-code-fast-1" || /build/i.test(model)) {
          return callXaiChat({ ...req, model: "grok-code-fast-1" });
        }
        if (model === "grok-4-1-fast-non-reasoning") {
          return callXaiChat({ ...req, model: "grok-3-mini-fast" });
        }
      }
      const msg =
        typeof data.error === "string"
          ? data.error
          : data.error?.message || `xAI error ${res.status}`;
      return { ok: false, status: res.status, error: msg, model, rateLimit };
    }

    const content = data.choices?.[0]?.message?.content?.trim();
    if (!content) {
      return { ok: false, status: res.status, error: "Empty response from Grok", model, rateLimit };
    }

    return {
      ok: true,
      content,
      model: data.model || model,
      usage: data.usage,
      status: res.status,
      rateLimit,
    };
  } catch (e) {
    if (req.signal?.aborted || (e instanceof Error && e.name === "AbortError")) {
      return { ok: false, aborted: true, error: "Stopped" };
    }
    return {
      ok: false,
      error: e instanceof Error ? e.message : "Network error calling xAI",
    };
  }
}

export type StreamHandlers = {
  onDelta?: (text: string) => void;
  onStatus?: (status: string) => void;
  signal?: AbortSignal;
};

/** Stream Grok tokens (SSE). Calls onDelta for each piece of content. */
export async function callXaiChatStream(
  req: GrokChatRequest,
  handlers: StreamHandlers = {},
): Promise<GrokChatResult> {
  const auth = resolveBearer(req);
  if (!auth) {
    return {
      ok: false,
      status: 401,
      error:
        "Not connected to Grok. Use Settings → Connect with Grok OAuth (SuperGrok / X Premium) or paste an xAI API key.",
    };
  }

  const signal = handlers.signal || req.signal;
  const { model, body } = buildBody(req, true);
  handlers.onStatus?.("connecting");

  try {
    const res = await fetch(`${XAI_BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${auth.bearer}`,
        accept: "text/event-stream",
      },
      body: JSON.stringify(body),
      signal,
    });

    if (!res.ok) {
      // Fall back to non-stream once for model aliases / older accounts
      const errText = await res.text().catch(() => "");
      if (res.status === 404 || /model|not found|invalid/i.test(errText)) {
        if (model === "grok-4.5" || model === "grok-4-5") {
          return callXaiChatStream({ ...req, model: "grok-4.3" }, handlers);
        }
        if (model === "grok-4.3") {
          return callXaiChatStream({ ...req, model: "grok-4" }, handlers);
        }
        if (model === "grok-4-1-fast-non-reasoning") {
          return callXaiChatStream({ ...req, model: "grok-3-mini-fast" }, handlers);
        }
      }
      // Non-stream fallback
      handlers.onStatus?.("fallback");
      const full = await callXaiChat({ ...req, model, signal });
      if (full.ok && full.content) handlers.onDelta?.(full.content);
      return full;
    }

    if (!res.body) {
      handlers.onStatus?.("fallback");
      const full = await callXaiChat({ ...req, model, signal });
      if (full.ok && full.content) handlers.onDelta?.(full.content);
      return full;
    }

    handlers.onStatus?.("streaming");
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let content = "";
    let usedModel = model;
    let streamUsage: GrokChatResult["usage"] | undefined;
    const rateLimit = parseRateLimitHeaders(res.headers);

    while (true) {
      if (signal?.aborted) {
        try {
          await reader.cancel();
        } catch {
          /* ignore */
        }
        return {
          ok: false,
          aborted: true,
          error: "Stopped",
          content,
          model: usedModel,
          usage: streamUsage,
          rateLimit,
        };
      }
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop() || "";
      for (const raw of lines) {
        const line = raw.trim();
        if (!line || line.startsWith(":")) continue;
        if (!line.startsWith("data:")) continue;
        const data = line.slice(5).trim();
        if (data === "[DONE]") continue;
        try {
          const json = JSON.parse(data) as {
            model?: string;
            usage?: GrokChatResult["usage"];
            choices?: Array<{ delta?: { content?: string }; message?: { content?: string } }>;
          };
          if (json.model) usedModel = json.model;
          if (json.usage) streamUsage = json.usage;
          const piece =
            json.choices?.[0]?.delta?.content ||
            json.choices?.[0]?.message?.content ||
            "";
          if (piece) {
            content += piece;
            handlers.onDelta?.(piece);
          }
        } catch {
          /* skip bad chunk */
        }
      }
    }

    if (!content.trim()) {
      return { ok: false, error: "Empty stream from Grok", model: usedModel, rateLimit };
    }
    handlers.onStatus?.("done");
    return { ok: true, content, model: usedModel, usage: streamUsage, rateLimit };
  } catch (e) {
    if (signal?.aborted || (e instanceof Error && e.name === "AbortError")) {
      return { ok: false, aborted: true, error: "Stopped" };
    }
    // Network / stream failure → one non-stream retry
    handlers.onStatus?.("fallback");
    try {
      const full = await callXaiChat({ ...req, model, signal });
      if (full.ok && full.content) handlers.onDelta?.(full.content);
      return full;
    } catch (e2) {
      if (signal?.aborted || (e2 instanceof Error && e2.name === "AbortError")) {
        return { ok: false, aborted: true, error: "Stopped" };
      }
      return {
        ok: false,
        error: e instanceof Error ? e.message : "Network error calling xAI",
      };
    }
  }
}

/** Parse HOST_CMD commands the model emits for desktop execution (own line or inline). */
export function extractHostCommands(text: string): string[] {
  const cmds: string[] = [];
  // Own-line form (preferred)
  for (const line of text.split("\n")) {
    const m = line.match(/^\s*HOST_CMD:\s*(.+?)\s*$/i);
    if (m?.[1]) cmds.push(m[1].trim());
  }
  // Inline form: "... now. HOST_CMD: ls ..."
  const inline = [...text.matchAll(/(?:^|[\s.])HOST_CMD:\s*(.+?)(?=\n|$)/gi)];
  for (const m of inline) {
    const cmd = (m[1] || "").trim();
    if (cmd && !cmds.includes(cmd)) cmds.push(cmd);
  }
  // Fenced host/bash — only short shell one-liners (not multi-line code samples)
  const fenced = [...text.matchAll(/```(?:host|bash|sh)\s*\n([\s\S]*?)```/gi)];
  for (const m of fenced) {
    const body = (m[1] || "").trim();
    const lines = body
      .split("\n")
      .map((l) => l.trim())
      .filter((l) => l && !l.startsWith("#"));
    // Skip large code dumps — those are examples, not host actions
    if (lines.length > 6 || body.length > 600) continue;
    for (const cmd of lines) {
      if (/^(import |export |function |const |let |var |class |def |package )/i.test(cmd)) continue;
      if (cmd && !cmds.includes(cmd)) cmds.push(cmd);
    }
  }
  return cmds.filter(Boolean);
}

/** Remove HOST_CMD markers from text shown to the user. */
export function stripHostCommands(text: string): string {
  let out = text;
  out = out
    .split("\n")
    .filter(
      (line) =>
        !/^\s*HOST_CMD:\s*/i.test(line) &&
        !/^\s*WORK_PIN:\s*/i.test(line) &&
        !/^\s*WORK_UPDATE:\s*/i.test(line),
    )
    .join("\n");
  out = out.replace(/\s*HOST_CMD:\s*.+$/gim, "");
  out = out.replace(/\s*WORK_PIN:\s*.+$/gim, "");
  out = out.replace(/\s*WORK_UPDATE:\s*.+$/gim, "");
  // Drop short host fences only (keep large code samples for display)
  out = out.replace(/```(?:host|bash|sh)\s*\n([\s\S]*?)```/gi, (block, body: string) => {
    const lines = String(body || "")
      .trim()
      .split("\n")
      .filter((l) => l.trim() && !l.trim().startsWith("#"));
    if (lines.length <= 6 && String(body || "").length <= 600) return "";
    return block;
  });
  return out.replace(/\n{3,}/g, "\n\n").trim();
}

/**
 * Model promised host work but emitted no HOST_CMD (classic stall).
 * e.g. "I'll probe processes…" / "Would you like me to start the investigation?"
 */
export function looksLikeDeferredHostWork(text: string): boolean {
  const s = text || "";
  if (/HOST_CMD\s*:/i.test(s)) return false;
  const plan =
    /\b(i('ll| will)|let me|i can|i should|i'm going to|going to)\b.{0,40}\b(check|probe|inspect|investigate|scan|look|run|start|continue|dig|examine|verify)\b/i.test(
      s,
    ) ||
    /\b(continuing|continue)\b.{0,30}\b(deep dive|investigation|scan|probe)\b/i.test(s) ||
    /\b(would you like me to|shall i|want me to)\b.{0,40}\b(start|run|check|investigate|probe)\b/i.test(
      s,
    ) ||
    /\binstead of actually running\b/i.test(s) ||
    /\bnever output any real\b.{0,20}\bHOST_CMD\b/i.test(s);
  return plan;
}

/** User wants local investigation / diagnostics. */
export function userWantsHostInvestigation(prompt: string): boolean {
  const pr = prompt || "";
  return (
    /\b(deep dive|investigate|diagnos|why.*(stop|stall|break|fail)|what.*(wrong|broken)|check (my |the )?(system|install|process|log)|on my (machine|desktop|pc|linux)|host (cmd|tool)|run (commands?|diagnostics?)|look at (my |the )?(system|files?|install))\b/i.test(
      pr,
    ) ||
    /\b(streaming stops?|agent (stall|stuck|not working)|host.?cmd|desktop host)\b/i.test(pr) ||
    /\b(ps aux|journalctl|find \/|list (files|processes)|read (the )?log)\b/i.test(pr)
  );
}

/**
 * If the user clearly asks about local files/folders and the model forgot HOST_CMD,
 * invent a safe listing / diagnostics command.
 */
export function inferHostCommandsFromUser(prompt: string): string[] {
  const p = prompt.toLowerCase();
  const wantsList =
    /\b(list|show|what('|’)?s|whats|what do i have|contents?|files?|inside|in my)\b/.test(p) ||
    /\b(check|look at|open)\b/.test(p);
  if (!wantsList && !/\b(download|downloads|desktop|documents|home|folder|directory)\b/.test(p) && !userWantsHostInvestigation(prompt)) {
    return [];
  }

  if (/\bdownloads?\b/.test(p)) {
    return [
      'ls -la "${HOME}/Downloads" 2>/dev/null || ls -la ~/Downloads 2>/dev/null || ls -la "$HOME/Descargas" 2>/dev/null || echo "Downloads folder not found"',
    ];
  }
  if (/\bdocuments?\b/.test(p)) {
    return [
      'ls -la "${HOME}/Documents" 2>/dev/null || ls -la ~/Documents 2>/dev/null || echo "Documents folder not found"',
    ];
  }
  if (/\bdesktop\b/.test(p) && !/\bdesktop host\b/i.test(prompt)) {
    return [
      'ls -la "${HOME}/Desktop" 2>/dev/null || ls -la ~/Desktop 2>/dev/null || echo "Desktop folder not found"',
    ];
  }
  if (/\bhome\b/.test(p) && wantsList) {
    return ['ls -la "$HOME" | head -80'];
  }
  if (userWantsHostInvestigation(prompt)) {
    return [
      'uname -a; echo "---"; whoami; echo "---"; pwd; echo "---"; ls -la "${HOME}/.local/lib/grokhub" 2>/dev/null | head -30; ls -la /usr/lib/grokhub 2>/dev/null | head -20; echo "---"; ps -eo pid,cmd --sort=-pid 2>/dev/null | grep -iE "grokhub|electron" | grep -v grep | head -20 || true',
    ];
  }
  return [];
}

export async function probeXaiKey(apiKey: string): Promise<{ ok: boolean; detail: string }> {
  const key = apiKey.trim();
  if (!key) return { ok: false, detail: "API key is empty" };
  try {
    const res = await fetch(`${XAI_BASE}/models`, {
      headers: { authorization: `Bearer ${key}` },
    });
    if (res.ok) return { ok: true, detail: "Connected to xAI · models reachable" };
    const text = await res.text();
    return { ok: false, detail: `xAI ${res.status}: ${text.slice(0, 160)}` };
  } catch (e) {
    return { ok: false, detail: e instanceof Error ? e.message : "probe failed" };
  }
}

export async function probeXaiBearer(bearer: string): Promise<{ ok: boolean; detail: string }> {
  return probeXaiKey(bearer);
}

export type GrokImagineResult = {
  ok: boolean;
  imageDataUrl?: string;
  videoDataUrl?: string;
  model?: string;
  source?: "xai" | "local";
  error?: string;
  mediaKind?: "image" | "video";
};

/** Map UI aspect → xAI image aspect_ratio (no OpenAI-style `size`). */
function imageAspectRatio(aspect?: string): string | undefined {
  if (!aspect || aspect === "auto") return "auto";
  const allowed = new Set([
    "1:1",
    "16:9",
    "9:16",
    "4:3",
    "3:4",
    "3:2",
    "2:3",
    "2:1",
    "1:2",
    "19.5:9",
    "9:19.5",
    "20:9",
    "9:20",
  ]);
  if (allowed.has(aspect)) return aspect;
  // legacy aliases
  if (aspect === "4:5") return "2:3";
  return "auto";
}

/** Video API is pickier — landscape / portrait / square. */
function videoAspectRatio(aspect?: string): string {
  if (aspect === "9:16" || aspect === "2:3" || aspect === "3:4" || aspect === "1:2") {
    return "9:16";
  }
  if (aspect === "1:1") return "1:1";
  return "16:9";
}

function qualityHint(quality?: string, kind?: string): string {
  if (kind === "video") {
    return quality === "quality"
      ? "cinematic motion, high detail, smooth camera"
      : "clear motion, simple scene";
  }
  return quality === "quality"
    ? "ultra detailed, sharp focus, professional lighting, high fidelity"
    : "clean composition, efficient render";
}

function sleep(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}

async function pollVideoJob(
  requestId: string,
  bearer: string,
  maxMs = 180_000,
): Promise<{ url?: string; error?: string }> {
  const started = Date.now();
  let delay = 1500;
  while (Date.now() - started < maxMs) {
    const res = await fetch(`${XAI_BASE}/videos/${encodeURIComponent(requestId)}`, {
      headers: { authorization: `Bearer ${bearer}`, accept: "application/json" },
    });
    const data = (await res.json().catch(() => ({}))) as {
      status?: string;
      error?: { message?: string } | string;
      video?: { url?: string };
      url?: string;
      video_url?: string;
    };
    if (!res.ok) {
      const msg =
        typeof data.error === "string"
          ? data.error
          : data.error?.message || `poll ${res.status}`;
      // 404 may mean not ready on some deployments — keep waiting a bit
      if (res.status === 404 && Date.now() - started < 15_000) {
        await sleep(delay);
        continue;
      }
      return { error: msg };
    }
    const status = String(data.status || "").toLowerCase();
    if (status === "done" || status === "completed" || status === "succeeded" || status === "success") {
      const url = data.video?.url || data.video_url || data.url;
      if (url) return { url };
      return { error: "video done but no URL" };
    }
    if (status === "failed" || status === "expired" || status === "error" || status === "cancelled") {
      const msg =
        typeof data.error === "string"
          ? data.error
          : data.error?.message || `video ${status}`;
      return { error: msg };
    }
    await sleep(delay);
    delay = Math.min(5000, Math.floor(delay * 1.25));
  }
  return { error: "video generation timed out — try a shorter clip or Image mode" };
}

/** Live Grok / xAI Imagine (image + async video). Never sends unsupported `size`. */
export async function callXaiImagine(req: {
  prompt: string;
  accessToken?: string;
  apiKey?: string;
  model?: string;
  aspect?: string;
  quality?: "speed" | "quality";
  mediaKind?: "image" | "video";
  n?: number;
  duration?: number;
  /** data URL or https URL for reference / img2img / image-to-video */
  referenceDataUrl?: string;
}): Promise<GrokImagineResult> {
  const auth = resolveBearer({
    accessToken: req.accessToken,
    apiKey: req.apiKey,
    messages: [],
  });
  if (!auth) {
    return {
      ok: false,
      error: "Not connected — Grok OAuth or API key required for live Imagine",
    };
  }
  const prompt = req.prompt.trim();
  if (!prompt) return { ok: false, error: "empty prompt" };

  const mediaKind = req.mediaKind || "image";
  const qHint = qualityHint(req.quality, mediaKind);
  const fullPrompt = `${prompt}\n\n[${qHint}]`.trim();
  const n = Math.min(4, Math.max(1, req.n || 1));
  const headers = {
    "content-type": "application/json",
    authorization: `Bearer ${auth.bearer}`,
    accept: "application/json",
  };

  if (mediaKind === "video") {
    const videoModels = [
      req.model,
      "grok-imagine-video-1.5",
      "grok-imagine-video",
    ].filter(Boolean) as string[];
    const aspect = videoAspectRatio(req.aspect);
    const resolution =
      req.quality === "quality" ? "1080p" : "720p";
    const duration = Math.min(15, Math.max(5, Number(req.duration) || (req.quality === "quality" ? 10 : 6)));
    let lastErr = "video generation unavailable";

    for (const model of videoModels) {
      try {
        const body: Record<string, unknown> = {
          model,
          prompt: fullPrompt,
          duration,
          aspect_ratio: aspect,
          resolution,
        };
        // Image-to-video when a reference is set
        if (req.referenceDataUrl) {
          body.image = { url: req.referenceDataUrl };
        }
        const res = await fetch(`${XAI_BASE}/videos/generations`, {
          method: "POST",
          headers,
          body: JSON.stringify(body),
        });
        const data = (await res.json().catch(() => ({}))) as {
          error?: { message?: string } | string;
          request_id?: string;
          id?: string;
          data?: Array<{ video_url?: string; url?: string; b64_json?: string }>;
          video?: { url?: string };
          url?: string;
          model?: string;
          status?: string;
        };
        if (!res.ok) {
          lastErr =
            typeof data.error === "string"
              ? data.error
              : data.error?.message || `xAI video ${res.status} (${model})`;
          // If aspect rejected, retry once with 16:9
          if (/aspect/i.test(lastErr) && aspect !== "16:9") {
            try {
              const retry = await fetch(`${XAI_BASE}/videos/generations`, {
                method: "POST",
                headers,
                body: JSON.stringify({
                  ...body,
                  aspect_ratio: "16:9",
                  resolution: "720p",
                }),
              });
              const rd = (await retry.json().catch(() => ({}))) as typeof data;
              if (retry.ok) {
                Object.assign(data, rd);
              } else {
                continue;
              }
            } catch {
              continue;
            }
          } else {
            continue;
          }
        }

        // Sync URL if API ever returns immediately
        const immediate =
          data.video?.url ||
          data.url ||
          data.data?.[0]?.video_url ||
          data.data?.[0]?.url;
        if (immediate) {
          return {
            ok: true,
            videoDataUrl: immediate,
            model: data.model || model,
            source: "xai",
            mediaKind: "video",
          };
        }

        const requestId = data.request_id || data.id;
        if (requestId) {
          const polled = await pollVideoJob(requestId, auth.bearer);
          if (polled.url) {
            return {
              ok: true,
              videoDataUrl: polled.url,
              model: data.model || model,
              source: "xai",
              mediaKind: "video",
            };
          }
          lastErr = polled.error || lastErr;
          continue;
        }

        lastErr = "video response missing request_id";
      } catch (e) {
        lastErr = e instanceof Error ? e.message : "network error";
      }
    }
    return {
      ok: false,
      error: `${lastErr}. Video needs SuperGrok + xAI video API access; try Image mode if this keeps failing.`,
      mediaKind: "video",
    };
  }

  // ── Images: aspect_ratio + resolution (1k/2k). Never send `size`. ──
  const models = [
    req.model,
    req.quality === "quality" ? "grok-imagine-image-quality" : "grok-imagine-image",
    "grok-imagine-image-quality",
    "grok-imagine-image",
    "grok-2-image",
    "grok-2-image-1212",
  ].filter(Boolean) as string[];
  const aspect = imageAspectRatio(req.aspect);
  const resolution = req.quality === "quality" ? "2k" : "1k";
  let lastErr = "image generation failed";

  for (const model of models) {
    try {
      const body: Record<string, unknown> = {
        model,
        prompt: fullPrompt,
        n,
        response_format: "b64_json",
        resolution,
      };
      if (aspect) body.aspect_ratio = aspect;
      // Reference / edit when API accepts image url
      if (req.referenceDataUrl) {
        body.image = { url: req.referenceDataUrl };
      }
      const res = await fetch(`${XAI_BASE}/images/generations`, {
        method: "POST",
        headers,
        body: JSON.stringify(body),
      });
      const data = (await res.json().catch(() => ({}))) as {
        error?: { message?: string } | string;
        data?: Array<{ b64_json?: string; b64?: string; url?: string; image?: string }>;
        model?: string;
      };
      if (!res.ok) {
        lastErr =
          typeof data.error === "string"
            ? data.error
            : data.error?.message || `xAI image ${res.status} (${model})`;
        // Retry without resolution if unknown field
        if (/resolution|argument not supported/i.test(lastErr)) {
          const body2 = { ...body };
          delete body2.resolution;
          const res2 = await fetch(`${XAI_BASE}/images/generations`, {
            method: "POST",
            headers,
            body: JSON.stringify(body2),
          });
          const data2 = (await res2.json().catch(() => ({}))) as typeof data;
          if (res2.ok) {
            Object.assign(data, data2);
          } else {
            continue;
          }
        } else {
          continue;
        }
      }
      const row = data.data?.[0];
      const b64 = row?.b64_json || row?.b64 || row?.image || "";
      if (b64) {
        return {
          ok: true,
          imageDataUrl: b64.startsWith("data:") ? b64 : `data:image/png;base64,${b64}`,
          model: data.model || model,
          source: "xai",
          mediaKind: "image",
        };
      }
      if (row?.url) {
        return {
          ok: true,
          imageDataUrl: row.url,
          model: data.model || model,
          source: "xai",
          mediaKind: "image",
        };
      }
      lastErr = "empty image response";
    } catch (e) {
      lastErr = e instanceof Error ? e.message : "network error";
    }
  }
  return { ok: false, error: lastErr, mediaKind: "image" };
}


/** Grok Speech-to-Text (batch). audioBase64 = raw file bytes as base64, with mime. */
export async function callXaiStt(req: {
  accessToken?: string;
  apiKey?: string;
  /** base64 of audio file (webm/wav/ogg/mp3) */
  audioBase64: string;
  mimeType?: string;
  language?: string;
  fileName?: string;
}): Promise<{ ok: boolean; text?: string; error?: string }> {
  const auth = resolveBearer({
    accessToken: req.accessToken,
    apiKey: req.apiKey,
    messages: [],
  });
  if (!auth) {
    return { ok: false, error: "Not connected — sign in to Grok for voice transcription" };
  }
  const b64 = String(req.audioBase64 || "").replace(/^data:[^;]+;base64,/, "");
  if (!b64) return { ok: false, error: "empty audio" };
  let bytes: Uint8Array;
  try {
    if (typeof Buffer !== "undefined") {
      bytes = Buffer.from(b64, "base64");
    } else {
      const bin = atob(b64);
      bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    }
  } catch {
    return { ok: false, error: "invalid audio base64" };
  }
  const mime = req.mimeType || "audio/webm";
  const name =
    req.fileName ||
    (mime.includes("wav")
      ? "speech.wav"
      : mime.includes("ogg")
        ? "speech.ogg"
        : mime.includes("mp4") || mime.includes("m4a")
          ? "speech.m4a"
          : "speech.webm");
  const form = new FormData();
  form.append("format", "true");
  form.append("language", req.language || "en");
  // Node 18+ / browsers: File or Blob
  const ab = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
  const file =
    typeof File !== "undefined"
      ? new File([ab], name, { type: mime })
      : new Blob([ab], { type: mime });
  form.append("file", file, name);
  try {
    const res = await fetch(`${XAI_BASE}/stt`, {
      method: "POST",
      headers: { authorization: `Bearer ${auth.bearer}` },
      body: form,
    });
    const data = (await res.json().catch(() => ({}))) as {
      error?: { message?: string } | string;
      text?: string;
      transcript?: string;
      result?: string;
    };
    if (!res.ok) {
      const err =
        typeof data.error === "string"
          ? data.error
          : data.error?.message || `STT ${res.status}`;
      return { ok: false, error: err };
    }
    const text = (data.text || data.transcript || data.result || "").trim();
    if (!text) return { ok: false, error: "empty transcript" };
    return { ok: true, text };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "STT network error" };
  }
}
