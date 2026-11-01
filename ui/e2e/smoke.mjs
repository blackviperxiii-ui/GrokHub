// tauri-driver smoke test: start a mock model server, launch the built shell
// through WebDriver and send one chat. The mock model asks to write a file:
// the test checks that nothing is written and the model is not called again
// until Allow once is clicked, then waits for the file and the mocked reply.
//
//   node e2e/smoke.mjs <path to grokhub-shell binary>
//
// Needs `tauri-driver` on PATH (cargo install tauri-driver) and the platform
// driver: WebKitWebDriver on Linux, msedgedriver on Windows. macOS has none.
// Set SMOKE_NATIVE_DRIVER to that driver's path when it is not on PATH, and
// SMOKE_SCREENSHOT=<file.png> to keep a screenshot of the rendered reply.
// SMOKE_SHOTS=<folder> to keep one screenshot per page the run walks through.
// SMOKE_WEB=1 adds the web access step, which needs Playwright's Chromium.

import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { connect as tlsConnect } from "node:tls";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPLY = "Wrote the note for the smoke test";
const PROMPT = "write a note from the smoke test";
const NOTE = "smoke-note.txt";
const NOTE_TEXT = "written after Allow once";
const DRIVER = "http://127.0.0.1:4444";

const binary = process.argv[2];
if (!binary) {
  console.error("usage: node e2e/smoke.mjs <path to grokhub-shell binary>");
  process.exit(2);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function sse(events) {
  return events.map((e) => `data: ${JSON.stringify(e)}\n\n`).join("");
}

function toolCall(callId, name, args) {
  const item = { type: "function_call", call_id: callId, name, arguments: JSON.stringify(args) };
  return [
    { type: "response.output_item.done", item },
    {
      type: "response.completed",
      response: { usage: { input_tokens: 3, output_tokens: 5 }, output: [item] },
    },
  ];
}

// The loop holds a change to its done list ("the asked change is made",
// "result checked"), as with a real model: write the note, re-read it, tick
// the checklist, then reply in two pieces.
function mockReply(callNumber, body) {
  // The Pulse writer's two calls: the taste note, then one post.
  if (body.includes("You keep a short note on what one person likes")) {
    return textReply("They like smoke tests and shell news.");
  }
  if (body.includes("You write one post for a person's news feed")) {
    return textReply(JSON.stringify(PULSE_POST));
  }
  if (body.includes("You suggest ideas for one person's Pulse")) {
    return textReply(JSON.stringify([PULSE_IDEA]));
  }
  if (body.includes(WEB_PROMPT)) return textReply(WEB_REPLY);
  if (body.includes(DESK_PROMPT)) {
    if (!body.includes("smoke-desk")) return toolCall("smoke-desk", "type", { text: DESK_TEXT });
    return textReply(DESK_REPLY);
  }
  // Checked after the desktop prompt, whose body carries this turn as history.
  if (body.includes(PLAN_PROMPT)) {
    if (body.includes("smoke-plan-exit")) return textReply(PLAN_REPLY);
    if (body.includes("smoke-plan-write")) {
      return toolCall("smoke-plan-exit", "exit_plan_mode", { plan: PLAN_TEXT });
    }
    return toolCall("smoke-plan-write", "write", { path: "plan-note.txt", content: "no" });
  }
  if (callNumber === 1) return toolCall("smoke-write", "write", { path: NOTE, content: NOTE_TEXT });
  if (callNumber === 2) return toolCall("smoke-read", "read_file", { target_file: NOTE });
  if (callNumber === 3) {
    return toolCall("smoke-todo", "todo_write", {
      todos: [
        { id: "1", content: "the asked change is made", status: "completed" },
        { id: "2", content: "result checked", status: "completed" },
      ],
    });
  }
  return textReply(REPLY);
}

function textReply(text) {
  const half = text.length >> 1;
  return [
    { type: "response.output_text.delta", delta: text.slice(0, half) },
    { type: "response.output_text.delta", delta: text.slice(half) },
    {
      type: "response.completed",
      response: { usage: { input_tokens: 3, output_tokens: 5 }, output: [] },
    },
  ];
}

let mockSeen = [];

/** True when some model call carried the automation's instructions. */
function mockRunPrompt() {
  return mockSeen.some((c) => c.body.includes(AUTOMATION));
}

function startMock() {
  const seen = [];
  mockSeen = seen;
  const server = createServer((req, res) => {
    if (req.url.startsWith("/pulse/")) return pulseFixture(req, res);
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      seen.push({ path: req.url, auth: req.headers.authorization, body });
      const calls = seen.filter((c) => c.path === "/v1/responses").length;
      const payload = sse(mockReply(calls, body));
      res.writeHead(200, {
        "Content-Type": "text/event-stream",
        "Content-Length": Buffer.byteLength(payload),
        Connection: "close",
      });
      res.end(payload);
    });
  });
  return new Promise((ok) =>
    server.listen(0, "127.0.0.1", () => ok({ server, seen, port: server.address().port })),
  );
}

// The Pulse writer's network (GROKHUB_SHELL_PULSE_NET): a Hacker News search
// with one story about the smoke topic, the story's page, and its image.
const PULSE_TOPIC = "smoke news";
const PULSE_STORY = "https://smoke.example.com/story";
const PULSE_IMAGE = "https://smoke.example.com/hero.jpg";
const PULSE_TITLE = "Smoke news: the shell writes its own feed";
const PULSE_POST = {
  title: PULSE_TITLE,
  snippet: `The shell now writes [its own feed posts](${PULSE_STORY}) from free sources.`,
  icon: "🚀",
  why: "You asked for smoke news.",
};

// The idea writer's one idea, after the feed run.
const PULSE_IDEA = {
  title: "Smoke news weekly digest",
  description: "I can gather the week's smoke news into one Friday digest.",
  emoji: "🛠️",
  category: "productivity",
  action: "every Friday at 5, gather the week's smoke news into a digest",
};

function pulseFixture(req, res) {
  const url = new URL(req.url, "http://127.0.0.1");
  const asked = url.searchParams.get("url") ?? "";
  const kind = url.pathname.slice("/pulse/".length);
  let type = "text/html";
  let body = null;
  let askedHost = "";
  try {
    askedHost = new URL(asked).hostname;
  } catch {}
  if (kind === "source" && (askedHost === "hn.algolia.com" || askedHost.endsWith(".hn.algolia.com"))) {
    type = "application/json";
    body = JSON.stringify({
      hits: [
        {
          title: "Smoke news: a shell that writes its own feed",
          url: PULSE_STORY,
          points: 99,
          num_comments: 3,
          created_at_i: Math.floor(Date.now() / 1000) - 600,
          objectID: "1",
        },
      ],
    });
  } else if (kind === "page" && asked === PULSE_STORY) {
    body = `<html><head><meta property="og:image" content="${PULSE_IMAGE}"></head><body><p>Smoke news.</p></body></html>`;
  } else if (kind === "image" && asked === PULSE_IMAGE) {
    type = "image/jpeg";
    body = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "fixtures", "smoke-hero.jpg"));
  }
  if (body === null) {
    res.writeHead(404, { Connection: "close" });
    return res.end();
  }
  res.writeHead(200, { "Content-Type": type, Connection: "close" });
  res.end(body);
}

