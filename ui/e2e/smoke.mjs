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

import { spawn, spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
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
// The Code tab's agent window: one message that plans first and then, once
// its plan is approved, writes a note in its session's folder and ticks a
// to-do.
const AGENT_PROMPT = "smoke agent window: write the session note";
const AGENT_PLAN = "Write one note in the project.\n\nTasks\n1. write the session note";
const AGENT_NOTE = "agent-session-note.txt";
const AGENT_NOTE_TEXT = "written by the agent window's session";
const AGENT_TODO = "write the session note";
const AGENT_REPLY = "The session note is written";
// A second message in the same session, sent after the app has moved on.
const AGENT_PROMPT_2 = "smoke agent window: write the follow-up note";
const AGENT_NOTE_2 = "agent-session-follow-up.txt";
const AGENT_REPLY_2 = "The follow-up note is written";
// That message hands its task to a helper first. The helper reads one file
// and answers; the Tasks panel must follow it as it runs.
const AGENT_HELPER_PROMPT = "smoke helper: read the session note";
const AGENT_HELPER_TITLE = `1: ${AGENT_TODO}`;
const AGENT_HELPER_REPLY = "The helper read the session note";
// A third message hands two tasks to helpers that each work in a worktree of
// their own. Each writes one file there; nothing reaches the project until
// Merge is clicked in the Tasks panel.
const AGENT_PROMPT_3 = "smoke agent window: write two files in worktrees";
const AGENT_REPLY_3 = "Two helpers are writing in worktrees";
const WORK = [1, 2].map((n) => ({
  todo: { id: String(n + 1), content: `write work file ${n}`, status: "in_progress" },
  title: `${n + 1}: write work file ${n}`,
  // The session's first helper was s1.
  helper: `s${n + 1}`,
  prompt: `smoke worktree helper ${n}: write w${n}.txt`,
  spawn: `smoke-work-spawn-${n}`,
  call: `smoke-work-write-${n}`,
  file: `w${n}.txt`,
  text: `written by worktree helper ${n}\n`,
  reply: `Worktree helper ${n} wrote w${n}.txt`,
}));
// What the project holds as w2.txt before the second Merge, which must then
// be a conflict.
const WORK_MINE = "written in the project by hand\n";
// Review in the Tasks panel sends this to the lead (REVIEW_ASK in
// Agents.tsx). The lead starts a review helper, which is given the change
// and reports one finding that must be fixed.
const REVIEW_ASK = "Review the change: start one helper with type review, then tell me what it found.";
const REVIEW_PROMPT = "smoke review helper: check the two work files";
const REVIEW_FINDING = `${WORK[0].file}:1 the smoke blocker`;
const REVIEW_HELPER_REPLY = "The review helper found one blocker";
const REVIEW_REPLY = "The review found one blocker";
// The Tasks panel of a lead session reads when the app says its tasks
// changed, and otherwise only every 15 seconds. A change the mock has seen
// must show within this long, three times in a row: a timer cannot do that.
const LIVE_MS = 1000;

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
  // The helper of the agent window's second message. Checked first: its
  // request starts from the lead's conversation, so it carries that prompt.
  const stage = helperStage(body);
  if (stage === "start") return toolCall("smoke-helper-read", "read_file", { target_file: AGENT_NOTE });
  if (stage === "read") return textReply(AGENT_HELPER_REPLY);
  // The review helper starts fresh: its requests carry its task and not the
  // lead's spawn call. Its task holds the diff, so it is checked before
  // anything a file in the change could match.
  if (body.includes(REVIEW_PROMPT) && !body.includes("smoke-review-spawn")) {
    if (!body.includes("smoke-review-report")) {
      return toolCall("smoke-review-report", "report_findings", {
        findings: [{ severity: "high", text: REVIEW_FINDING }],
      });
    }
    return textReply(REVIEW_HELPER_REPLY);
  }
  // The lead's turn after Review, which carries every earlier turn.
  if (body.includes(REVIEW_ASK)) {
    if (!body.includes("smoke-review-spawn")) {
      return toolCall("smoke-review-spawn", "spawn_subagent", {
        type: "review",
        description: "review the change",
        prompt: REVIEW_PROMPT,
      });
    }
    return textReply(REVIEW_REPLY);
  }
  // A worktree helper starts fresh, so its requests carry its own prompt and
  // not the lead's message.
  const worker = WORK.find((w) => body.includes(w.prompt) && !body.includes(AGENT_PROMPT_3));
  if (worker) {
    if (!body.includes(worker.call)) {
      return toolCall(worker.call, "write", { path: worker.file, content: worker.text });
    }
    return textReply(worker.reply);
  }
  // Before the earlier agent prompts, which this turn carries as history.
  if (body.includes(AGENT_PROMPT_3)) {
    if (!body.includes("smoke-work-todo")) {
      return toolCall("smoke-work-todo", "todo_write", {
        todos: [{ id: "1", content: AGENT_TODO, status: "completed" }, ...WORK.map((w) => w.todo)],
      });
    }
    // One tool call per answer: the two helpers start in two steps.
    const next = WORK.find((w) => !body.includes(w.spawn));
    if (next) {
      return toolCall(next.spawn, "spawn_subagent", {
        type: "general",
        description: next.title,
        prompt: [
          `Goal: ${next.prompt}`,
          "Constraints: write only that file",
          "Edge cases: none",
          `Done when: ${next.file} is written`,
          `Files: ${next.file}`,
        ].join("\n"),
        isolation: "worktree",
        background: true,
      });
    }
    return textReply(AGENT_REPLY_3);
  }
  // Before the first agent prompt, which this turn carries as history.
  if (body.includes(AGENT_PROMPT_2)) {
    if (!body.includes("smoke-agent-todo-run")) {
      return toolCall("smoke-agent-todo-run", "todo_write", {
        todos: [{ id: "1", content: AGENT_TODO, status: "in_progress" }],
      });
    }
    if (!body.includes("smoke-agent-spawn")) {
      return toolCall("smoke-agent-spawn", "spawn_subagent", {
        type: "general",
        description: AGENT_HELPER_TITLE,
        prompt: [
          `Goal: ${AGENT_HELPER_PROMPT}`,
          "Constraints: read only",
          "Edge cases: none",
          "Done when: the note is read",
          `Files: ${AGENT_NOTE}`,
        ].join("\n"),
      });
    }
    if (!body.includes("smoke-agent-todo-done")) {
      return toolCall("smoke-agent-todo-done", "todo_write", {
        todos: [{ id: "1", content: AGENT_TODO, status: "completed" }],
      });
    }
    // This request carries the to-do's result, so the task is closed by now.
    taskSeen.done ??= Date.now();
    if (!body.includes("smoke-agent-follow-up")) {
      return toolCall("smoke-agent-follow-up", "write", { path: AGENT_NOTE_2, content: AGENT_NOTE_TEXT });
    }
    return textReply(AGENT_REPLY_2);
  }
  if (body.includes(AGENT_PROMPT)) {
    // A session started in the agent window plans first.
    if (!body.includes("smoke-agent-plan")) {
      return toolCall("smoke-agent-plan", "exit_plan_mode", { plan: AGENT_PLAN });
    }
    if (!body.includes("smoke-agent-write")) {
      return toolCall("smoke-agent-write", "write", { path: AGENT_NOTE, content: AGENT_NOTE_TEXT });
    }
    if (!body.includes("smoke-agent-todo")) {
      return toolCall("smoke-agent-todo", "todo_write", {
        todos: [{ id: "1", content: AGENT_TODO, status: "completed" }],
      });
    }
    return textReply(AGENT_REPLY);
  }
  // A supervised desktop session: its first step types a delete (the card
  // waits), the next says it is waiting. Checked before the desktop prompt,
  // whose turn the chat carries as history.
  if (body.includes(EP_PROMPT)) {
    const asks = mockSeen.filter((c) => c.path === "/v1/responses" && c.body.includes(EP_PROMPT)).length;
    if (asks <= 1) return toolCall("smoke-ep", "type", { text: EP_TEXT });
    return textReply(EP_REPLY);
  }
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

/** While set, the mock holds its answer to the agent window's first call, so
 * the test can act while that reply is still running. */
let agentHold = null;

/** Which of the helper's two model calls this is: "start" before it has read
 * the note and "read" after, or null for any other call. The lead's own
 * requests carry the helper's prompt too, but only after its spawn call. */
function helperStage(body) {
  if (!body.includes(AGENT_HELPER_PROMPT) || body.includes("smoke-agent-spawn")) return null;
  return body.includes("smoke-helper-read") ? "read" : "start";
}

/** When the mock first saw each change to the second message's task: the
 * helper's two calls ("start", "read") and the lead's request after it
 * closed the task ("done"). */
const taskSeen = {};

/** While set, the mock holds its answer to each of the helper's calls
 * (`start`, `read`), so the test can read the Tasks panel at that point. */
let helperHold = null;

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
      const answer = () => {
        res.writeHead(200, {
          "Content-Type": "text/event-stream",
          "Content-Length": Buffer.byteLength(payload),
          Connection: "close",
        });
        res.end(payload);
      };
      const stage = helperStage(body);
      if (stage) taskSeen[stage] ??= Date.now();
      const hold = stage ? helperHold?.[stage] : agentHold && body.includes(AGENT_PROMPT) ? agentHold : null;
      if (hold) void hold.then(answer);
      else answer();
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

async function wd(method, path, body, retried = false) {
  const res = await fetch(`${DRIVER}${path}`, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json();
  if (!res.ok) {
    // WebKitGTK refuses a click or type on an element that is still moving (a
    // page just opened, a post just landed, text reflowing) as "not
    // interactable" / "not focusable". Wait for it to sit still and try
    // again; one that never does still fails.
    const act = /^(\/session\/[^/]+)\/element\/([^/]+)\/(click|value)$/.exec(path);
    if (act && json.value?.error === "element not interactable" && !retried) {
      const [, s, el] = act;
      await waitFor("the element to sit still", async () => ((await settled(s, el)) ? true : null), 10000);
      return wd(method, path, body, true);
    }
    throw new Error(`${method} ${path}: ${JSON.stringify(json.value)}`);
  }
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
// The app reads this as a plan with no picker (intent_tests.rs pins it).
const PLAN_PROMPT = "make a plan to tidy the smoke notes";
const PLAN_TEXT = "1. Read the smoke notes. 2. Merge the duplicates.";
const PLAN_REPLY = "Plan approved, ready to tidy";
const DESK_PROMPT = "clear out the old folder from the terminal";
const DESK_TEXT = "rm -rf ~/old";
const DESK_REPLY = "Left the old folder alone";
const EP_PROMPT = "tidy the old folder as a watched session";
const EP_TEXT = "rm -rf ~/old";
const EP_REPLY = "Waiting for your OK on the delete";
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
// Right after a turn ends the window can still be moving (the reply settles,
// a card leaves), and WebKitGTK then refuses the click as "not interactable".
// So click only once the tab sits still, and click again until it is pressed.
async function tab(s, label) {
  const button = `//nav[contains(@class,'tabs')]/button[normalize-space()='${label}']`;
  await waitFor(`the ${label} view`, async () => {
    const [on] = await wd("POST", `${s}/elements`, {
      using: "xpath",
      value: `${button}[@aria-pressed='true']`,
    });
    if (on) return true;
    const [el] = await wd("POST", `${s}/elements`, { using: "xpath", value: button });
    if (el && (await settled(s, el[ELEMENT]))) {
      await wd("POST", `${s}/element/${el[ELEMENT]}/click`, {});
    }
    return null;
  });
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

/** What sits on top of the element's middle, for a refused click. */
async function onTop(s, el) {
  return wd("POST", `${s}/execute/sync`, {
    script:
      "const r = arguments[0].getBoundingClientRect(); const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2); return hit ? `${hit.tagName.toLowerCase()}.${hit.className} at ${Math.round(r.x)},${Math.round(r.y)}` : 'nothing';",
    args: [{ [ELEMENT]: el }],
  });
}

/** Clicks the element once it sits still. When a turn ends, the composer row
 * changes (Stop turns back into Send, the chip row comes in) and the session
 * switch and panel above it move up; a click sent mid-move is refused as
 * "element click intercepted" or lands on whatever moved under it. So wait
 * until the element holds its place, and while something else still covers
 * it, say what and wait for it to go. */
async function clickStill(s, el, what) {
  await waitFor(`${what} to take a click`, async () => {
    if (!(await settled(s, el))) return null;
    try {
      await wd("POST", `${s}/element/${el}/click`, {});
    } catch (err) {
      if (!err.message.includes("element click intercepted")) throw err;
      throw new Error(`${what} is under ${await onTop(s, el)}`);
    }
    return true;
  });
}

/** Waits for a turn to end: a finished reply holds `text` (a session's reply
 * goes on to say what waits) and the composer shows Send again (Stop while
 * the turn runs). */
async function turnOver(s, text) {
  await waitFor(`the reply ${JSON.stringify(text)} and Send`, async () => {
    // Only the Chat tab's own log and composer: the Code tab's agent window
    // has a second pair once that tab has opened.
    const replies = await wd("POST", `${s}/elements`, {
      using: "css selector",
      value: "section[aria-label='Chat'] .bubble.assistant:not(.streaming)",
    });
    let seen = false;
    for (const el of replies) {
      if ((await wd("GET", `${s}/element/${el[ELEMENT]}/text`)).includes(text)) seen = true;
    }
    if (!seen) return false;
    const send = await wd("POST", `${s}/elements`, {
      using: "xpath",
      value:
        "//section[@aria-label='Chat']//form[contains(@class,'composer')]//button[@type='submit'][normalize-space()='Send']",
    });
    return send.length === 1;
  });
}

/** The top-row category each Settings page sits in (src/sections.ts). */
const SETTINGS_CATEGORY = {
  Account: "Account",
  Appearance: "Look & feel",
  Defaults: "AI & chat",
  Personality: "AI & chat",
  Playbooks: "AI & chat",
  Pulse: "AI & chat",
  "Local AI": "AI & chat",
  General: "Your PC",
  Behavior: "Your PC",
  Background: "Your PC",
  "Privacy & data": "Privacy & safety",
  Permissions: "Privacy & safety",
  "Web access": "Privacy & safety",
  About: "Help & updates",
  Labs: "Help & updates",
};

/** A Settings section by its label: its category in the top row, then the
 * page in the side menu. */
async function section(s, label) {
  const category = SETTINGS_CATEGORY[label];
  if (!category) throw new Error(`no Settings category for ${label}`);
  const tab = await waitFor(`the ${category} category`, () =>
    wd("POST", `${s}/element`, {
      using: "xpath",
      value: `//nav[contains(@class,'settings-tabs')]/button[normalize-space()='${category}']`,
    }),
  );
  await wd("POST", `${s}/element/${tab[ELEMENT]}/click`, {});
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

// Personal: open Notes files, Remember a line, check it lands in the work
// folder's MEMORY.md, then search for it the way chat recall would.
async function memory(s, workspace) {
  await tab(s, "Personal");
  // Tabs + Reflect/Restore sit above the notes <details>, which renders open.
  // Clicking its summary would toggle it shut and leave New note unfocusable,
  // so make sure it is open instead of toggling it.
  const notes = await waitFor("the personal page", () => find(s, ".memory-files details"));
  await wd("POST", `${s}/execute/sync`, {
    script: "arguments[0].open = true;",
    args: [{ [ELEMENT]: notes }],
  });
  const input = await waitFor("the notes files", () => find(s, "input[aria-label='New note']"));
  await wd("POST", `${s}/element/${input}/value`, { text: MEMORY_NOTE });
  await click(s, ".memory-files form.add button[type='submit']");
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
  await click(s, ".memory-files form.add:last-of-type button[type='submit']");
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
// For You. Like on the post writes the ledger line to memory/pulse-ledger.md.
// Saving the feed instructions makes the writer post at once: it reads the
// mocked sources, asks the mocked model, and caches the story's image, which
// opens in the lightbox. Dismiss takes the first post away. After the post
// the idea writer adds one idea; its one row reads Like, Let's do it, I don't
// like, Dismiss, with no menu, and Like is kept for the next run. A click on
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
  await click(s, ".pulse .feed-card .feed-foot button[aria-label='Like']");
  await waitFor("the loved post", async () => {
    const el = await find(s, ".pulse .feed-card .feed-foot button[aria-label='Like']");
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

  // No menu: Dismiss, last in the row, asks first then takes the post away.
  const dismiss = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: `${post}//div[contains(@class,'card-row')]/button[last()][normalize-space()='Dismiss']`,
  });
  await wd("POST", `${s}/element/${dismiss[ELEMENT]}/click`, {});
  const dismissConfirm = await waitFor("dismiss confirm", () =>
    wd("POST", `${s}/element`, {
      using: "xpath",
      value: `${post}//div[@role='group' and @aria-label='Dismiss this post?']//button[normalize-space()='Dismiss']`,
    }).catch(() => null),
  );
  await wd("POST", `${s}/element/${dismissConfirm[ELEMENT]}/click`, {});
  await waitFor("the dismissed post", async () => {
    const left = await wd("POST", `${s}/elements`, { using: "xpath", value: post });
    return left.length === 0;
  });
  console.log("dismiss took the post away");

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
  const menus = await wd("POST", `${s}/elements`, {
    using: "xpath",
    value: `${written}//button[@aria-label='More']`,
  });
  if (menus.length !== 0) throw new Error("the idea still has a menu");
  const rowButtons = await wd("POST", `${s}/elements`, {
    using: "xpath",
    value: `${written}//div[contains(@class,'card-row')]/button`,
  });
  const labels = [];
  for (const b of rowButtons) {
    const t = await wd("GET", `${s}/element/${b[ELEMENT]}/text`);
    labels.push(t.replace(/[\u2661\u2665]/g, "").trim());
  }
  if (labels.join("|") !== "Like|Let's do it|Not interested|Dismiss") {
    throw new Error(`the idea buttons say ${JSON.stringify(labels)}`);
  }
  await shot(s, "pulse-ideas");
  await wd("POST", `${s}/element/${rowButtons[0][ELEMENT]}/click`, {});
  await waitFor("Like kept for the next run", async () => {
    try {
      const state = JSON.parse(readFileSync(join(config, "pulse-ideas.json"), "utf8"));
      return state.moreLike?.[0] === PULSE_IDEA.title ? true : null;
    } catch {
      return null;
    }
  });
  console.log(`idea row: ${labels.join(", ")}; Like kept for the next run`);

  // The rows stay disabled until Like has returned, and the kept
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

// A plan-shaped message plans on its own: the turn is read-only, so a write is
// refused without a card, and the plan waits on its card until Approve plan.
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
  const [picker] = await wd("POST", `${s}/elements`, { using: "css selector", value: "[aria-label='Session mode']" });
  if (picker) throw new Error("the composer still shows a mode picker");
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
  console.log(`plan approved: ${JSON.stringify(PLAN_REPLY)}, no picker needed`);
}

// Settings → Local AI: on Windows smoke, capture Get engine → progress →
// ready → Gemma 4 12B draft offer (PR proof). Then Manual takes a model file whose
// name says its safety training was removed only after the notice is read,
// and Reset to Auto puts local_ai.json back.
async function localAi(s, config) {
  await tab(s, "Settings");
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

      // 3) Installed + ready after the checksum lands the binary. A download
      // error or a 120 s stall fails this step: the ready shot is only ever a
      // real install, never a planted stub.
      let ready = null;
      let failure = null;
      const readyDeadline = Date.now() + 120000;
      while (Date.now() < readyDeadline && !ready && !failure) {
        const errEls = await wd("POST", `${s}/elements`, {
          using: "css selector",
          value: ".local-ai .local-engine-error",
        });
        if (errEls.length > 0) {
          failure = await wd("GET", `${s}/element/${errEls[0][ELEMENT]}/text`);
          break;
        }
        const t = await textOf(s, ".local-ai .local-engine .note");
        if (/llama-server is ready/i.test(t)) ready = t;
        else await sleep(250);
      }
      if (!ready) {
        const cancel = await wd("POST", `${s}/elements`, {
          using: "xpath",
          value: "//div[contains(@class,'local-engine')]//button[normalize-space()='Cancel']",
        });
        if (cancel.length > 0) await wd("POST", `${s}/element/${cancel[0][ELEMENT]}/click`, {});
        throw new Error(
          failure
            ? `the live engine download failed: ${failure}`
            : "the live engine download made no progress to ready in 120 s",
        );
      }
      await scrollIntoView(s, ".local-ai .local-engine");
      await shot(s, "settings-local-ai-engine-ready");
      console.log(`shot engine ready: ${JSON.stringify(ready)}`);
    } else {
      // A binary on PATH or GROKHUB_LLAMA_SERVER: no download ran, so the shot
      // says so and can't pass for an install.
      await shot(s, "settings-local-ai-engine-skipped");
      console.log("Get engine absent; download skipped, shot the state as found");
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

// Code tab: the project bar. The folder the chat worked in is added to the
// project list and shows that chat as a session; a second project is switched
// to and back, and each switch moves the app's folder and the editor's files.
async function codeProjects(s, config, workspace) {
  const xp = (value) => wd("POST", `${s}/elements`, { using: "xpath", value });
  const press = async (what, value) => {
    const el = await waitFor(what, async () => (await xp(value))[0]);
    await clickStill(s, el[ELEMENT], what);
  };
  const bound = () => JSON.parse(readFileSync(join(config, "shell.json"), "utf8")).workspace;
  const rows = () => {
    const file = join(config, "projects.json");
    return existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : [];
  };
  const inBar = "//div[contains(@class,'code-projects')]";
  const other = join(config, "second-project");
  mkdirSync(other);
  writeFileSync(join(other, "only-in-second.txt"), "second");

  await tab(s, "Code");
  await press("Add to projects", `${inBar}//button[normalize-space()='Add to projects']`);
  await waitFor("the folder in the project list", () => rows().some((r) => r.kind === "project" && r.path === bound()));
  await waitFor("the chat as a session of this project", () =>
    find(s, ".code-sessions ul[aria-label='Sessions'] button"),
  );
  const first = bound();

  // A project made outside the tab is there the next time the tab shows.
  writeFileSync(
    join(config, "projects.json"),
    JSON.stringify([...rows(), { id: "proj-smoke-2", name: "Second smoke project", kind: "project", path: other, parent: null, open: false }]),
  );
  await tab(s, "Chat");
  await tab(s, "Code");
  await press("Switch project", `${inBar}//button[normalize-space()='Switch project']`);
  await shot(s, "code-projects");
  await press("the second project's row", `${inBar}//ul[@aria-label='Projects']//button[contains(.,'Second smoke project')]`);
  await waitFor("the app in the second project", () => realpathSync(bound()) === realpathSync(other));
  await waitFor("the second project's name on the bar", async () =>
    (await textOf(s, ".code-projects > .code-head .card-title")) === "Second smoke project",
  );
  await waitFor("the second project's file in the editor", async () =>
    (await xp("//section[@aria-label='Code']//button[normalize-space()='only-in-second.txt']"))[0],
  );
  const stale = await xp(`${inBar}//ul[@aria-label='Sessions']//button`);
  if (stale.length) throw new Error("the second project lists the first project's sessions");
  await shot(s, "code-project-switched");

  await press("Switch project", `${inBar}//button[normalize-space()='Switch project']`);
  await press("the first project's row", `${inBar}//ul[@aria-label='Projects']//button[not(@disabled)]`);
  await waitFor("the app back in the first project", () => bound() === first);
  if (realpathSync(first) !== realpathSync(workspace)) throw new Error(`the first project was ${first}, not ${workspace}`);
  await waitFor("the first project's session again", () =>
    find(s, ".code-sessions ul[aria-label='Sessions'] button"),
  );
  console.log("code tab: added the folder as a project, switched to a second project and back");
}

// The Code tab's agent window: a message sent there runs on the Code tab, in
// the folder of the project its session started in. While its reply runs the
// app is moved to the second project from Chat (`/project bind`); the note
// must still land in the first project. The session plans first: its plan
// shows as a card that can be edited, is saved with the session, and nothing
// is written until Approve plan. Its to-do shows in the Tasks panel, and the
// session is listed under the first project.
async function codeAgent(s, config, workspace) {
  const xp = (value) => wd("POST", `${s}/elements`, { using: "xpath", value });
  const bound = () => JSON.parse(readFileSync(join(config, "shell.json"), "utf8")).workspace;
  const other = join(config, "second-project");
  const pane = "aside.code-agent";
  const name = workspace.split(/[\\/]/).filter(Boolean).pop();

  await tab(s, "Code");
  await waitFor("the session's folder under the agent window's title", async () =>
    (await textOf(s, `${pane} > .code-head .note`)) === `Works in ${name}`,
  );
  const sessionsBefore = (await xp("//div[contains(@class,'code-sessions')]//ul[@aria-label='Sessions']//button")).length;

  let release = () => {};
  agentHold = new Promise((ok) => (release = ok));
  const box = await find(s, `${pane} textarea[aria-label='Message the agent']`);
  await wd("POST", `${s}/element/${box}/value`, { text: AGENT_PROMPT });
  await click(s, `${pane} form.composer button[type='submit']`);
  await waitFor("the agent window's reply to start", async () =>
    (await xp("//aside[contains(@class,'code-agent')]//form[contains(@class,'composer')]//button[normalize-space()='Stop']"))[0],
  );
  await waitFor("the model asked from the agent window", () => mockSeen.some((c) => c.body.includes(AGENT_PROMPT)));

  // The app moves to the second project while that reply waits on the model.
  await tab(s, "Chat");
  const chatBox = await find(s, "section[aria-label='Chat'] textarea[aria-label='Message']");
  await wd("POST", `${s}/element/${chatBox}/value`, { text: `/project bind ${other}` });
  await click(s, "section[aria-label='Chat'] form.composer button[type='submit']");
  await waitFor("the app in the second project", () => realpathSync(bound()) === realpathSync(other));
  await tab(s, "Code");
  await waitFor("the second project's name on the bar", async () =>
    (await textOf(s, ".code-projects > .code-head .card-title")) === "Second smoke project",
  );
  // The running session stays in the agent window, with its own folder.
  if ((await textOf(s, `${pane} > .code-head .note`)) !== `Works in ${name}`) {
    throw new Error("the running session's folder changed with the app's project");
  }
  await shot(s, "code-agent-running");

  /** The note lands in the session's project, never the app's, and the
   * reply ends in the agent window. */
  const wrote = async (file, reply) => {
    const note = join(workspace, file);
    await waitFor(`${file} in the session's own project`, async () => {
      if (existsSync(note)) return true;
      // Ask me first: the write waits on its card in the agent window.
      const cards = await wd("POST", `${s}/elements`, {
        using: "css selector",
        value: `${pane} .card.permission .card-buttons button:not([disabled])`,
      });
      const allow = cards[0];
      if (allow && (await settled(s, allow[ELEMENT]))) {
        await wd("POST", `${s}/element/${allow[ELEMENT]}/click`, {}).catch(() => {});
      }
      return null;
    });
    if (readFileSync(note, "utf8") !== AGENT_NOTE_TEXT) throw new Error(`${file} has the wrong words`);
    if (existsSync(join(other, file))) throw new Error(`${file} landed in the project the app moved to`);
    await waitFor(`the agent window's reply ${JSON.stringify(reply)}`, async () => {
      const replies = await wd("POST", `${s}/elements`, {
        using: "css selector",
        value: `${pane} .bubble.assistant:not(.streaming)`,
      });
      for (const el of replies) {
        if ((await wd("GET", `${s}/element/${el[ELEMENT]}/text`)).includes(reply)) return true;
      }
      return null;
    });
    await waitFor("Send again in the agent window", async () =>
      (await xp("//aside[contains(@class,'code-agent')]//form[contains(@class,'composer')]//button[@type='submit'][normalize-space()='Send']"))[0],
    );
  };

  release();
  agentHold = null;
  // The first message plans: the plan waits on its card, where it can be
  // edited, and it is saved with the session. Nothing is written yet.
  const planCard = await waitFor("the plan card in the agent window", () =>
    find(s, `${pane} .card.elicit[aria-label="The plan"]`),
  );
  const draft = await find(s, `${pane} textarea[aria-label='Plan']`);
  const drafted = await wd("GET", `${s}/element/${draft}/property/value`);
  if (drafted !== AGENT_PLAN) throw new Error(`the plan box held ${JSON.stringify(drafted)}`);
  const lead = (c) => c.body.includes(AGENT_PROMPT) && c.body.includes("You are the lead for this session.");
  if (!mockSeen.some(lead)) {
    throw new Error("the agent window's first message carried no lead note");
  }
  await waitFor("the plan saved with the session", () => {
    const sessions = join(config, "sessions");
    if (!existsSync(sessions)) return null;
    return readdirSync(sessions).some((id) => {
      const file = join(sessions, id, "plan.md");
      return existsSync(file) && readFileSync(file, "utf8") === AGENT_PLAN;
    })
      ? true
      : null;
  });
  await sleep(500);
  if (existsSync(join(workspace, AGENT_NOTE)) || existsSync(join(other, AGENT_NOTE))) {
    throw new Error("the agent window wrote the note before its plan was approved");
  }
  if (mockSeen.some((c) => c.body.includes("smoke-agent-plan") && c.body.includes("function_call_output"))) {
    throw new Error("the planning turn went on before its plan was answered");
  }
  await shot(s, "code-agent-plan");
  const approve = await wd("POST", `${s}/element/${planCard}/element`, {
    using: "xpath",
    value: ".//button[normalize-space()='Approve plan']",
  });
  await clickStill(s, approve[ELEMENT], "Approve plan in the agent window");
  console.log("code tab: the agent window planned first, saved the plan and waited for Approve plan");
  await wrote(AGENT_NOTE, AGENT_REPLY);

  // A message that starts after the move: the app is in the second project,
  // and the session still works in the first.
  if (realpathSync(bound()) !== realpathSync(other)) throw new Error("the app left the second project early");
  const again = await find(s, `${pane} textarea[aria-label='Message the agent']`);
  // The lead hands the task to a helper. The mock holds each of the helper's
  // two model calls, so the task stands still while the panel is read.
  let startHelper = () => {};
  let endHelper = () => {};
  helperHold = {
    start: new Promise((ok) => (startHelper = ok)),
    read: new Promise((ok) => (endHelper = ok)),
  };
  const panel = `${pane} section[aria-label='Agent tasks']`;
  const todoRow = `${panel} ul[aria-label='Tasks'] li[aria-label='${AGENT_TODO}']`;
  const helperRow = `${panel} ul[aria-label='Helpers and commands'] li[aria-label='${AGENT_HELPER_TITLE}']`;
  /** The first match's own text, shown or not; "" when nothing matches. */
  const pageText = (css) =>
    wd("POST", `${s}/execute/sync`, {
      script: "const el = document.querySelector(arguments[0]); return el ? el.textContent : '';",
      args: [css],
    });
  /** Waits for the mock to see the change, then reads the panel until it
   * shows it. The panel has LIVE_MS from the moment the mock saw it. */
  const live = async (stage, what, css, ok) => {
    // A tight wait: a slow look here would eat the panel's LIVE_MS.
    for (const giveUp = Date.now() + 60000; taskSeen[stage] === undefined; await sleep(25)) {
      if (Date.now() > giveUp) throw new Error(`timed out waiting for the mock to see: ${what}`);
    }
    const end = taskSeen[stage] + LIVE_MS;
    for (;;) {
      const late = Date.now() > end;
      const text = await pageText(css);
      if (ok(text)) return text;
      if (late) {
        throw new Error(
          `${what}: ${LIVE_MS} ms after the change the Tasks panel still read ${JSON.stringify(text)}; it must update when the app says the tasks changed`,
        );
      }
      await sleep(25);
    }
  };
  await wd("POST", `${s}/element/${again}/value`, { text: AGENT_PROMPT_2 });
  await click(s, `${pane} form.composer button[type='submit']`);
  await live("start", "the helper started", `${todoRow} .card-status`, (text) => text === "Running · helper s1");
  startHelper();
  const stepped = await live("read", "the helper took its first step", `${todoRow} .task-progress`, (text) =>
    /^Step 1: read_file · (\d+ tokens · )?\d+s$/.test(text),
  );
  if ((await pageText(`${todoRow} .card-status`)) !== "Running · helper s1") {
    throw new Error("the task left Running while its helper was still working");
  }
  await shot(s, "code-agent-tasks-live");
  endHelper();
  await live("done", "the lead closed the task", `${todoRow} .card-status`, (text) => text === "Done · helper s1");
  helperHold = null;
  console.log(`code tab: the Tasks panel followed the helper live (running, ${JSON.stringify(stepped)}, done)`);
  await wrote(AGENT_NOTE_2, AGENT_REPLY_2);
  // The helper really ran: its answer went back to the lead as the result.
  if (!mockSeen.some((c) => c.body.includes("smoke-agent-spawn") && c.body.includes(AGENT_HELPER_REPLY))) {
    throw new Error("the lead never got its helper's answer");
  }
  // The helper's own row keeps its progress after it ended, with Details.
  await waitFor("the ended helper's row", async () => (await pageText(`${helperRow} .card-status`)) === "Done · s1");
  const spent = await pageText(`${helperRow} .task-progress`);
  if (!/^Step 1: read_file · \d+ tokens · \d+s$/.test(spent)) {
    throw new Error(`the ended helper's progress read ${JSON.stringify(spent)}`);
  }
  const details = await find(s, `${helperRow} button[aria-label='Helper s1 details']`);
  await clickStill(s, details, "Details on the helper's row");
  const dialog = "[role='dialog'][aria-label='Helper s1']";
  await waitFor("the helper's result in its details window", async () =>
    (await pageText(`${dialog} pre.task-result[aria-label='Result']`)).includes(AGENT_HELPER_REPLY),
  );
  const steps = await pageText(`${dialog} ol.task-steps[aria-label='Steps']`);
  if (steps !== "read_file") throw new Error(`the helper's steps read ${JSON.stringify(steps)}`);
  await shot(s, "code-agent-task-details");
  const [close] = await xp("//div[@role='dialog'][@aria-label='Helper s1']//button[normalize-space()='Close']");
  await clickStill(s, close[ELEMENT], "Close in the helper's details window");
  await waitFor("the details window to close", async () =>
    (await wd("POST", `${s}/elements`, { using: "css selector", value: dialog })).length === 0,
  );
  console.log("code tab: Details showed the helper's step and result");
  await helperWork(s, workspace, other);
  await waitFor("the to-do in the Tasks panel", () =>
    find(s, `${pane} section[aria-label='Agent tasks'] li[aria-label='${AGENT_TODO}']`),
  );
  await onView(s, "Code");
  await shot(s, "code-agent-window");

  // Back in the first project, the session is one of its sessions and the
  // agent window starts a new one there.
  await waitFor("the first project again", async () => {
    if (realpathSync(bound()) === realpathSync(workspace)) return true;
    const [open] = await xp("//div[contains(@class,'code-projects')]//ul[@aria-label='Projects']//button[not(@disabled)]");
    if (open) await clickStill(s, open[ELEMENT], "the first project's row");
    else {
      const [sw] = await xp("//div[contains(@class,'code-projects')]//button[normalize-space()='Switch project']");
      if (sw) await clickStill(s, sw[ELEMENT], "Switch project");
    }
    return null;
  });
  await waitFor("the agent's session among the first project's sessions", async () =>
    (await xp("//div[contains(@class,'code-sessions')]//ul[@aria-label='Sessions']//button")).length > sessionsBefore,
  );
  await waitFor("a new session in the agent window", async () =>
    (await wd("POST", `${s}/elements`, { using: "css selector", value: `${pane} .bubble` })).length === 0,
  );
  console.log("code tab: the agent window ran a session in its own project while the app moved, and listed its to-do");
}

/** `git` in the folder, with the user set inline so no git config is read
 * for it or written. Returns what it printed. */
function git(dir, ...args) {
  const run = spawnSync(
    "git",
    ["-c", "user.name=smoke", "-c", "user.email=smoke@example.com", "-c", "commit.gpgsign=false", ...args],
    { cwd: dir, encoding: "utf8" },
  );
  if (run.status !== 0) throw new Error(`git ${args.join(" ")}: ${run.error?.message ?? run.stderr}`);
  return run.stdout.trim();
}

/** Two helpers each write one file in a worktree of their own. The work
 * waits in the Tasks panel's Helper work list: Merge puts the first file in
 * the project as an uncommitted change, a file the project got meanwhile
 * makes the second a conflict that changes nothing, and Discard removes it. */
async function helperWork(s, workspace, other) {
  const pane = "aside.code-agent";
  const panel = `${pane} section[aria-label='Agent tasks']`;
  const row = (w) => `${panel} ul[aria-label='Helper work'] li[aria-label='${w.title}']`;
  const ownRow = (w) => `${panel} ul[aria-label='Helpers and commands'] li[aria-label='${w.title}']`;
  const todoRow = (w) => `${panel} ul[aria-label='Tasks'] li[aria-label='${w.todo.content}']`;
  const pageText = (css) =>
    wd("POST", `${s}/execute/sync`, {
      script: "const el = document.querySelector(arguments[0]); return el ? el.textContent : '';",
      args: [css],
    });
  const count = async (css) => (await wd("POST", `${s}/elements`, { using: "css selector", value: css })).length;
  /** The button with this label inside the element the selector names. */
  const button = async (css, label) => {
    const el = await wd("POST", `${s}/execute/sync`, {
      script:
        "const at = document.querySelector(arguments[0]); return [...(at ? at.querySelectorAll('button') : [])].find((b) => b.textContent.trim() === arguments[1]) ?? null;",
      args: [css, label],
    });
    return el ? el[ELEMENT] : null;
  };
  const press = async (css, label) => {
    const el = await waitFor(`${label} in ${css}`, () => button(css, label));
    await clickStill(s, el, label);
  };

  // The session's project becomes a git repository with one commit.
  git(workspace, "init", "-q");
  git(workspace, "add", "-A");
  git(workspace, "commit", "-q", "-m", "smoke project");
  const head = git(workspace, "rev-parse", "HEAD");

  const box = await find(s, `${pane} textarea[aria-label='Message the agent']`);
  await wd("POST", `${s}/element/${box}/value`, { text: AGENT_PROMPT_3 });
  await click(s, `${pane} form.composer button[type='submit']`);

  // Ask me first: each helper's write waits on its card in the agent window.
  const ready = "Ready to merge · 1 file";
  await waitFor("both helpers' work ready to merge", async () => {
    const lines = [];
    for (const w of WORK) lines.push(await pageText(`${row(w)} .task-work`));
    if (lines.every((line) => line === ready)) return true;
    const cards = await wd("POST", `${s}/elements`, {
      using: "css selector",
      value: `${pane} .card.permission .card-buttons button:not([disabled])`,
    });
    const allow = cards[0];
    if (allow && (await settled(s, allow[ELEMENT]))) {
      await wd("POST", `${s}/element/${allow[ELEMENT]}/click`, {}).catch(() => {});
    }
    throw new Error(`the Helper work rows read ${JSON.stringify(lines)}`);
  }, 60000);
  for (const w of WORK) {
    if (!mockSeen.some((c) => c.body.includes(w.call) && c.body.includes("function_call_output"))) {
      throw new Error(`the write of ${w.file} never reached its helper`);
    }
    for (const dir of [workspace, other]) {
      if (existsSync(join(dir, w.file))) throw new Error(`${w.file} is in ${dir} before any Merge`);
    }
    const todo = await pageText(`${todoRow(w)} .card-status`);
    if (todo !== `Not merged · helper ${w.helper}`) throw new Error(`the task of ${w.file} read ${JSON.stringify(todo)}`);
  }
  if (git(workspace, "status", "--porcelain") !== "") throw new Error("the project changed before any Merge");
  await waitFor("Send again in the agent window", async () => {
    const send = await wd("POST", `${s}/elements`, {
      using: "xpath",
      value:
        "//aside[contains(@class,'code-agent')]//form[contains(@class,'composer')]//button[@type='submit'][normalize-space()='Send']",
    });
    if (send.length === 1) return true;
    throw new Error(`the composer read ${JSON.stringify(await pageText(`${pane} form.composer`))}`);
  });
  await scrollIntoView(s, row(WORK[0]));
  await shot(s, "helper-work-ready");
  console.log(`code tab: two helpers' work waits (${JSON.stringify(ready)} twice, tasks "Not merged · helper s2" and "… s3"), nothing in the project`);

  // Merge the first: its file is in the project, uncommitted.
  const [first, second] = WORK;
  await press(row(first), "Merge");
  const merged = await waitFor("the first helper's work merged", async () => {
    const line = await pageText(`${ownRow(first)} .task-work`);
    return line === "Merged · 1 file" ? line : null;
  });
  const said = await pageText(`${panel} .work-said`);
  if (said !== "Merged 1 file into the project.") throw new Error(`after Merge the panel said ${JSON.stringify(said)}`);
  if ((await count(row(first))) !== 0) throw new Error("merged work stayed in the Helper work list");
  const landed = readFileSync(join(workspace, first.file), "utf8");
  if (landed !== first.text) throw new Error(`${first.file} holds ${JSON.stringify(landed)}`);
  if (git(workspace, "rev-parse", "HEAD") !== head) throw new Error("Merge moved the project's HEAD");
  const status = git(workspace, "status", "--porcelain");
  if (status !== `?? ${first.file}`) throw new Error(`after Merge git status read ${JSON.stringify(status)}`);
  if (existsSync(join(workspace, second.file))) throw new Error(`${second.file} is in the project before its Merge`);
  console.log(`code tab: Merge put ${first.file} in the project uncommitted (${JSON.stringify(merged)}, ${JSON.stringify(said)}, HEAD unchanged)`);

  // The project gets its own w2.txt, so the second Merge is a conflict.
  writeFileSync(join(workspace, second.file), WORK_MINE);
  await press(row(second), "Merge");
  const conflict = await waitFor("the second helper's work in conflict", async () => {
    const line = await pageText(`${row(second)} .task-work`);
    return line === `Conflict · ${second.file}` ? line : null;
  });
  const mine = readFileSync(join(workspace, second.file), "utf8");
  if (mine !== WORK_MINE) throw new Error(`the conflict changed ${second.file} to ${JSON.stringify(mine)}`);
  if (git(workspace, "rev-parse", "HEAD") !== head) throw new Error("the conflict moved the project's HEAD");
  await waitFor("Merge again on the conflict row", () => button(row(second), "Merge again"));
  await scrollIntoView(s, row(second));
  await shot(s, "helper-work-conflict");
  console.log(`code tab: a file the project got meanwhile made the second Merge ${JSON.stringify(conflict)}, the project's file unchanged`);

  // Discard asks first, then the row is gone.
  await press(row(second), "Discard");
  const dialog = "[role='alertdialog'][aria-label='Discard this work?']";
  const asked = await waitFor("the Discard question", async () => (await pageText(`${dialog} p`)) || null);
  if (asked !== "Delete this helper's 1 changed file? This can't be undone.") {
    throw new Error(`the Discard question read ${JSON.stringify(asked)}`);
  }
  await press(dialog, "Discard");
  await waitFor("the discarded row to go", async () => (await count(row(second))) === 0);
  const gone = await waitFor("the line after Discard", async () => (await pageText(`${panel} .work-said`)) || null);
  if (gone !== "Work discarded.") throw new Error(`after Discard the panel said ${JSON.stringify(gone)}`);
  if (readFileSync(join(workspace, second.file), "utf8") !== WORK_MINE) {
    throw new Error(`Discard changed the project's ${second.file}`);
  }
  console.log(`code tab: Discard asked ${JSON.stringify(asked)} and removed the row (${JSON.stringify(gone)})`);

  // Review: nothing is reviewed until the person asks. The button sends the
  // ask in this session; the lead starts a read-only review helper, which is
  // handed the plan and the change and reports a blocker. The findings show
  // in the panel, the record is in the config folder, and the project's
  // files, HEAD and status are as they were.
  const line = `${panel} .review-line`;
  const before = await waitFor("the Review section", async () => (await pageText(line)) || null);
  if (before !== "Not reviewed") throw new Error(`before any review the panel said ${JSON.stringify(before)}`);
  const records = join(dirname(workspace), "helper-work");
  const reviewFiles = () =>
    readdirSync(records)
      .map((session) => join(records, session, "review.json"))
      .filter((file) => existsSync(file));
  if (reviewFiles().length !== 0) throw new Error("a review record exists before any review");
  const statusBefore = git(workspace, "status", "--porcelain");
  await scrollIntoView(s, line);
  // Review waits while a turn of the session runs; the lead's turn is over.
  await waitFor("Review to be free to press", () =>
    wd("POST", `${s}/execute/sync`, {
      script:
        "const at = document.querySelector(arguments[0]); const b = [...(at ? at.querySelectorAll('button') : [])].find((b) => b.textContent.trim() === 'Review'); return b && !b.disabled ? true : null;",
      args: [panel],
    }),
  );
  await press(panel, "Review");
  const reviewed = await waitFor(
    "the review's findings in the Tasks panel",
    async () => {
      const text = await pageText(line);
      return text.startsWith("Reviewed") ? text : null;
    },
    60000,
  );
  if (reviewed !== "Reviewed: 1 blocker") throw new Error(`after the review the panel said ${JSON.stringify(reviewed)}`);
  const found = await pageText(`${panel} ul[aria-label='Review findings'] li`);
  if (found !== `${REVIEW_FINDING}Blocker`) throw new Error(`the finding read ${JSON.stringify(found)}`);
  await waitFor("Review again and Ask the lead to fix", async () =>
    (await button(panel, "Review again")) && (await button(panel, "Ask the lead to fix")),
  );
  // What the helper was sent: the approved plan, the changed files, the diff
  // and the lead's words last, with no write and no shell among its tools.
  const sent = mockSeen.find((c) => c.body.includes(REVIEW_PROMPT) && !c.body.includes("smoke-review-spawn"));
  if (!sent) throw new Error("the review helper never called the model");
  const request = JSON.parse(sent.body);
  const task = JSON.stringify(request.input);
  for (const part of [
    "Approved plan:\\nWrite one note in the project.",
    `Changed files (2):\\nA ${first.file}\\nA ${second.file}`,
    `+${first.text.trim()}`,
    `+${WORK_MINE.trim()}`,
    `Note from the lead:\\n${REVIEW_PROMPT}`,
  ]) {
    if (!task.includes(part)) throw new Error(`the review helper's task lacks ${JSON.stringify(part)}: ${task}`);
  }
  const tools = request.tools.map((tool) => tool.name);
  for (const banned of ["write", "search_replace", "run_terminal_command"]) {
    if (tools.includes(banned)) throw new Error(`the review helper was offered ${banned}`);
  }
  if (!tools.includes("report_findings")) throw new Error(`the review helper's tools were ${JSON.stringify(tools)}`);
  const files = reviewFiles();
  if (files.length !== 1) throw new Error(`expected one review record, found ${files.length}`);
  const record = JSON.parse(readFileSync(files[0], "utf8"));
  if (record.head !== head || record.base !== head || record.blockers !== 1 || record.planGiven !== true) {
    throw new Error(`the review record read ${JSON.stringify(record)}`);
  }
  if (!/^[0-9a-f]{40}$/.test(record.tree) || record.commits.length !== 0) {
    throw new Error(`the review record's tree and commits read ${JSON.stringify([record.tree, record.commits])}`);
  }
  if (git(workspace, "rev-parse", "HEAD") !== head) throw new Error("the review moved the project's HEAD");
  if (git(workspace, "status", "--porcelain") !== statusBefore) throw new Error("the review changed the project's files");
  // The lead gets the helper's words and, with them, the findings.
  await waitFor("the review helper's answer and its findings back at the lead", () =>
    mockSeen.some(
      (c) =>
        c.body.includes("smoke-review-spawn") &&
        c.body.includes(REVIEW_HELPER_REPLY) &&
        c.body.includes(`Findings from this run\\n\\n- High: ${REVIEW_FINDING}`),
    ),
  );
  await scrollIntoView(s, line);
  await shot(s, "code-agent-review");
  console.log(
    `code tab: Review sent the ask in the session, a read-only helper got the plan and the diff, the lead got its findings, and the panel read ${JSON.stringify(reviewed)} (${JSON.stringify(found)}); the record names HEAD and the project is unchanged`,
  );

  // Checks: read only. With the turn over, the panel lists the tests, the
  // secret scan and the branch, and what is in the way of a commit: the
  // review's blocker and the default branch. A planted fake key is flagged
  // by path and line, never by value, and makes the review stale; taking it
  // out brings the review back. Nothing is committed or staged.
  const checks = `${panel} ul[aria-label='Checks']`;
  const check = (name) => pageText(`${checks} li[aria-label='${name}']`);
  const blocked = () =>
    wd("POST", `${s}/execute/sync`, {
      script:
        "return [...document.querySelectorAll(arguments[0])].map((row) => row.textContent);",
      args: [`${panel} ul[aria-label='In the way of a commit'] li`],
    });
  const IN_TURN = "Wait for the agent to finish its turn.";
  const BLOCKER = "The review found 1 blocker.";
  const way = await waitFor("the checks once the lead's turn is over", async () => {
    const rows = await blocked();
    return rows.includes(BLOCKER) && !rows.includes(IN_TURN) ? rows : null;
  });
  const branch = git(workspace, "symbolic-ref", "--short", "HEAD");
  const onDefault = ["main", "master"].includes(branch)
    ? `${branch} is the main or default branch. The app never commits to it; make a branch first.`
    : "This project's default branch is not known. Use New branch.";
  if (!way.includes(onDefault)) throw new Error(`the checks did not refuse the branch ${branch}: ${JSON.stringify(way)}`);
  const rows = { tests: await check("Tests"), scan: await check("Secret scan"), branch: await check("Branch") };
  const expected = {
    tests: "TestsNo test command is known, so no tests are run.",
    scan: "Secret scanNothing flagged.",
    branch: `Branch${branch}`,
  };
  if (JSON.stringify(rows) !== JSON.stringify(expected)) throw new Error(`the checks read ${JSON.stringify(rows)}`);
  const recordBefore = readFileSync(files[0], "utf8");
  const fakeKey = "sk-abcdefghijklmnopqrstuv";
  writeFileSync(join(workspace, "smoke-key.txt"), `first line\ntoken = "${fakeKey}"\n`);
  await scrollIntoView(s, checks);
  await press(panel, "Check again");
  const FLAGGED = "The secret scan flagged 1 place in the change.";
  const STALE = "Files or commits changed since the review. Review again.";
  const flagged = await waitFor("the planted key in the secret scan", async () => {
    const text = await check("Secret scan");
    return text.includes("smoke-key.txt") ? text : null;
  });
  if (flagged !== "Secret scansmoke-key.txt:2 · looks like a key or a password") {
    throw new Error(`the secret scan read ${JSON.stringify(flagged)}`);
  }
  const whole = await pageText(panel);
  if (whole.includes(fakeKey) || whole.includes("abcdefghij")) throw new Error("the panel shows the flagged value");
  const now = await blocked();
  for (const sentence of [FLAGGED, STALE]) {
    if (!now.includes(sentence)) throw new Error(`with the key planted the checks read ${JSON.stringify(now)}`);
  }
  if (now.includes(BLOCKER)) throw new Error("a stale review still counted its blockers");
  const staleLine = await pageText(line);
  if (staleLine !== "Stale: files or commits changed since the review") {
    throw new Error(`with a file changed the review line read ${JSON.stringify(staleLine)}`);
  }
  await shot(s, "code-agent-checks");
  rmSync(join(workspace, "smoke-key.txt"));
  await press(panel, "Check again");
  await waitFor("the review to count again once the key is gone", async () => {
    const rows = await blocked();
    return rows.includes(BLOCKER) && !rows.includes(STALE) && !rows.includes(FLAGGED) && (await pageText(line)) === reviewed;
  });
  if (readFileSync(files[0], "utf8") !== recordBefore) throw new Error("reading the checks changed the review record");
  if (git(workspace, "rev-parse", "HEAD") !== head) throw new Error("the checks moved the project's HEAD");
  if (git(workspace, "status", "--porcelain") !== statusBefore) throw new Error("the checks changed the project's files");
  console.log(
    `code tab: Checks listed ${JSON.stringify(rows)} and ${JSON.stringify(way)}; a planted key read ${JSON.stringify(flagged)} and made the review stale, and taking it out brought ${JSON.stringify(reviewed)} back; HEAD and the files are unchanged`,
  );
}

/** The window opens 900px wide (tauri.conf.json). Every tab, the brand, the
 * Activity dock and Command must fit it without the page scrolling sideways:
 * a sideways scroll puts a scrollbar along the bottom edge, and WebKit takes
 * a WebDriver click on Send, just above it, for a scrollbar press (mousedown
 * and mouseup, no click, nothing sent). Checked once the capability report
 * has added every tab (Pulse is the last to arrive) and the dock shows. */
async function noSidewaysScroll(s, when) {
  await waitFor("the Pulse tab", () =>
    wd("POST", `${s}/element`, {
      using: "xpath",
      value: "//nav[contains(@class,'tabs')]/button[normalize-space()='Pulse']",
    }),
  );
  const [wide, fits] = await wd("POST", `${s}/execute/sync`, {
    script: "const r = document.documentElement; return [r.scrollWidth, r.clientWidth];",
    args: [],
  });
  if (wide > fits) throw new Error(`the page scrolls sideways ${when}: ${wide}px wide in a ${fits}px window`);
  console.log(`no sideways scroll ${when}: ${wide}px in ${fits}px`);
}

/** A fresh profile opens on the first-run window (AutoSetup.tsx), which
 * covers the page until it's answered, and WebKit then refuses a click on
 * Send as "element click intercepted". Skip it the way a person would, then
 * wait out the corner note the launch pass may leave over the composer. */
async function skipFirstRun(s) {
  const skip = await waitFor(
    "the first-run window",
    () =>
      wd("POST", `${s}/element`, {
        using: "xpath",
        value: "//div[@role='dialog' and contains(@class,'first-run')]//button[normalize-space()='Skip setup']",
      }),
    20000,
  ).catch(() => null);
  if (!skip) {
    console.log("first run: no window (auto setup off)");
    return;
  }
  await wd("POST", `${s}/element/${skip[ELEMENT]}/click`, {});
  // Get to know you (GetToKnowYou.tsx) is the next stage and uses the
  // same backdrop; skip it too.
  const later = await waitFor(
    "the get-to-know-you stage",
    () =>
      wd("POST", `${s}/element`, {
        using: "xpath",
        value: "//div[@role='dialog' and contains(@class,'onboard')]//button[normalize-space()='Skip for now']",
      }),
    10000,
  ).catch(() => null);
  if (later) await wd("POST", `${s}/element/${later[ELEMENT]}/click`, {});
  await waitFor("the first-run window to close", async () => {
    const open = await wd("POST", `${s}/elements`, { using: "css selector", value: ".first-run-backdrop" });
    return open.length === 0;
  });
  await sleep(1000);
  const notes = await wd("POST", `${s}/elements`, { using: "css selector", value: ".auto-note.card .chip" });
  if (notes.length > 0) await wd("POST", `${s}/element/${notes[0][ELEMENT]}/click`, {});
  const also = [later && "get to know you skipped", notes.length > 0 && "corner note closed"].filter(Boolean);
  console.log(`first run: skipped${also.map((a) => `, ${a}`).join("")}`);
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
    await skipFirstRun(s);
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
    await noSidewaysScroll(s, "with the permission card waiting");

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
    await episode(s, config);
    await codeProjects(s, config, workspace);
    await codeAgent(s, config, workspace);
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
  await turnOver(s, DESK_REPLY);
  console.log(`the turn ended on ${JSON.stringify(DESK_REPLY)}`);
}

// A supervised desktop session: with desktop control on, the switch arms the
// chat, the mocked model types a delete, and the session panel shows the card
// waiting for an OK in every approvals mode (Always is still on). Deny writes
// the answer the session reads, and Stop ends the session with nothing run.
async function episode(s, config) {
  await tab(s, "Chat");
  await clickStill(s, await find(s, ".episode-arm input"), "the session switch");
  await waitFor("the switch to read on", async () => {
    const [on] = await wd("POST", `${s}/elements`, { using: "css selector", value: ".episode-arm input:checked" });
    return Boolean(on);
  });
  await shot(s, "episode-armed");
  const box = await waitFor("the message box", () => find(s, "textarea[aria-label='Message']"));
  await wd("POST", `${s}/element/${box}/value`, { text: EP_PROMPT });
  await click(s, "form.composer button[type='submit']");
  await waitFor("the session's waiting card", async () => {
    const [el] = await wd("POST", `${s}/elements`, { using: "css selector", value: ".episode .episode-park" });
    return el ? wd("GET", `${s}/element/${el[ELEMENT]}/text`) : null;
  });
  await turnOver(s, EP_REPLY);
  await shot(s, "episode-card");
  console.log("session card waits for an OK, in Always mode");
  const parkDir = join(config, "harness", "park");
  const deny = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: "//section[contains(@class,'episode')]//div[contains(@class,'episode-park')]//button[normalize-space()='Deny']",
  });
  await clickStill(s, deny[ELEMENT], "the session's Deny");
  await waitFor("the deny answer file", () =>
    existsSync(parkDir) &&
    readdirSync(parkDir).some((f) => f.endsWith(".answer") && readFileSync(join(parkDir, f), "utf8").trim() === "deny"),
  );
  console.log("Deny wrote the answer the session reads");
  const stop = await wd("POST", `${s}/element`, {
    using: "xpath",
    value: "//section[contains(@class,'episode')]//button[normalize-space()='Stop']",
  });
  await clickStill(s, stop[ELEMENT], "the session's Stop");
  await waitFor("the session to end", async () => {
    const [el] = await wd("POST", `${s}/elements`, { using: "css selector", value: ".episode .note[role='status']" });
    return el ? (await wd("GET", `${s}/element/${el[ELEMENT]}/text`)).includes("Stopped") : false;
  });
  await shot(s, "episode-stopped");
  console.log("Stop ended the session");
}

// Web access: the window turns it on, and Chromium pairs from the localhost
// link and chats through this shell's engine. Desktop control and Always
// are still on from the step above, and the web chat must get no desktop
// tools. Then a refused command, and turning it off ends the pairing.
// Diagnostics only (CI's web access step times out now and then and it
// doesn't happen locally): when the page never loads, say which requests
// were still open, whether the server answers at all, and where every
// thread of the shell was waiting. Changes nothing when the step passes.
async function webStallReport(origin, pending) {
  console.log(`web stall: ${pending.size} request(s) still open`);
  for (const [req, at] of pending) console.log(`  open since ${at}ms: ${req.method()} ${new URL(req.url()).pathname}`);
  for (const path of ["/remote/v1/hello", "/"]) {
    const started = Date.now();
    const answer = await fetch(`${origin}${path}`, { signal: AbortSignal.timeout(5000) }).then(
      (res) => `HTTP ${res.status}`,
      (err) => `no answer (${err.name})`,
    );
    console.log(`web stall: ${path} -> ${answer} after ${Date.now() - started}ms`);
  }
  // The shell itself, not the node or xvfb-run whose arguments name it too.
  const exe = resolve(binary);
  const pids = spawnSync("pgrep", ["-f", binary], { encoding: "utf8" })
    .stdout.trim()
    .split(/\s+/)
    .filter((pid) => {
      try {
        return pid && realpathSync(`/proc/${pid}/exe`) === realpathSync(exe);
      } catch {
        return false;
      }
    });
  if (pids.length === 0) console.log(`web stall: no running ${exe}`);
  for (const pid of pids) {
    // sudo -n: a runner allows it without a prompt; elsewhere it just fails.
    const gdb = spawnSync("sudo", ["-n", "gdb", "-p", pid, "-batch", "-ex", "thread apply all bt 25"], {
      encoding: "utf8",
      timeout: 120000,
    });
    const out = `${gdb.stdout ?? ""}${gdb.stderr ?? ""}`.trim();
    console.log(`web stall: threads of pid ${pid}${gdb.status === 0 ? "" : ` (gdb exit ${gdb.status})`}:\n${out || "(no gdb output)"}`);
    if (gdb.status === 0) continue;
    // No gdb: each thread's name, state and the kernel call it sits in.
    const tasks = spawnSync("sh", ["-c", `for t in /proc/${pid}/task/*; do echo "$(cat $t/comm) $(cut -d' ' -f3 $t/stat) $(cat $t/wchan)"; done`], {
      encoding: "utf8",
    });
    console.log(`web stall: tasks of pid ${pid} (name state wchan):\n${tasks.stdout.trim()}`);
  }
}

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
    const opened = Date.now();
    const pending = new Map();
    page.on("request", (r) => pending.set(r, Date.now() - opened));
    page.on("requestfinished", (r) => pending.delete(r));
    page.on("requestfailed", (r) => pending.delete(r));
    try {
      await page.goto(link);
    } catch (err) {
      await webStallReport(origin, pending);
      throw err;
    }
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
