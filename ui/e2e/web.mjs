// Web app smoke test (cross-platform Phase 5): a mocked desktop serves the
// built UI (ui/dist) and the remote API on one origin, the way GrokHub on
// your computer will. Chromium opens the pairing link, sends a chat, answers
// the tool approval the mock asks for, and gets the reply. Then it checks the
// pairing code is in no browser store and a reload asks to pair again.
//
//   npm run build --prefix ui && node ui/e2e/web.mjs
//
// The protocol is in docs/cross-platform/README.md (Phase 5).

import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const { chromium } = createRequire(import.meta.url)("playwright");

const PORT = 4620;
const ORIGIN = `http://127.0.0.1:${PORT}`;
// An obvious fake; the real desktop makes a random one.
const CODE = "pair-abcdefghijklmnop";
const PROMPT = "web smoke: say hello";
const REPLY = "Hello from the web smoke test";
const TOOL = "write_file";
const DIST = resolve(fileURLToPath(new URL("../dist", import.meta.url)));
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".png": "image/png",
};

const STATE = {
  calls: [],
  streams: new Set(),
  permission: null,
};

function settings() {
  return {
    model: "grok-4.7",
    closeToTray: true,
    hasApiKey: true,
    workspace: "",
    workspacePath: "/home/viper/GrokHub",
    approvals: "ask",
    desktopControl: false,
  };
}

function emit(event, payload) {
  const frame = `data: ${JSON.stringify({ event, payload })}\n\n`;
  for (const res of STATE.streams) res.write(frame);
}

function chat(conversationId, event) {
  emit("chat://event", { conversationId, event });
}

/** One turn: ask to run a tool, wait for the answer, then reply in two parts. */
async function sendChat({ conversationId, text }) {
  if (text !== PROMPT) return { ok: false, error: `unexpected prompt: ${text}` };
  const answered = new Promise((done) => (STATE.permission = { id: "call-1", done }));
  chat(conversationId, {
    kind: "permission",
    id: "call-1",
    title: TOOL,
    action: "notes.txt",
    reason: "It changes a file.",
  });
  const answer = await answered;
  chat(conversationId, {
    kind: "tool",
    id: "call-1",
    title: TOOL,
    status: answer === "deny" ? "denied" : "done",
    detail: "",
  });
  chat(conversationId, { kind: "text", text: REPLY.slice(0, 9) });
  chat(conversationId, { kind: "text", text: REPLY.slice(9) });
  chat(conversationId, { kind: "done", stop: "end_turn" });
  return { ok: true, value: "end_turn" };
}

const COMMANDS = {
  get_settings: () => ({ ok: true, value: settings() }),
  send_chat: sendChat,
  answer_permission: ({ callId, answer }) => {
    if (STATE.permission?.id !== callId) return { ok: false, error: "no such approval" };
    STATE.permission.done(answer);
    STATE.permission = null;
    return { ok: true, value: null };
  },
  cancel_chat: () => ({ ok: true, value: null }),
  close_chat: () => ({ ok: true, value: null }),
  list_automations: () => ({ ok: true, value: [] }),
  get_memory: () => ({
    ok: true,
    value: { global: "", workspace: "", globalPath: "", workspacePath: "", amr: false },
  }),
  list_connectors: () => ({ ok: true, value: [] }),
};

function json(res, status, body) {
  const text = JSON.stringify(body);
  res.writeHead(status, {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(text),
    "Cache-Control": "no-store",
  });
  res.end(text);
}

function authorized(req) {
  return req.headers.authorization === `Bearer ${CODE}`;
}

function serveFile(req, res) {
  const path = decodeURIComponent(new URL(req.url, ORIGIN).pathname);
  let file = normalize(join(DIST, path));
  if (!file.startsWith(DIST + sep) && file !== DIST) return json(res, 404, {});
  if (!existsSync(file) || statSync(file).isDirectory()) file = join(DIST, "index.html");
  res.writeHead(200, { "Content-Type": TYPES[extname(file)] ?? "application/octet-stream" });
  createReadStream(file).pipe(res);
}