async function wd(method, path, body) {
  const res = await fetch(`${DRIVER}${path}`, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json();
  if (!res.ok) throw new Error(`${method} ${path}: ${JSON.stringify(json.value)}`);
  return json.value;
}

async function waitFor(what, fn, timeoutMs = 30000) {
  const end = Date.now() + timeoutMs;
  let last;
  while (Date.now() < end) {
    try {
      const value = await fn();
      if (value) return value;
    } catch (err) {
      last = err;
    }
    await sleep(250);
  }
  throw new Error(`timed out waiting for ${what}${last ? `: ${last.message}` : ""}`);
}

// New cards rise in from opacity 0 (styles.css rise-in), and WebDriver reads
// an invisible element's text as "". Read until the card is visible and says
// what it should, then return what it said last.
async function cardText(s, card, ok, timeoutMs = 5000) {
  const end = Date.now() + timeoutMs;
  let text = "";
  while (Date.now() < end) {
    text = await wd("GET", `${s}/element/${card}/text`);
    if (ok(text)) break;
    await sleep(100);
  }
  return text;
}

const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";
const AUTOMATION = "every day at 7am say hello from the smoke test";
const MEMORY_NOTE = "the smoke test likes kumquats";
const PULSE_ACTION = "close every done card on the smoke workboard";
const JOURNAL_QUESTION = "Should the smoke test ship the shell first?";
const PLAN_PROMPT = "plan a tidy of the smoke notes";
const PLAN_TEXT = "1. Read the smoke notes. 2. Merge the duplicates.";
const PLAN_REPLY = "Plan approved, ready to tidy";
const DESK_PROMPT = "clear out the old folder from the terminal";
const DESK_TEXT = "rm -rf ~/old";
const DESK_REPLY = "Left the old folder alone";
const WEB_PROMPT = "hello from the browser smoke test";
const WEB_REPLY = "Hello, browser";

async function find(s, css) {
  const el = await wd("POST", `${s}/element`, { using: "css selector", value: css });
  return el[ELEMENT];
}

async function click(s, css) {
  await wd("POST", `${s}/element/${await find(s, css)}/click`, {});
}

/** A top tab by its label, so adding a tab moves no test. */
// Waits for the tab, since Pulse shows only once the capability report loads.
async function tab(s, label) {
  const el = await waitFor(`the ${label} tab`, () =>
    wd("POST", `${s}/element`, {
      using: "xpath",
      value: `//nav[contains(@class,'tabs')]/button[normalize-space()='${label}']`,
    }),
  );
  await wd("POST", `${s}/element/${el[ELEMENT]}/click`, {});
  await onView(s, label);
}

/** Waits until the top tab `label` is the one pressed. A page stays mounted
 * once opened, so finding its content proves nothing about what shows. */
async function onView(s, label) {
  await waitFor(`the ${label} view`, () =>
    wd("POST", `${s}/element`, {
      using: "xpath",
      value: `//nav[contains(@class,'tabs')]/button[normalize-space()='${label}'][@aria-pressed='true']`,
    }),
  );
}

/** Scrolls the element into view and is true once it sits still there for
 * 300 ms, so a click lands on it. */
async function settled(s, el) {
  const where = () =>
    wd("POST", `${s}/execute/sync`, {
      script:
        "arguments[0].scrollIntoView({ block: 'center' }); const r = arguments[0].getBoundingClientRect(); return [r.x, r.y];",
      args: [{ [ELEMENT]: el }],
    });
  const [x, y] = await where();
  await sleep(300);
  const [x2, y2] = await where();
  return x === x2 && y === y2;
}

/** A Settings section by its label in the Settings nav. */
async function section(s, label) {
  const el = await waitFor(`the ${label} section`, () =>
    wd("POST", `${s}/element`, {
      using: "xpath",
      value: `//nav[contains(@class,'settings-nav')]/button[normalize-space()='${label}']`,
    }),
  );
  await wd("POST", `${s}/element/${el[ELEMENT]}/click`, {});
}

async function textOf(s, css) {
  return wd("GET", `${s}/element/${await find(s, css)}/text`);
}

/** With SMOKE_SHOTS set, a screenshot of the window as it is now. */
async function shot(s, name) {
  const dir = process.env.SMOKE_SHOTS;
  if (!dir) return;
  await sleep(400);
  mkdirSync(dir, { recursive: true });
  const png = await wd("GET", `${s}/screenshot`);
  writeFileSync(join(dir, `${name}.png`), Buffer.from(png, "base64"));
}

/** Grow the window so Settings → Local AI fits Get engine + draft offer. */
async function setWindowSize(s, width, height) {
  await wd("POST", `${s}/window/rect`, { x: 0, y: 0, width, height }).catch(() => undefined);
}

async function scrollIntoView(s, css) {
  const id = await find(s, css);
  await wd("POST", `${s}/execute/sync`, {
    script: "arguments[0].scrollIntoView({block:'start', inline:'nearest'});",
    args: [{ [ELEMENT]: id }],
  }).catch(() => undefined);
}

/** True when this run should capture Windows Local AI engine-install proof. */
function windowsEngineProof() {
  return Boolean(process.env.SMOKE_SHOTS) && process.platform === "win32";
}

// Automations: add one from plain text, check it lands in the cabin's
// automations.json, then Run now and wait for the mocked reply on its row.
async function automations(s, config) {
  await tab(s, "Automations");
  const input = await waitFor("the automations page", () => find(s, "input[aria-label='New automation']"));
  await wd("POST", `${s}/element/${input}/value`, { text: AUTOMATION });
  await click(s, "form.add button[type='submit']");
  const summary = await waitFor("the new automation", async () => {
    const t = await textOf(s, ".card.automation .card-status");
    return t.startsWith("daily at 07:00") ? t : null;
  });
  console.log(`automation added: ${JSON.stringify(summary)}`);
  const saved = JSON.parse(readFileSync(join(config, "automations.json"), "utf8"));
  if (saved.length !== 1 || saved[0].schedule !== "daily" || saved[0].time !== "07:00") {
    throw new Error(`automations.json holds ${JSON.stringify(saved)}`);
  }

  await click(s, ".card.automation .card-buttons button");
  const result = await waitFor("the run's reply", async () => {
    const t = await textOf(s, ".card.automation .card-detail");
    return t.includes(REPLY) ? t : null;
  });
  await shot(s, "automations");
  console.log(`run now replied: ${JSON.stringify(result)}`);
  const after = JSON.parse(readFileSync(join(config, "automations.json"), "utf8"));
  if (after[0].runCount !== 1 || after[0].health?.outcome !== "ok") {
    throw new Error(`the run was not recorded: ${JSON.stringify(after[0])}`);
  }
  const ran = mockRunPrompt();
  if (!ran) throw new Error("the model never got the automation's instructions");
}

// Memory: Remember a line, check it lands in the work folder's MEMORY.md,
// then search for it the way chat recall would.
async function memory(s, workspace) {
  await tab(s, "Memory");
  const input = await waitFor("the memory page", () => find(s, "input[aria-label='New note']"));
  await wd("POST", `${s}/element/${input}/value`, { text: MEMORY_NOTE });
  await click(s, ".memory form.add button[type='submit']");
  // Status is role=status; AMR-on installs also show a banner in p.note.
  await waitFor("the remembered note", async () => {
    const t = await textOf(s, ".memory [role='status']");
    return t.startsWith("Remembered in workspace") ? t : null;
  });
  const file = readFileSync(join(workspace, "MEMORY.md"), "utf8");
  if (file !== `${MEMORY_NOTE}\n`) throw new Error(`MEMORY.md says ${JSON.stringify(file)}`);
  console.log(`note remembered: ${JSON.stringify(file)}`);

  const search = await find(s, "input[aria-label='Search memory']");
  await wd("POST", `${s}/element/${search}/value`, { text: "kumquats" });
  await click(s, ".memory form.add:last-of-type button[type='submit']");
  const hit = await waitFor("the search hit", async () => {
    const t = await textOf(s, ".card.hit .card-detail");
    return t === MEMORY_NOTE ? t : null;
  });
  await shot(s, "memory");
  console.log(`search found: ${JSON.stringify(hit)}`);
}

// Connectors: an mcp.json entry for the fake stdio server shows as not
// checked, connects with its one tool on Check all, and leaves mcp.json only
// after the Disconnect question is answered.
async function connectors(s, config) {
  const script = join(dirname(fileURLToPath(import.meta.url)), "fake-mcp.mjs");
  const mcpFile = join(config, "mcp.json");
  writeFileSync(
    mcpFile,
    JSON.stringify({ mcpServers: { box: { command: process.execPath, args: [script] } } }),
  );
  await tab(s, "Connectors");
  await waitFor("the connector row", async () => {
    const t = await textOf(s, ".card.connector .card-status");
    return t === "Not checked" ? t : null;
  });
  await click(s, ".connectors > .card-buttons button");
  const status = await waitFor("the connected row", async () => {
    const t = await textOf(s, ".card.connector .card-status");
    return t === "Connected · 1 tool" ? t : null;
  });
  await shot(s, "connectors");
  console.log(`connector checked: ${JSON.stringify(status)}`);

  await click(s, ".card.connector button.quiet.danger");
  await waitFor("the disconnect question", () => find(s, ".card.connector [role='group']"));
  if (!readFileSync(mcpFile, "utf8").includes("fake-mcp.mjs")) {
    throw new Error("the entry left mcp.json before the question was answered");
  }
  await click(s, ".card.connector [role='group'] button.danger");
  await waitFor("the disconnected status", async () => {
    const t = await textOf(s, ".connectors p.status");
    return t === "Disconnected box" ? t : null;
  });
  const saved = readFileSync(mcpFile, "utf8");
  if (saved.includes("fake-mcp.mjs")) throw new Error(`mcp.json still has box: ${saved}`);
  console.log("connector disconnected after the OK");
}

// Pulse: a post and an idea in the cabin's updates.json show on Feed and
// For You. Love on the post writes the ledger line to memory/pulse-ledger.md.
// Saving the feed instructions makes the writer post at once: it reads the
// mocked sources, asks the mocked model, and caches the story's image, which
// opens in the lightbox. Delete… and its confirm take the first post away. After the post
// the idea writer adds one idea; its menu holds Let's do it, More like this
// and Not interested, and More like this is kept for the next run. A click on
// the seeded idea's row puts its action in the chat box, unsent.
async function pulse(s, config) {
  const now = Date.now();
  const cards = [
    {
      id: "smoke-post",
      kind: "automation_done",
      title: "Smoke board summary",
      body: "Three cards moved to done.",
      createdAt: now - 120000,
      status: "unread",
    },
    {
      id: "smoke-idea",
      kind: "idea",
      title: "Tidy the smoke workboard",
      body: "Close the done cards.",
      createdAt: now - 60000,
      status: "unread",
      prompt: PULSE_ACTION,
    },
  ];
  writeFileSync(join(config, "updates.json"), JSON.stringify(cards));
  writeFileSync(
    join(config, "interests.json"),
    JSON.stringify({ topics: [{ topic: PULSE_TOPIC, source: "explicit", weight: 60 }], seeded: true }),
  );
  await tab(s, "Pulse");
  await waitFor("the feed post", async () => {
    const t = await textOf(s, ".pulse .feed-card .feed-title");
    return t === "Smoke board summary" ? t : null;
  });
  const post = "//li[@aria-label='Smoke board summary']";
  await click(s, ".pulse .feed-card .feed-foot button[aria-label='Love']");
  await waitFor("the loved post", async () => {
    const el = await find(s, ".pulse .feed-card .feed-foot button[aria-label='Love']");
    return (await wd("GET", `${s}/element/${el}/attribute/aria-pressed`)) === "true";
  });
  const ledger = readFileSync(join(config, "memory", "pulse-ledger.md"), "utf8");
  if (!ledger.includes('pulse: liked "Smoke board summary" reason=liked')) {
    throw new Error(`pulse-ledger.md says ${JSON.stringify(ledger)}`);
  }
  console.log(`ledger line: ${JSON.stringify(ledger.trim())}`);

  await click(s, ".pulse button[aria-label='Edit feed instructions']");
  const prompt = await waitFor("the instructions box", () => find(s, "#feed-prompt"));
  await waitFor("the default instructions", async () =>
    (await wd("GET", `${s}/element/${prompt}/property/value`)) ? true : null,
  );
  await wd("POST", `${s}/element/${prompt}/clear`, {});
  await wd("POST", `${s}/element/${prompt}/value`, { text: "Smoke news, please." });
  await shot(s, "pulse-instructions");
  const save = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: "//div[@aria-label='Edit feed instructions']//button[normalize-space()='Save']",
  });
  await wd("POST", `${s}/element/${save[ELEMENT]}/click`, {});
  await waitFor(
    "the written post",
    async () => {
      const el = await wd("POST", `${s}/element`, {
        using: "xpath",
        value: `//li[@aria-label=${JSON.stringify(PULSE_TITLE)}]//button[@aria-label='Open image 1']`,
      });
      return el[ELEMENT];
    },
    60000,
  );
  const state = JSON.parse(readFileSync(join(config, "pulse-writer.json"), "utf8"));
  if (state.prompt !== "Smoke news, please." || state.queued) {
    throw new Error(`pulse-writer.json says ${JSON.stringify(state)}`);
  }
  console.log(`writer posted ${JSON.stringify(PULSE_TITLE)} after the save`);
  await shot(s, "pulse");

  await click(s, ".pulse .media-item");
  await waitFor("the lightbox", () => find(s, ".lightbox img"));
  await shot(s, "pulse-lightbox");
  const close = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: "//div[contains(@class,'lightbox')]//button[normalize-space()='Close']",
  });
  await wd("POST", `${s}/element/${close[ELEMENT]}/click`, {});

  const more = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: `${post}//button[@aria-label='More']`,
  });
  await wd("POST", `${s}/element/${more[ELEMENT]}/click`, {});
  // The menu's Delete… asks first; the confirm row's Delete takes it away.
  const del = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: `${post}//button[@role='menuitem'][normalize-space()='Delete…']`,
  });
  await wd("POST", `${s}/element/${del[ELEMENT]}/click`, {});
  const sure = await waitFor("the delete confirm", async () => {
    const el = await wd("POST", `${s}/elements`, {
      using: "xpath",
      value: `${post}//div[contains(@class,'confirm-row')]//button[normalize-space()='Delete']`,
    });
    return el.length === 1 ? el[0][ELEMENT] : null;
  });
  await wd("POST", `${s}/element/${sure}/click`, {});
  await waitFor("the deleted post", async () => {
    const left = await wd("POST", `${s}/elements`, { using: "xpath", value: post });
    return left.length === 0;
  });
  console.log("delete took the post away");

  const ideas = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: "//div[@aria-label='Pulse view']/button[normalize-space()='For You']",
  });
  await wd("POST", `${s}/element/${ideas[ELEMENT]}/click`, {});
  // The idea writer ran after the feed post: its idea leads, with no header.
  const written = `//li[@aria-label=${JSON.stringify(PULSE_IDEA.title)}]`;
  await waitFor(
    "the written idea",
    async () => {
      await wd("POST", `${s}/element/${ideas[ELEMENT]}/click`, {});
      const el = await wd("POST", `${s}/elements`, { using: "xpath", value: written });
      return el.length === 1 ? true : null;
    },
    60000,
  );
  const ideaMenu = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: `${written}//button[@aria-label='More']`,
  });
  await wd("POST", `${s}/element/${ideaMenu[ELEMENT]}/click`, {});
  const items = await wd("POST", `${s}/elements`, {
    using: "xpath",
    value: `${written}//button[@role='menuitem']`,
  });
  const labels = [];
  for (const item of items) labels.push(await wd("GET", `${s}/element/${item[ELEMENT]}/text`));
  if (labels.join("|") !== "More like this") {
    throw new Error(`the idea menu says ${JSON.stringify(labels)}`);
  }
  const rowButtons = await wd("POST", `${s}/elements`, {
    using: "xpath",
    value: `${written}//div[contains(@class,'idea-buttons')]/button`,
  });
  const buttonLabels = [];
  for (const b of rowButtons) buttonLabels.push(await wd("GET", `${s}/element/${b[ELEMENT]}/text`));
  if (buttonLabels.join("|") !== "Let's do it|Not interested") {
    throw new Error(`the idea buttons say ${JSON.stringify(buttonLabels)}`);
  }
  await shot(s, "pulse-ideas");
  await wd("POST", `${s}/element/${items[0][ELEMENT]}/click`, {});
  await waitFor("More like this kept", async () => {
    try {
      const state = JSON.parse(readFileSync(join(config, "pulse-ideas.json"), "utf8"));
      return state.moreLike?.[0] === PULSE_IDEA.title ? true : null;
    } catch {
      return null;
    }
  });
  console.log(`idea menu: ${labels.join(", ")}; More like this kept for the next run`);

  // The rows stay disabled until More like this has returned, and the kept
  // file is written before it does, so a click right away can land on a
  // disabled row and do nothing.
  const run = await waitFor("the idea row, enabled", async () => {
    const el = await wd("POST", `${s}/element`, {
      using: "xpath",
      value: "//li[contains(@aria-label,'smoke workboard')]//button[contains(@class,'idea-main')]",
    });
    return (await wd("GET", `${s}/element/${el[ELEMENT]}/enabled`)) ? el[ELEMENT] : null;
  });
  const calls = mockSeen.length;
  await wd("POST", `${s}/element/${run}/click`, {});
  const box = await waitFor("the chat box", () => find(s, "textarea[aria-label='Message']"));
  let seen;
  const filled = await waitFor("the filled chat box", async () => {
    seen = await wd("GET", `${s}/element/${box}/property/value`);
    return seen === PULSE_ACTION ? seen : null;
  }).catch((err) => {
    const held = seen === undefined ? "could not be read" : `held ${JSON.stringify(seen)}`;
    throw new Error(`${err.message}; the box ${held}`);
  });
  await sleep(1000);
  if (mockSeen.length !== calls) throw new Error("Let's do it sent the idea without the person");
  console.log(`let's do it filled the chat box: ${JSON.stringify(filled)}`);
}

