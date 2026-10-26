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
// SMOKE_WEB=1 adds the web access step, which needs Playwright's Chromium.

import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
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
  await waitFor("the remembered note", async () => {
    const t = await textOf(s, ".memory p.note");
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
// Ideas. Not this on the post writes the ledger line to the global MEMORY.md
// and takes it off; Run on the idea puts its action in the chat box, unsent.
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
  await tab(s, "Pulse");
  await waitFor("the feed post", async () => {
    const t = await textOf(s, ".pulse .card.post .card-title");
    return t === "Smoke board summary" ? t : null;
  });
  const notThis = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: "//li[contains(@class,'post')]//button[normalize-space()='Not this']",
  });
  await wd("POST", `${s}/element/${notThis[ELEMENT]}/click`, {});
  await waitFor("the not-this status", async () => {
    const t = await textOf(s, ".pulse p.status");
    return t === "Got it. Less like this." ? t : null;
  });
  const ledger = readFileSync(join(config, "memory", "MEMORY.md"), "utf8");
  if (!ledger.includes('pulse: dismissed "Smoke board summary" reason=not-this')) {
    throw new Error(`MEMORY.md says ${JSON.stringify(ledger)}`);
  }
  console.log(`ledger line: ${JSON.stringify(ledger.trim())}`);

  const ideas = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: "//div[@aria-label='Pulse view']/button[normalize-space()='Ideas']",
  });
  await wd("POST", `${s}/element/${ideas[ELEMENT]}/click`, {});
  const run = await waitFor("the idea's Run", async () => {
    const el = await wd("POST", `${s}/element`, {
      using: "xpath",
      value: "//li[contains(@class,'post')]//button[normalize-space()='Run']",
    });
    return el[ELEMENT];
  });
  const calls = mockSeen.length;
  await wd("POST", `${s}/element/${run}/click`, {});
  const box = await waitFor("the chat box", () => find(s, "textarea[aria-label='Message']"));
  const filled = await waitFor("the filled chat box", async () => {
    const v = await wd("GET", `${s}/element/${box}/property/value`);
    return v === PULSE_ACTION ? v : null;
  });
  await sleep(1000);
  if (mockSeen.length !== calls) throw new Error("Run sent the idea without the person");
  console.log(`run filled the chat box: ${JSON.stringify(filled)}`);
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
  await wd("POST", `${s}/element/${search}/value`, { text: "web access\uE007" });
  await waitFor("Settings at Web access", () => find(s, "section.settings[aria-label='Web access']"));
  console.log("Ctrl+K opened Settings: Web access");

  await click(s, "button.palette-open");
  const again = await waitFor("the palette again", () =>
    find(s, "[aria-label='Command palette'] input"),
  );
  await wd("POST", `${s}/element/${again}/value`, { text: "journ\uE007" });
  await waitFor("the Journal page", () => find(s, "section.journal"));
  console.log("the Command button opened the Journal");
}

// Plan chip: the turn is read-only, so a write is refused without a card;
// the plan waits on its card until Approve plan, and the chip goes back to Chat.
// Settings → Personality: Use makes a preset the default in shell.json, and
// the next chat message's system prompt carries its voice after the rules.
async function usePersonality(s, config, name) {
  await tab(s, "Settings");
  await section(s, "Personality");
  const use = await waitFor(`${name}'s Use button`, async () => {
    const el = await wd("POST", `${s}/element`, {
      using: "xpath",
      value: `//ul[@aria-label='Personalities']/li[@aria-label='${name}']//button[normalize-space()='Use']`,
    });
    return el[ELEMENT];
  });
  await wd("POST", `${s}/element/${use}/click`, {});
  await waitFor(`${name} in use`, async () => {
    const t = await textOf(s, `ul[aria-label='Personalities'] li[aria-label='${name}'] .card-status`);
    return t === "In use" ? t : null;
  });
  const saved = JSON.parse(readFileSync(join(config, "shell.json"), "utf8"));
  const id = name === "Default" ? "" : name.toLowerCase();
  if (saved.personality !== id) throw new Error(`shell.json holds personality ${JSON.stringify(saved.personality)}`);
  console.log(`personality: ${name} in use`);
}

