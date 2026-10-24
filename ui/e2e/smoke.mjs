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

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

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
function mockReply(callNumber) {
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
  const half = REPLY.length >> 1;
  return [
    { type: "response.output_text.delta", delta: REPLY.slice(0, half) },
    { type: "response.output_text.delta", delta: REPLY.slice(half) },
    {
      type: "response.completed",
      response: { usage: { input_tokens: 3, output_tokens: 5 }, output: [] },
    },
  ];
}

function startMock() {
  const seen = [];
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      seen.push({ path: req.url, auth: req.headers.authorization, body });
      const payload = sse(mockReply(seen.filter((c) => c.path === "/v1/responses").length));
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

main().catch((err) => {
  console.error(`smoke failed: ${err.message}`);
  process.exit(1);
});