// Journal: a question added here lands in the cabin's decisions.json, and To
// mission links it to a chat and puts the mission prompt in the chat box,
// unsent. The box is emptied after (Ctrl+A, Backspace, so React sees it) for
// the next step.
async function journal(s, config) {
  await tab(s, "Journal");
  const input = await waitFor("the question box", () =>
    find(s, ".journal-add input"),
  );
  await wd("POST", `${s}/element/${input}/value`, { text: JOURNAL_QUESTION });
  await click(s, ".journal-add button[type='submit']");
  const mission = await waitFor("the question's To mission", async () => {
    const el = await wd("POST", `${s}/element`, {
      using: "xpath",
      value: `//li[@aria-label='${JOURNAL_QUESTION}']//button[normalize-space()='To mission']`,
    });
    return el[ELEMENT];
  });
  const calls = mockSeen.length;
  await wd("POST", `${s}/element/${mission}/click`, {});
  const box = await waitFor("the chat box", () => find(s, "textarea[aria-label='Message']"));
  const filled = await waitFor("the mission prompt", async () => {
    const v = await wd("GET", `${s}/element/${box}/property/value`);
    return v.startsWith(`Mission from my decision journal: ${JOURNAL_QUESTION}`) ? v : null;
  });
  const saved = await waitFor("the investigating entry", async () => {
    const entries = JSON.parse(readFileSync(join(config, "decisions.json"), "utf8"));
    const entry = entries.find((e) => e.question === JOURNAL_QUESTION);
    return entry?.status === "investigating" && entry.missionThread ? entry : null;
  });
  await sleep(1000);
  if (mockSeen.length !== calls) throw new Error("To mission sent the prompt without the person");
  await shot(s, "journal-to-mission");
  console.log(`journal mission in ${saved.missionThread}: ${JSON.stringify(filled.split("\n")[0])}`);
  await wd("POST", `${s}/element/${box}/value`, { text: "\uE009a\uE000\uE003" });
  await waitFor("the emptied chat box", async () =>
    (await wd("GET", `${s}/element/${box}/property/value`)) === "" ? true : null,
  );
}