async function plan(s, workspace) {
  await tab(s, "Chat");
  await click(s, "[aria-label='Session mode'] button[title^='Grok writes a plan']");
  const box = await waitFor("the message box", () => find(s, "textarea[aria-label='Message']"));
  await wd("POST", `${s}/element/${box}/value`, { text: PLAN_PROMPT });
  await click(s, "form.composer button[type='submit']");
  const card = await waitFor("the plan card", () => find(s, ".card.elicit[aria-label=\"Grok's plan\"]"));
  const shown = await wd("GET", `${s}/element/${card}/text`);
  if (!shown.includes(PLAN_TEXT)) throw new Error(`plan card said ${JSON.stringify(shown)}`);
  if (existsSync(join(workspace, "plan-note.txt"))) throw new Error("plan mode wrote a file");
  const voiced = mockSeen.find((c) => c.body.includes(PLAN_PROMPT));
  if (!voiced?.body.includes("Personality: Pirate.")) throw new Error("the plan turn's prompt had no Pirate voice");
  console.log("the chat prompt carries the Pirate voice");
  const refused = mockSeen.find((c) => c.body.includes("smoke-plan-write") && c.body.includes("function_call_output"));
  if (!refused || !refused.body.includes("read-only")) throw new Error("the write was not refused as read-only");
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

// Settings → Local AI: the Auto status line renders, Manual takes a model
// file whose name says its safety training was removed only after the
// notice is read, and Reset to Auto puts local_ai.json back.
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
  console.log("labs: bundle installed off, trusted, then turned on");
}

// Settings → Playbooks: Duplicate writes the copy to playbooks/ and opens it,
// Save keeps an edit, and Remove deletes the file again.
async function playbooks(s, config) {
  await tab(s, "Settings");
  await section(s, "Playbooks");
  const button = async (card, label) => {
    const el = await waitFor(`${card}'s ${label}`, async () => {
      const found = await wd("POST", `${s}/element`, {
        using: "xpath",
        value: `//ul[@aria-label='All playbooks']/li[@aria-label='${card}']//button[normalize-space()='${label}']`,
      });
      return found[ELEMENT];
    });
    await wd("POST", `${s}/element/${el}/click`, {});
  };
  await button("Files", "Duplicate");
  const file = join(config, "playbooks", "files-copy.toml");
  const box = await waitFor("the playbook editor", () => find(s, "textarea[aria-label='Playbook TOML']"));
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

async function main() {
  const mock = await startMock();
  const config = mkdtempSync(join(tmpdir(), "grokhub-shell-smoke-"));
  const workspace = join(config, "work");
  mkdirSync(workspace);
  writeFileSync(join(config, "shell.json"), JSON.stringify({ workspace }));
  const modelCalls = () => mock.seen.filter((c) => c.path === "/v1/responses");
  const native = process.env.SMOKE_NATIVE_DRIVER;
  const driver = spawn("tauri-driver", native ? ["--native-driver", native] : [], {
    stdio: ["ignore", "inherit", "inherit"],
    env: {
      ...process.env,
      GROKHUB_CONFIG: config,
      GROKHUB_SHELL_RESPONSES_URL: `http://127.0.0.1:${mock.port}/v1/responses`,
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
    const asked = await wd("GET", `${s}/element/${card}/text`);
    if (!asked.includes("write") || !asked.includes(NOTE)) {
      throw new Error(`permission card said ${JSON.stringify(asked)}`);
    }
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
    await usePersonality(s, config, "Default");
    await localAi(s, config);
    await labs(s, config);
    await playbooks(s, config);
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

// Computer-use: desktop tools are off until Settings turns them on. With the
// switch on and approvals at Always, the model types a delete into the
// desktop: the card must wait with nothing typed and no further model call,
// and Deny must reach the model as a refusal.
async function desktop(s, config) {
  const offered = (call) => (JSON.parse(call.body).tools ?? []).map((t) => t.name);
  const before = mockSeen.find((c) => c.path === "/v1/responses");
  if (offered(before).includes("type")) throw new Error("desktop tools were offered while off");

  await tab(s, "Settings");
  await section(s, "General");
  const desk = await waitFor("the desktop switch", () =>
    wd("POST", `${s}/element`, {
      using: "xpath",
      value: "//label[contains(., 'Let Grok control the desktop')]/input",
    }).then((el) => el[ELEMENT]),
  );
  await wd("POST", `${s}/element/${desk}/click`, {});
  const approvals = await find(s, ".settings > form select");
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
    const tools = (JSON.parse(asked.body).tools ?? []).map((t) => t.name);
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
