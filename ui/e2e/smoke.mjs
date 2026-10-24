// tauri-driver smoke test: start a mock model server, launch the built shell
// through WebDriver, send one chat and wait for the mocked reply.
//
//   node e2e/smoke.mjs <path to grokhub-shell binary>
//
// Needs `tauri-driver` on PATH (cargo install tauri-driver) and the platform
// driver: WebKitWebDriver on Linux, msedgedriver on Windows. macOS has none.
// Set SMOKE_NATIVE_DRIVER to that driver's path when it is not on PATH, and
// SMOKE_SCREENSHOT=<file.png> to keep a screenshot of the rendered reply.

import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const REPLY = "Hello from the mock model";
const PROMPT = "ping from the smoke test";
const DRIVER = "http://127.0.0.1:4444";

const binary = process.argv[2];
if (!binary) {
  console.error("usage: node e2e/smoke.mjs <path to grokhub-shell binary>");
  process.exit(2);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function startMock() {
  const seen = [];
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      seen.push({ path: req.url, auth: req.headers.authorization, body });
      const half = REPLY.length >> 1;
      const events = [
        { type: "response.output_text.delta", delta: REPLY.slice(0, half) },
        { type: "response.output_text.delta", delta: REPLY.slice(half) },
        {
          type: "response.completed",
          response: { usage: { input_tokens: 3, output_tokens: 5 }, output: [] },
        },
      ];
      const payload = events.map((e) => `data: ${JSON.stringify(e)}\n\n`).join("");
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

    const calls = mock.seen.filter((c) => c.path === "/v1/responses");
    if (calls.length !== 1) throw new Error(`expected 1 model call, saw ${calls.length}`);
    const sent = JSON.parse(calls[0].body);
    const userText = sent.input?.[0]?.content?.[0]?.text;
    if (userText !== PROMPT) throw new Error(`model got ${JSON.stringify(userText)}`);
    if (calls[0].auth !== "Bearer loopback-test") throw new Error(`auth was ${calls[0].auth}`);
    console.log(`model got the prompt with model=${sent.model}`);
    console.log("smoke ok");
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