// Command palette: Ctrl+K opens it over any page, typing filters it, and
// Enter opens the pick; the Command button opens it too, for touch.
async function palette(s) {
  await wd("POST", `${s}/actions`, {
    actions: [
      {
        type: "key",
        id: "keyboard",
        actions: [
          { type: "keyDown", value: "\uE009" },
          { type: "keyDown", value: "k" },
          { type: "keyUp", value: "k" },
          { type: "keyUp", value: "\uE009" },
        ],
      },
    ],
  });
  const search = await waitFor("the palette", () =>
    find(s, "[aria-label='Command palette'] input"),
  );
  await shot(s, "palette");
  await wd("POST", `${s}/element/${search}/value`, { text: "web access\uE007" });
  await waitFor("Settings at Web access", () => find(s, "section.settings[aria-label='Web access']"));
  console.log("Ctrl+K opened Settings: Web access");

  await click(s, "button.palette-open");
  const again = await waitFor("the palette again", () =>
    find(s, "[aria-label='Command palette'] input"),
  );
  await wd("POST", `${s}/element/${again}/value`, { text: "journ\uE007" });
  await onView(s, "Journal");
  await waitFor("the palette to close", async () => {
    const open = await wd("POST", `${s}/elements`, {
      using: "css selector",
      value: "[aria-label='Command palette']",
    });
    return open.length === 0;
  });
  console.log("the Command button opened the Journal");
}

// Plan chip: the turn is read-only, so a write is refused without a card;
// the plan waits on its card until Approve plan, and the chip goes back to Chat.
// Settings → Personality: Use makes a preset the default in shell.json, and
// the next chat message's system prompt carries its voice after the rules.
async function usePersonality(s, config, name) {
  await tab(s, "Settings");
  await section(s, "Personality");
  // The cards' pictures arrive after the list and push the buttons down, so
  // a click can land where Use was a moment ago. Click Use once it sits
  // still, and again while it still shows.
  const useButton = `//ul[@aria-label='Personalities']/li[@aria-label='${name}']//button[normalize-space()='Use']`;
  await waitFor(`${name}'s Use button`, () =>
    wd("POST", `${s}/element`, { using: "xpath", value: useButton }),
  );
  await waitFor(`${name} in use`, async () => {
    const t = await textOf(s, `ul[aria-label='Personalities'] li[aria-label='${name}'] .card-status`);
    if (t === "In use") return t;
    const [use] = await wd("POST", `${s}/elements`, { using: "xpath", value: useButton });
    if (use && (await settled(s, use[ELEMENT]))) {
      await wd("POST", `${s}/element/${use[ELEMENT]}/click`, {});
      await sleep(500);
    }
    return null;
  });
  const saved = JSON.parse(readFileSync(join(config, "shell.json"), "utf8"));
  const id = name === "Nyx" ? "" : name.toLowerCase();
  if (saved.personality !== id) throw new Error(`shell.json holds personality ${JSON.stringify(saved.personality)}`);
  await shot(s, `settings-personality-${name.toLowerCase()}`);
  console.log(`personality: ${name} in use`);
}

async function plan(s, workspace) {
  await tab(s, "Chat");
  await click(s, "[aria-label='Session mode'] button[title^='Loadout writes a plan']");
  const box = await waitFor("the message box", () => find(s, "textarea[aria-label='Message']"));
  await wd("POST", `${s}/element/${box}/value`, { text: PLAN_PROMPT });
  await click(s, "form.composer button[type='submit']");
  const card = await waitFor("the plan card", () => find(s, ".card.elicit[aria-label=\"The plan\"]"));
  const shown = await cardText(s, card, (t) => t.includes(PLAN_TEXT));
  if (!shown.includes(PLAN_TEXT)) throw new Error(`plan card said ${JSON.stringify(shown)}`);
  if (existsSync(join(workspace, "plan-note.txt"))) throw new Error("plan mode wrote a file");
  const voiced = mockSeen.find((c) => c.body.includes(PLAN_PROMPT));
  if (!voiced?.body.includes("Personality: Pirate.")) throw new Error("the plan turn's prompt had no Pirate voice");
  console.log("the chat prompt carries the Pirate voice");
  const refused = mockSeen.find((c) => c.body.includes("smoke-plan-write") && c.body.includes("function_call_output"));
  if (!refused || !refused.body.includes("read-only")) throw new Error("the write was not refused as read-only");
  await shot(s, "plan-card");
  console.log("plan mode refused the write and showed the plan");
  const approve = await wd("POST", `${s}/element/${card}/element`, {
    using: "xpath",
    value: ".//button[normalize-space()='Approve plan']",
  });
  await wd("POST", `${s}/element/${approve[ELEMENT]}/click`, {});
  await waitFor("the reply after the plan", async () => {
    const els = await wd("POST", `${s}/elements`, {
      using: "css selector",
      value: ".bubble.assistant:not(.streaming)",
    });
    for (const el of els) {
      if ((await wd("GET", `${s}/element/${el[ELEMENT]}/text`)) === PLAN_REPLY) return true;
    }
    return null;
  });
  const chat = await find(s, "[aria-label='Session mode'] button[title^='Normal chat']");
  const pressed = await wd("GET", `${s}/element/${chat}/attribute/aria-pressed`);
  if (pressed !== "true") throw new Error("the chip stayed on Plan after the approval");
  console.log(`plan approved: ${JSON.stringify(PLAN_REPLY)}, chip back on Chat`);
}