function startDesktop() {
  const server = createServer((req, res) => {
    const { pathname } = new URL(req.url, ORIGIN);
    if (pathname === "/remote/v1/hello") return json(res, 200, { kind: "grokhub-remote", version: 1 });
    if (pathname === "/remote/v1/events") {
      if (!authorized(req)) return json(res, 401, { ok: false, error: "pair first" });
      res.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-store" });
      res.write(": open\n\n");
      STATE.streams.add(res);
      req.on("close", () => STATE.streams.delete(res));
      return;
    }
    const called = /^\/remote\/v1\/call\/([a-z_]+)$/.exec(pathname);
    if (called) {
      STATE.calls.push({ command: called[1], auth: req.headers.authorization, method: req.method });
      if (req.method !== "POST") return json(res, 405, { ok: false, error: "POST only" });
      if (!authorized(req)) return json(res, 401, { ok: false, error: "pair first" });
      let body = "";
      req.on("data", (chunk) => (body += chunk));
      req.on("end", async () => {
        const run = COMMANDS[called[1]];
        if (!run) return json(res, 200, { ok: false, error: `not on the web: ${called[1]}` });
        json(res, 200, await run(JSON.parse(body || "{}")));
      });
      return;
    }
    serveFile(req, res);
  });
  return new Promise((ok) => server.listen(PORT, "127.0.0.1", () => ok(server)));
}

function check(ok, message) {
  if (!ok) throw new Error(message);
}

async function main() {
  check(existsSync(join(DIST, "index.html")), "build the UI first: npm run build --prefix ui");
  const server = await startDesktop();
  console.log(`mock desktop at ${ORIGIN}`);
  const browser = await chromium.launch();
  try {
    const context = await browser.newContext();
    const page = await context.newPage();
    page.on("console", (msg) => {
      if (msg.type() === "error") console.log(`browser: ${msg.text()}`);
    });

    await page.goto(`${ORIGIN}/#code=${CODE}`);
    const box = page.getByRole("textbox", { name: "Message" });
    await box.waitFor({ timeout: 30000 });
    check(!page.url().includes("code="), `the pairing code stayed in the address: ${page.url()}`);
    const tabs = await page.locator("nav.tabs button").allTextContents();
    console.log(`paired; tabs=${tabs.join(",")}`);
    check(!tabs.includes("Pulse"), "the web app showed Pulse");

    await box.fill(PROMPT);
    await page.locator("form.composer button[type='submit']").click();
    const card = page.getByRole("group", { name: `Approve ${TOOL}` });
    await card.waitFor({ timeout: 30000 });
    await card.getByRole("button", { name: "Allow once" }).click();
    const reply = page.locator(".bubble.assistant:not(.streaming)");
    await reply.filter({ hasText: REPLY }).waitFor({ timeout: 30000 });
    console.log(`reply: ${await reply.first().textContent()}`);

    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByText("Change settings and the API key there.").waitFor();
    check((await page.getByLabel("API key").count()) === 0, "the web app offered to set the key");

    const stores = await page.evaluate(async () => ({
      local: localStorage.length,
      session: sessionStorage.length,
      cookie: document.cookie,
      idb: (await indexedDB.databases()).length,
    }));
    console.log(`browser stores: ${JSON.stringify(stores)}`);
    check(
      stores.local === 0 && stores.session === 0 && stores.cookie === "" && stores.idb === 0,
      "the web app kept something in browser storage",
    );
    check((await context.cookies()).length === 0, "the web app set a cookie");

    const commands = STATE.calls.map((c) => c.command);
    for (const want of ["get_settings", "send_chat", "answer_permission"]) {
      check(commands.includes(want), `the desktop never got ${want} (${commands.join(", ")})`);
    }
    check(
      STATE.calls.every((c) => c.auth === `Bearer ${CODE}` && c.method === "POST"),
      "a command went without the pairing code",
    );
    console.log(`the desktop got ${STATE.calls.length} command(s): ${[...new Set(commands)].join(", ")}`);

    await page.reload();
    await page.getByLabel("Pairing code").waitFor({ timeout: 30000 });
    console.log("a reload asks to pair again");
    console.log("web smoke ok");
  } finally {
    await browser.close();
    for (const res of STATE.streams) res.end();
    server.close();
  }
}

main().catch((err) => {
  console.error(err.message);
  process.exit(1);
});
