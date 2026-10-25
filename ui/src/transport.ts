// How the UI reaches GrokHub (cross-platform Phase 5). Inside the Tauri shell
// it calls the shell's commands directly. In a plain browser (the web app) it
// is a remote for GrokHub running on your computer: the same commands go over
// HTTP to that desktop, which serves this page, and events arrive on one
// server-sent event stream. The protocol is in docs/cross-platform/README.md.
//
// The pairing code is a secret. It lives in this module's memory only, never
// in localStorage, sessionStorage, IndexedDB or a cookie, so a reload asks for
// it again.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const REMOTE_PREFIX = "/remote/v1";
export const REMOTE_KIND = "grokhub-remote";

export const NOT_PAIRED = "Pair this browser with GrokHub on your computer first.";
export const CODE_REFUSED = "GrokHub on your computer refused that pairing code. Pair again.";
export const NOT_GROKHUB = "That address isn't GrokHub on your computer.";
export const UNREACHABLE = "Can't reach GrokHub on your computer. Is it open?";

/** True inside the Tauri shell; false in a plain browser. */
export function inShell(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

interface Link {
  base: string;
  code: string;
  stream: AbortController;
}

let link: Link | null = null;
const handlers = new Map<string, Set<(payload: unknown) => void>>();
const pairListeners = new Set<(paired: boolean) => void>();

export function paired(): boolean {
  return link !== null;
}

/** Called with false when the desktop drops the pairing (or `unpair` runs). */
export function onPairChange(fn: (paired: boolean) => void): () => void {
  pairListeners.add(fn);
  return () => pairListeners.delete(fn);
}

function setLink(next: Link | null) {
  link?.stream.abort();
  link = next;
  for (const fn of pairListeners) fn(next !== null);
}

export function unpair() {
  setLink(null);
}

function auth(code: string): HeadersInit {
  return { Authorization: `Bearer ${code}` };
}

/**
 * Checks `base` is GrokHub and accepts `code`, then opens the event stream.
 * Resolves once the stream is open, so no chat event is missed.
 */
export async function pair(code: string, base: string = window.location.origin): Promise<void> {
  const trimmed = code.trim();
  if (!trimmed) throw new Error("Enter the pairing code GrokHub shows.");
  const root = base.replace(/\/+$/, "");
  let hello: Response;
  try {
    hello = await fetch(`${root}${REMOTE_PREFIX}/hello`, { credentials: "omit", cache: "no-store" });
  } catch {
    throw new Error(UNREACHABLE);
  }
  const info = (await hello.json().catch(() => null)) as { kind?: string } | null;
  if (!hello.ok || info?.kind !== REMOTE_KIND) throw new Error(NOT_GROKHUB);
  const stream = new AbortController();
  const first = await openStream(root, trimmed, stream.signal);
  if (first?.status === 401) throw new Error(CODE_REFUSED);
  if (!first?.ok || !first.body) throw new Error(UNREACHABLE);
  setLink({ base: root, code: trimmed, stream });
  void keepStream(root, trimmed, stream.signal, first);
}

function openStream(base: string, code: string, signal: AbortSignal): Promise<Response | null> {
  return fetch(`${base}${REMOTE_PREFIX}/events`, {
    headers: auth(code),
    credentials: "omit",
    cache: "no-store",
    signal,
  }).catch(() => null);
}

/** Reads the stream; if it drops while still paired, reopens it every second. */
async function keepStream(base: string, code: string, signal: AbortSignal, first: Response) {
  let res: Response | null = first;
  while (!signal.aborted) {
    if (res?.ok && res.body) await readFrames(res.body);
    if (signal.aborted) return;
    await new Promise((done) => setTimeout(done, 1000));
    if (signal.aborted) return;
    res = await openStream(base, code, signal);
    if (res?.status === 401) {
      if (link?.stream.signal === signal) setLink(null);
      return;
    }
  }
}

async function readFrames(body: ReadableStream<Uint8Array>) {
  const reader = body.getReader();
  const text = new TextDecoder();
  let buffered = "";
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      buffered += text.decode(value, { stream: true }).replace(/\r\n/g, "\n");
      let cut = buffered.indexOf("\n\n");
      while (cut >= 0) {
        dispatch(buffered.slice(0, cut));
        buffered = buffered.slice(cut + 2);
        cut = buffered.indexOf("\n\n");
      }
    }
  } catch {
    // Aborted or dropped.
  }
}

/** One `data:` frame: `{"event": "chat://event", "payload": …}`. */
export function dispatch(frame: string) {
  const data = frame
    .split("\n")
    .filter((line) => line.startsWith("data:"))
    .map((line) => line.slice(5).trimStart())
    .join("\n");
  if (!data) return;
  let parsed: { event?: unknown; payload?: unknown };
  try {
    parsed = JSON.parse(data) as typeof parsed;
  } catch {
    return;
  }
  if (typeof parsed.event !== "string") return;
  for (const fn of handlers.get(parsed.event) ?? []) fn(parsed.payload);
}

/**
 * Runs a GrokHub command, in the shell or on the paired desktop. Like Tauri's
 * `invoke`, a failure rejects with the plain message string.
 */
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (inShell()) return invoke<T>(command, args);
  const now = link;
  if (!now) throw NOT_PAIRED;
  let res: Response;
  try {
    res = await fetch(`${now.base}${REMOTE_PREFIX}/call/${encodeURIComponent(command)}`, {
      method: "POST",
      headers: { ...auth(now.code), "Content-Type": "application/json" },
      body: JSON.stringify(args ?? {}),
      credentials: "omit",
      cache: "no-store",
    });
  } catch {
    throw UNREACHABLE;
  }
  if (res.status === 401) {
    if (link === now) setLink(null);
    throw CODE_REFUSED;
  }
  const reply = (await res.json().catch(() => null)) as
    | { ok: true; value: T }
    | { ok: false; error: string }
    | null;
  if (!reply) throw UNREACHABLE;
  if (!reply.ok) throw reply.error;
  return reply.value;
}

/** Listens for a GrokHub event, in the shell or from the paired desktop. */
export function subscribe<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (inShell()) return listen<T>(event, (e) => handler(e.payload));
  const fn = handler as (payload: unknown) => void;
  const set = handlers.get(event) ?? new Set();
  set.add(fn);
  handlers.set(event, set);
  return Promise.resolve(() => {
    set.delete(fn);
  });
}