// Settings → Local AI: on Windows smoke, capture Get engine → progress →
// ready → Gemma 4 12B draft offer (PR proof). Then Manual takes a model file whose
// name says its safety training was removed only after the notice is read,
// and Reset to Auto puts local_ai.json back.
async function localAi(s, config) {
  const settingsTab = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: "//nav[contains(@class,'tabs')]/button[normalize-space()='Settings']",
  });
  await wd("POST", `${s}/element/${settingsTab[ELEMENT]}/click`, {});
  await section(s, "Local AI");
  const status = await waitFor("the local AI status line", async () => {
    const t = await textOf(s, ".local-ai .local-status");
    return /^T\d/.test(t) ? t : null;
  });
  console.log(`local AI status: ${JSON.stringify(status)}`);

  if (windowsEngineProof()) {
    await setWindowSize(s, 1280, 900);
    await scrollIntoView(s, ".local-ai .local-engine");
    const engineHint = await waitFor("the local engine hint", async () => {
      const t = await textOf(s, ".local-ai .local-engine .note");
      return t.includes("llama") || t.includes("Runtime missing") ? t : null;
    });
    console.log(`local engine: ${JSON.stringify(engineHint)}`);

    // 1) Get engine (before install), when the pinned Windows build is missing.
    const getEngine = await wd("POST", `${s}/elements`, {
      using: "css selector",
      value: ".local-ai button.get-engine",
    });
    if (getEngine.length > 0) {
      await scrollIntoView(s, ".local-ai button.get-engine");
      await shot(s, "settings-local-ai-get-engine");
      console.log("shot Get engine before install");

      await wd("POST", `${s}/element/${getEngine[0][ELEMENT]}/click`, {});
      // 2) Progress / verifying while the pinned zip downloads.
      const progress = await waitFor(
        "engine download progress",
        async () => {
          try {
            const t = await textOf(s, ".local-ai .local-engine-progress");
            return /Downloading engine|checking/i.test(t) ? t : null;
          } catch {
            return null;
          }
        },
        60000,
      );
      await scrollIntoView(s, ".local-ai .local-engine");
      await shot(s, "settings-local-ai-engine-progress");
      console.log(`shot engine progress: ${JSON.stringify(progress)}`);

      // 3) Installed + ready after the checksum lands the binary. If the live
      // download stalls or errors, cancel, plant a stub exe, and re-open the
      // panel so the ready UI still ships as Windows proof.
      let ready = null;
      const readyDeadline = Date.now() + 120000;
      while (Date.now() < readyDeadline && !ready) {
        try {
          const errEls = await wd("POST", `${s}/elements`, {
            using: "css selector",
            value: ".local-ai .local-engine-error",
          });
          if (errEls.length > 0) {
            const err = await wd("GET", `${s}/element/${errEls[0][ELEMENT]}/text`);
            if (err) throw new Error(err);
          }
          const t = await textOf(s, ".local-ai .local-engine .note");
          if (/llama-server is ready/i.test(t)) ready = t;
        } catch (err) {
          console.log(`live engine install did not finish: ${err.message}`);
          break;
        }
        if (!ready) await sleep(250);
      }
      if (!ready) {
        const cancel = await wd("POST", `${s}/elements`, {
          using: "xpath",
          value: "//div[contains(@class,'local-engine')]//button[normalize-space()='Cancel']",
        });
        if (cancel.length > 0) {
          await wd("POST", `${s}/element/${cancel[0][ELEMENT]}/click`, {});
          await sleep(500);
        }
        const bin = join(config, "bin");
        mkdirSync(bin, { recursive: true });
        writeFileSync(join(bin, "llama-server.exe"), "MZ smoke stub\n");
        await section(s, "Appearance");
        await section(s, "Local AI");
        ready = await waitFor("llama-server ready after stub", async () => {
          const t = await textOf(s, ".local-ai .local-engine .note");
          return /llama-server is ready/i.test(t) ? t : null;
        });
      }
      await scrollIntoView(s, ".local-ai .local-engine");
      await shot(s, "settings-local-ai-engine-ready");
      console.log(`shot engine ready: ${JSON.stringify(ready)}`);
    } else {
      // Binary already on PATH / GROKHUB_LLAMA_SERVER — still prove ready.
      await shot(s, "settings-local-ai-engine-ready");
      console.log("Get engine absent; shot ready state only");
    }

    // 4) Gemma 4 12B → Install and run draft model offer (E2B not on disk).
    await click(s, ".local-ai button.mode-manual");
    const showAll = await wd("POST", `${s}/elements`, {
      using: "css selector",
      value: ".local-ai button.link",
    });
    for (const el of showAll) {
      const label = await wd("GET", `${s}/element/${el[ELEMENT]}/text`);
      if (/Show all/i.test(label)) {
        await wd("POST", `${s}/element/${el[ELEMENT]}/click`, {});
        break;
      }
    }
    const gemma = await waitFor("Gemma 4 12B in the picker", async () => {
      const el = await wd("POST", `${s}/element`, {
        using: "xpath",
        value: "//li[contains(@class,'local-model')][@aria-label='Gemma 4 12B']//input",
      });
      return el[ELEMENT];
    });
    await wd("POST", `${s}/element/${gemma}/click`, {});
    const offer = await waitFor("the Gemma 4 12B draft offer", async () => {
      const t = await textOf(s, ".local-ai .local-draft-offer");
      return /Install and run draft model/i.test(t) && /Gemma 4 E2B/i.test(t) ? t : null;
    });
    await scrollIntoView(s, ".local-ai .local-draft-offer");
    await shot(s, "settings-local-ai-gemma-draft-offer");
    console.log(`shot Gemma draft offer: ${JSON.stringify(offer.split("\n")[0])}`);

    await click(s, ".local-ai button.reset-local");
    await waitFor("the reset note after the draft offer", async () => {
      const t = await textOf(s, ".local-ai .local-note");
      return t === "Back to Auto." ? t : null;
    });
  }

  const modelFile = join(config, "Llama-3-8B-abliterated.Q4_K_M.gguf");
  writeFileSync(modelFile, "GGUF");
  await click(s, ".local-ai button.mode-manual");
  const input = await waitFor("the model file field", () =>
    find(s, "input[aria-label='Model file path']"),
  );
  await wd("POST", `${s}/element/${input}/value`, { text: modelFile });
  await click(s, ".local-ai button.use-file");
  const notice = await waitFor("the model notice", async () => {
    const t = await textOf(s, ".local-ai .notice");
    return t.includes("safety training removed") ? t : null;
  });
  console.log(`notice shown: ${JSON.stringify(notice.split("\n")[0])}`);
  await click(s, ".local-ai .notice button.accept");
  await click(s, ".local-ai button.save-local");
  await waitFor("the saved note", async () => {
    const t = await textOf(s, ".local-ai .local-note");
    return t === "Local AI saved." ? t : null;
  });
  const saved = JSON.parse(readFileSync(join(config, "local_ai.json"), "utf8"));
  if (
    saved.mode !== "manual" ||
    saved.localModel?.selected !== modelFile ||
    !saved.noticeSeen?.includes(modelFile)
  ) {
    throw new Error(`local_ai.json holds ${JSON.stringify(saved)}`);
  }
  const rules = JSON.parse(readFileSync(join(config, "local", "governor.json"), "utf8"));
  if (rules.pauseForGames !== true || rules.useIdleTime !== true) {
    throw new Error(`governor.json holds ${JSON.stringify(rules)}`);
  }
  await shot(s, "settings-local-ai");
  console.log("model file picked after the notice");

  await click(s, ".local-ai button.reset-local");
  await waitFor("the reset note", async () => {
    const t = await textOf(s, ".local-ai .local-note");
    return t === "Back to Auto." ? t : null;
  });
  const reset = JSON.parse(readFileSync(join(config, "local_ai.json"), "utf8"));
  if (reset.mode !== "auto" || reset.localModel?.selected !== "") {
    throw new Error(`Reset to Auto left ${JSON.stringify(reset)}`);
  }
  console.log("reset to Auto");
}

// Labs: a folder bundle installs off and untrusted into the config folder,
// then Trust and Turn on land in the same plugin-state.json the cabin reads.
async function labs(s, config) {
  const bundle = join(config, "smoke-bundle");
  mkdirSync(bundle, { recursive: true });
  writeFileSync(
    join(bundle, "plugin.json"),
    JSON.stringify({ name: "smoke-kit", version: "0.1.0", description: "Smoke bundle" }),
  );
  await tab(s, "Settings");
  await section(s, "Labs");
  const input = await waitFor("the install box", () => find(s, ".labs-row input"));
  await wd("POST", `${s}/element/${input}/value`, { text: bundle });
  await click(s, ".labs-row button[type='submit']");
  const step = async (label, state) => {
    const button = await waitFor(`the ${label} button`, async () => {
      const el = await wd("POST", `${s}/element`, {
        using: "xpath",
        value: `//li[@aria-label='smoke-kit']//button[normalize-space()='${label}']`,
      });
      return el[ELEMENT];
    });
    await wd("POST", `${s}/element/${button}/click`, {});
    await waitFor(`smoke-kit ${state}`, async () => {
      const t = await textOf(s, "li[aria-label='smoke-kit'] .card-status");
      return t === state ? t : null;
    });
  };
  await waitFor("the installed bundle", async () => {
    const t = await textOf(s, "li[aria-label='smoke-kit'] .card-status");
    return t === "Not trusted · off" ? t : null;
  });
  await step("Trust", "Trusted · off");
  await step("Turn on", "Trusted · on");
  const state = JSON.parse(readFileSync(join(config, "plugin-state.json"), "utf8"));
  const entry = state.entries?.["smoke-kit"];
  if (!entry?.enabled || !entry.hash) {
    throw new Error(`plugin-state.json holds ${JSON.stringify(state)}`);
  }
  await shot(s, "settings-labs");
  console.log("labs: bundle installed off, trusted, then turned on");
}

// Settings → Playbooks: Duplicate writes the copy to playbooks/ and opens it,
// Save keeps an edit, and Remove deletes the file again.
async function playbooks(s, config) {
  await tab(s, "Settings");
  await section(s, "Playbooks");
  // Prefer an enabled button: Save writes the file before React clears
  // `busy`, and a disabled Remove click is a no-op on Windows EdgeDriver.
  const button = async (card, label) => {
    const el = await waitFor(`${card}'s ${label}`, async () => {
      const found = await wd("POST", `${s}/element`, {
        using: "xpath",
        value: `//ul[@aria-label='All playbooks']/li[@aria-label='${card}']//button[normalize-space()='${label}' and not(@disabled)]`,
      });
      return found[ELEMENT];
    });
    await wd("POST", `${s}/element/${el}/click`, {});
  };
  await button("Files", "Duplicate");
  const file = join(config, "playbooks", "files-copy.toml");
  const box = await waitFor("the playbook editor", () => find(s, "textarea[aria-label='Playbook TOML']"));
  await shot(s, "settings-playbooks-editor");
  await wd("POST", `${s}/element/${box}/value`, { text: "\n# edited in the smoke test\n" });
  await click(s, "form.playbook-editor button[type='submit']");
  await waitFor("the saved copy", async () =>
    existsSync(file) && readFileSync(file, "utf8").includes("# edited in the smoke test") ? true : null,
  );
  console.log("playbook duplicated and saved to playbooks/files-copy.toml");
  await button("Files (copy)", "Remove");
  await waitFor("the removed copy", async () => (existsSync(file) ? null : true));
  console.log("playbook copy removed");
}

// Settings → Pulse: a topic added here and pinned lands in the cabin's
// interests.json, the file every Pulse lookup reads.
async function interests(s, config) {
  await tab(s, "Settings");
  await section(s, "Pulse");
  const input = await waitFor("the topic box", () => find(s, ".interests input"));
  await wd("POST", `${s}/element/${input}/value`, { text: "air compressors" });
  await click(s, ".interests button[type='submit']");
  const pin = await waitFor("the topic's Pin", async () => {
    const el = await wd("POST", `${s}/element`, {
      using: "xpath",
      value: "//li[@aria-label='air compressors']//button[normalize-space()='Pin']",
    });
    return el[ELEMENT];
  });
  await wd("POST", `${s}/element/${pin}/click`, {});
  const saved = await waitFor("the pinned topic", async () => {
    if (!existsSync(join(config, "interests.json"))) return null;
    const profile = JSON.parse(readFileSync(join(config, "interests.json"), "utf8"));
    const t = profile.topics?.find((x) => x.topic === "air compressors");
    return t?.pinned && t.source === "explicit" ? t : null;
  });
  await shot(s, "settings-pulse");
  console.log(`interest saved: ${JSON.stringify(saved.topic)}, pinned`);
}

// Settings → Permissions: a deny rule added here lands in the
// permission-rules.json the agent's gate reads, and Remove takes it out.
async function permissions(s, config) {
  await tab(s, "Settings");
  await section(s, "Permissions");
  const rulesFile = join(config, "permission-rules.json");
  const input = await waitFor("the rule box", () => find(s, ".permissions input[placeholder^='Bash(git']"));
  // The rule box, its chips and Add rule sit together, with no gap between them.
  const box = await wd("GET", `${s}/element/${input}/rect`);
  const add = await wd("GET", `${s}/element/${await find(s, ".permissions form:first-of-type button[type='submit']")}/rect`);
  const gap = add.y - (box.y + box.height);
  if (gap > 60) throw new Error(`Add rule sits ${Math.round(gap)}px below the rule box`);
  await click(s, "[aria-label='Rule action'] button:nth-child(3)");
  await wd("POST", `${s}/element/${input}/value`, { text: "WebFetch(domain:example.com)" });
  await click(s, ".permissions form:first-of-type button[type='submit']");
  await waitFor("the saved deny rule", async () => {
    if (!existsSync(rulesFile)) return null;
    const raw = readFileSync(rulesFile, "utf8");
    return raw.includes("WebFetch(domain:example.com)") && raw.includes("deny") ? true : null;
  });
  await shot(s, "settings-permissions");
  console.log("permissions: deny rule saved to permission-rules.json");
  const remove = await waitFor("the rule's Remove", async () => {
    const el = await wd("POST", `${s}/element`, {
      using: "xpath",
      value: "//ul[@aria-label='Saved rules']/li[@aria-label='Deny WebFetch(domain:example.com)']//button[normalize-space()='Remove']",
    });
    return el[ELEMENT];
  });
  await wd("POST", `${s}/element/${remove}/click`, {});
  await waitFor("the removed rule", async () =>
    readFileSync(rulesFile, "utf8").includes("WebFetch(domain:example.com)") ? null : true,
  );
  console.log("permissions: rule removed");
}

// Settings → Appearance: Ops Dark pins Loadout colors on the page right away
// and saves in shell.json; System hands them back to the OS.
async function appearance(s, config) {
  await tab(s, "Settings");
  await section(s, "Appearance");
  const pick = async (label) => {
    const el = await waitFor(`the ${label} theme`, async () => {
      const found = await wd("POST", `${s}/element`, {
        using: "xpath",
        value: `//div[@aria-label='Theme']/button[normalize-space()='${label}']`,
      });
      return found[ELEMENT];
    });
    await wd("POST", `${s}/element/${el}/click`, {});
  };
  const look = () =>
    wd("POST", `${s}/execute/sync`, {
      script:
        "return [document.documentElement.dataset.theme || '', getComputedStyle(document.body).backgroundColor];",
      args: [],
    });
  await pick("Ops Dark");
  const [theme, bg] = await waitFor("the ops-dark page", async () => {
    const seen = await look();
    return seen[0] === "ops-dark" ? seen : null;
  });
  if (bg !== "rgb(11, 11, 13)") throw new Error(`ops-dark theme painted ${bg}`);
  const saved = await waitFor("the saved theme", async () => {
    const t = JSON.parse(readFileSync(join(config, "shell.json"), "utf8")).theme;
    return t === "ops-dark" ? t : null;
  });
  await shot(s, "settings-appearance-ops-dark");
  console.log(`appearance: ${theme} on the page (${bg}), ${saved} in shell.json`);
  await pick("System");
  await waitFor("the system theme", async () => ((await look())[0] === "" ? true : null));
  await pick("Ops Dark");
  await waitFor("ops-dark after system", async () =>
    (await look())[0] === "ops-dark" ? true : null,
  );
}

async function main() {
  const mock = await startMock();
  const config = mkdtempSync(join(tmpdir(), "grokhub-shell-smoke-"));
  const workspace = join(config, "work");
  mkdirSync(workspace);
  writeFileSync(join(config, "shell.json"), JSON.stringify({ workspace, theme: "ops-dark" }));
  const modelCalls = () => mock.seen.filter((c) => c.path === "/v1/responses");
  const native = process.env.SMOKE_NATIVE_DRIVER;
  const driver = spawn("tauri-driver", native ? ["--native-driver", native] : [], {
    stdio: ["ignore", "inherit", "inherit"],
    env: {
      ...process.env,
      GROKHUB_CONFIG: config,
      GROKHUB_SHELL_RESPONSES_URL: `http://127.0.0.1:${mock.port}/v1/responses`,
      GROKHUB_SHELL_PULSE_NET: `http://127.0.0.1:${mock.port}`,
    },
  });
  let session;
  try {
    await waitFor("tauri-driver", async () => (await fetch(`${DRIVER}/status`)).ok);
    const created = await wd("POST", "/session", {
      capabilities: {
        alwaysMatch: { "tauri:options": { application: resolve(binary) } },
      },
    });
    session = created.sessionId;
    const s = `/session/${session}`;
    if (windowsEngineProof()) await setWindowSize(s, 1280, 900);

    const box = await waitFor("the message box", async () =>
      wd("POST", `${s}/element`, { using: "css selector", value: "textarea[aria-label='Message']" }),
    );
    await wd("POST", `${s}/element/${box[ELEMENT]}/value`, { text: PROMPT });
    const send = await wd("POST", `${s}/element`, {
      using: "css selector",
      value: "form.composer button[type='submit']",
    });
    await wd("POST", `${s}/element/${send[ELEMENT]}/click`, {});

    const card = await waitFor("the permission card", async () => {
      const el = await wd("POST", `${s}/element`, {
        using: "css selector",
        value: ".card.permission",
      });
      return el[ELEMENT];
    });
    const asked = await cardText(s, card, (t) => t.includes("write") && t.includes(NOTE));
    if (!asked.includes("write") || !asked.includes(NOTE)) {
      throw new Error(`permission card said ${JSON.stringify(asked)}`);
    }
    await shot(s, "chat-permission-card");
    console.log(`permission card shown: ${JSON.stringify(asked.split("\n")[0])}`);

    // Without an answer the write must not run and the turn must not go on.
    await sleep(1500);
    const notePath = join(workspace, NOTE);
    if (existsSync(notePath)) throw new Error("the note was written before Allow once");
    if (modelCalls().length !== 1) {
      throw new Error(`the turn went on without an answer: ${modelCalls().length} model calls`);
    }
    console.log("nothing ran while the card waited");

    const allow = await wd("POST", `${s}/element/${card}/element`, {
      using: "css selector",
      value: ".card-buttons button",
    });
    await wd("POST", `${s}/element/${allow[ELEMENT]}/click`, {});

    const text = await waitFor("the mocked reply", async () => {
      const el = await wd("POST", `${s}/element`, {
        using: "css selector",
        value: ".bubble.assistant:not(.streaming)",
      });
      const t = await wd("GET", `${s}/element/${el[ELEMENT]}/text`);
      return t === REPLY ? t : null;
    });
    await shot(s, "chat-reply");
    console.log(`reply rendered: ${JSON.stringify(text)}`);
    if (process.env.SMOKE_SCREENSHOT) {
      await sleep(1000);
      const png = await wd("GET", `${s}/screenshot`);
      writeFileSync(process.env.SMOKE_SCREENSHOT, Buffer.from(png, "base64"));
    }

    const written = readFileSync(notePath, "utf8");
    if (written !== NOTE_TEXT) throw new Error(`the note says ${JSON.stringify(written)}`);
    console.log(`note written after Allow once: ${JSON.stringify(written)}`);

    const calls = modelCalls();
    if (calls.length !== 4) throw new Error(`expected 4 model calls, saw ${calls.length}`);
    const first = JSON.parse(calls[0].body);
    const prompted = JSON.stringify(first.input).includes(PROMPT);
    if (!prompted) throw new Error("the model never got the prompt");
    const tools = (first.tools ?? []).map((t) => t.name);
    if (!tools.includes("write")) throw new Error(`the model was not offered write: ${tools}`);
    const second = calls[1].body;
    if (!second.includes("function_call_output") || !second.includes("smoke-write")) {
      throw new Error("the write's result never reached the model");
    }
    if (calls.some((c) => c.auth !== "Bearer loopback-test")) {
      throw new Error(`auth was ${calls.map((c) => c.auth)}`);
    }
    console.log(`model got the prompt with model=${first.model} and ${tools.length} tools`);

    await automations(s, config);
    await memory(s, workspace);
    await connectors(s, config);
    await pulse(s, config);
    await journal(s, config);
    await palette(s);
    await usePersonality(s, config, "Pirate");
    await plan(s, workspace);
    await usePersonality(s, config, "Nyx");
    await localAi(s, config);
    await labs(s, config);
    await playbooks(s, config);
    await interests(s, config);
    await permissions(s, config);
    await appearance(s, config);
    await desktop(s, config);
    if (process.env.SMOKE_WEB === "1") await web(s);
    console.log("smoke ok");
  } catch (err) {
    // What the window and the mock saw, for a failed CI run.
    if (session) {
      const log = await wd("POST", `/session/${session}/element`, {
        using: "css selector",
        value: ".chat",
      })
        .then((el) => wd("GET", `/session/${session}/element/${el[ELEMENT]}/text`))
        .catch((e) => `(no chat text: ${e.message})`);
      console.error(`chat shows: ${JSON.stringify(log)}`);
    }
    console.error(`model calls: ${mock.seen.length}`);
    throw err;
  } finally {
    if (session) await wd("DELETE", `/session/${session}`).catch(() => {});
    driver.kill();
    mock.server.close();
    rmSync(config, { recursive: true, force: true });
  }
}

// A tool counts as offered when the model gets its schema or, with deferred
// tools on, its line in load_tool_schema's index ("name — blurb").
function offeredTools(body) {
  const tools = JSON.parse(body).tools ?? [];
  const index = tools.find((t) => t.name === "load_tool_schema")?.description ?? "";
  const deferred = (index.split("Deferred tools (index):\n")[1] ?? "")
    .split("\n")
    .map((line) => line.split(" — ")[0].trim())
    .filter(Boolean);
  return [...tools.map((t) => t.name), ...deferred];
}

// Computer-use: desktop tools are off until Settings turns them on. With the
// switch on and approvals at Always, the model types a delete into the
// desktop: the card must wait with nothing typed and no further model call,
// and Deny must reach the model as a refusal.
async function desktop(s, config) {
  const offered = (call) => offeredTools(call.body);
  const before = mockSeen.find((c) => c.path === "/v1/responses");
  if (offered(before).includes("type")) throw new Error("desktop tools were offered while off");

  await tab(s, "Settings");
  await section(s, "General");
  const desk = await waitFor("the desktop switch", () =>
    wd("POST", `${s}/element`, {
      using: "xpath",
      value: "//label[contains(., 'Let Loadout control the desktop')]//button[@role='switch']",
    }).then((el) => el[ELEMENT]),
  );
  await wd("POST", `${s}/element/${desk}/click`, {});
  await click(s, ".settings > form button[type='submit']");
  await waitFor("the saved desktop switch", () => {
    const saved = JSON.parse(readFileSync(join(config, "shell.json"), "utf8"));
    return saved.desktopControl === true;
  });
  // Approvals live in Settings → Defaults.
  await section(s, "Defaults");
  const approvals = await waitFor("the approvals pick", () => find(s, ".settings > form select"));
  const always = await wd("POST", `${s}/element/${approvals}/element`, {
    using: "css selector",
    value: "option[value='always']",
  });
  await wd("POST", `${s}/element/${always[ELEMENT]}/click`, {});
  await click(s, ".settings > form button[type='submit']");
  await waitFor("the saved switch", () => {
    const saved = JSON.parse(readFileSync(join(config, "shell.json"), "utf8"));
    return saved.desktopControl === true && saved.approvals === "always";
  });
  console.log("desktop control on, approvals at Always");

  await tab(s, "Chat");
  const box = await waitFor("the message box", () => find(s, "textarea[aria-label='Message']"));
  await wd("POST", `${s}/element/${box}/value`, { text: DESK_PROMPT });
  await click(s, "form.composer button[type='submit']");
  const card = await waitFor("the desktop permission card", async () => {
    const cards = await wd("POST", `${s}/elements`, {
      using: "css selector",
      value: ".card.permission",
    });
    for (const el of cards) {
      const t = await wd("GET", `${s}/element/${el[ELEMENT]}/text`);
      if (t.includes(DESK_TEXT)) return { el: el[ELEMENT], text: t };
    }
    return null;
  });
  await shot(s, "desktop-card");
  console.log(`desktop card shown: ${JSON.stringify(card.text.split("\n").slice(0, 2).join(" "))}`);
  const deskCalls = () => mockSeen.filter((c) => c.body.includes(DESK_PROMPT));
  if (!offered(deskCalls()[0]).includes("type")) throw new Error("desktop tools were not offered");
  await sleep(1500);
  if (deskCalls().length !== 1) {
    throw new Error(`the turn went on without an OK: ${deskCalls().length} model calls`);
  }
  console.log("nothing typed while the card waited, in Always mode");

  const deny = await wd("POST", `${s}/element/${card.el}/element`, {
    using: "xpath",
    value: ".//button[normalize-space()='Deny']",
  });
  await wd("POST", `${s}/element/${deny[ELEMENT]}/click`, {});
  await waitFor("the reply after Deny", () => deskCalls().length === 2);
  const refused = deskCalls()[1].body;
  if (!refused.includes("User rejected the execution for tool `type`")) {
    throw new Error("Deny never reached the model as a refusal");
  }
  console.log("Deny reached the model as a refusal");
}

// Web access: the window turns it on, and Chromium pairs from the localhost
// link and chats through this shell's engine. Desktop control and Always
// are still on from the step above, and the web chat must get no desktop
// tools. Then a refused command, and turning it off ends the pairing.
async function web(s) {
  const { chromium } = createRequire(import.meta.url)("playwright");
  await tab(s, "Settings");
  await section(s, "Web access");
  const on = await waitFor("the web access button", () =>
    wd("POST", `${s}/element`, {
      using: "xpath",
      value: "//button[normalize-space()='Turn on web access']",
    }).then((el) => el[ELEMENT]),
  );
  await wd("POST", `${s}/element/${on}/click`, {});
  const code = await waitFor("the pairing code", () => textOf(s, "[aria-label='Pairing code']"));
  const links = await wd("POST", `${s}/elements`, { using: "css selector", value: ".web-access li code" });
  const texts = await Promise.all(links.map((el) => wd("GET", `${s}/element/${el[ELEMENT]}/text`)));
  const link = texts.find((t) => t.startsWith("http://127.0.0.1:"));
  if (!link || !link.endsWith(`#code=${code}`)) throw new Error(`no localhost pairing link: ${texts}`);
  console.log(`web access on: ${texts.length} link(s), a ${code.length}-character code`);
  await shot(s, "settings-web-access");
  const origin = new URL(link).origin;

  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.goto(link);
    const box = page.getByRole("textbox", { name: "Message" });
    await box.waitFor({ timeout: 30000 });
    await box.fill(WEB_PROMPT);
    await page.locator("form.composer button[type='submit']").click();
    await page
      .locator(".bubble.assistant:not(.streaming)")
      .filter({ hasText: WEB_REPLY })
      .waitFor({ timeout: 30000 });
    console.log(`the browser got the reply: ${JSON.stringify(WEB_REPLY)}`);

    const asked = mockSeen.find((c) => c.body.includes(WEB_PROMPT));
    const tools = offeredTools(asked.body);
    if (tools.includes("type") || tools.includes("click")) {
      throw new Error(`a web chat was offered desktop tools: ${tools}`);
    }
    if (asked.auth !== "Bearer loopback-test") throw new Error(`auth was ${asked.auth}`);
    console.log(`the web chat ran on this shell with ${tools.length} tools and no desktop tools`);

    const refused = await page.evaluate(async (pairing) => {
      const call = (auth) =>
        fetch("/remote/v1/call/set_api_key", {
          method: "POST",
          headers: { "Content-Type": "application/json", ...auth },
          body: JSON.stringify({ key: "sk-abcdefghijklmnopqrstuv" }),
        });
      const paired = await (await call({ Authorization: `Bearer ${pairing}` })).json();
      return { paired, unpaired: (await call({})).status };
    }, code);
    if (refused.paired.ok !== false || refused.unpaired !== 401) {
      throw new Error(`set_api_key from the web: ${JSON.stringify(refused)}`);
    }
    console.log(`the web can't set the key: ${JSON.stringify(refused.paired.error)}`);

    // Port 4620 is HTTPS with the certificate whose fingerprint Settings shows.
    const shown = await textOf(s, "[aria-label='Certificate fingerprint']");
    const served = await new Promise((done, fail) => {
      const sock = tlsConnect({ host: "127.0.0.1", port: 4620, rejectUnauthorized: false }, () => {
        done(sock.getPeerCertificate().fingerprint256);
        sock.end();
      });
      sock.on("error", fail);
    });
    if (served !== shown) throw new Error(`HTTPS certificate ${served}, Settings shows ${shown}`);
    // A browser that accepted the certificate pairs and loads the app over HTTPS.
    const secure = await browser.newContext({ ignoreHTTPSErrors: true });
    const tlsPage = await secure.newPage();
    await tlsPage.goto(`https://127.0.0.1:4620/#code=${code}`);
    await tlsPage.getByRole("textbox", { name: "Message" }).waitFor({ timeout: 30000 });
    console.log(`web access over HTTPS: paired, certificate ${shown.slice(0, 11)}… matches Settings`);
  } finally {
    await browser.close();
  }

  await click(s, ".web-access button");
  await waitFor("web access off", async () => {
    // A stopped server may still take the connection but never answers.
    const reachable = await fetch(`${origin}/remote/v1/hello`, {
      signal: AbortSignal.timeout(2000),
    }).then(
      () => true,
      () => false,
    );
    return !reachable;
  });
  console.log("web access off: the address no longer answers");
}

main().catch((err) => {
  console.error(`smoke failed: ${err.message}`);
  process.exit(1);
});
