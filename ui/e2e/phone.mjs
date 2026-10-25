// Phone smoke test (cross-platform Phase 4): serve a mock model, install and
// launch a phone smoke build on a booted Android emulator or iOS simulator,
// and read back the report the app's own driver (ui/src/phoneSmoke.ts)
// writes after one chat round-trip. Sign-in is mocked: the build carries the
// mock's loopback URL, so no key is needed.
//
//   node e2e/phone.mjs android <app-debug.apk>
//   node e2e/phone.mjs ios <GrokHub Shell.app>
//
// Build the app with VITE_PHONE_SMOKE=1, GROKHUB_PHONE_SMOKE_URL set to the
// MOCK_URL below and `--features phone-smoke`. Android needs adb, iOS xcrun.

import { execFileSync } from "node:child_process";
import { createServer } from "node:http";

const PORT = 4610;
const MOCK_URL = `http://127.0.0.1:${PORT}/v1/responses`;
const APP_ID = "dev.grokhub.shell";
const REPORT = "phone-smoke.txt";
// Keep in step with ui/src/phoneSmoke.ts.
const PROMPT = "phone smoke: say hello";
const REPLY = "Hello from the phone smoke test";
const TIMEOUT_MS = 5 * 60 * 1000;

const [platform, app] = process.argv.slice(2);
if (!["android", "ios"].includes(platform) || !app) {
  console.error("usage: node e2e/phone.mjs android|ios <app>");
  process.exit(2);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function run(cmd, args) {
  return execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

function tryRun(cmd, args) {
  try {
    return run(cmd, args);
  } catch {
    return null;
  }
}

function sse(events) {
  return events.map((e) => `data: ${JSON.stringify(e)}\n\n`).join("");
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

function startMock() {
  const seen = [];
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      seen.push({ path: req.url, auth: req.headers.authorization, body });
      const payload = sse(textReply(REPLY));
      res.writeHead(200, {
        "Content-Type": "text/event-stream",
        "Content-Length": Buffer.byteLength(payload),
        Connection: "close",
      });
      res.end(payload);
    });
  });
  return new Promise((ok) => server.listen(PORT, "127.0.0.1", () => ok({ server, seen })));
}

const android = {
  launch() {
    // The emulator's 127.0.0.1 is its own; this maps the mock's port to ours.
    run("adb", ["reverse", `tcp:${PORT}`, `tcp:${PORT}`]);
    run("adb", ["install", "-r", app]);
    run("adb", ["shell", "monkey", "-p", APP_ID, "-c", "android.intent.category.LAUNCHER", "1"]);
  },
  report() {
    const found = tryRun("adb", ["shell", "run-as", APP_ID, "find", ".", "-name", REPORT]);
    const path = found?.trim().split("\n")[0];
    return path ? tryRun("adb", ["shell", "run-as", APP_ID, "cat", path]) : null;
  },
  logs() {
    return tryRun("adb", ["logcat", "-d", "-t", "200"]) ?? "";
  },
};

const ios = {
  launch() {
    run("xcrun", ["simctl", "install", "booted", app]);
    run("xcrun", ["simctl", "launch", "booted", APP_ID]);
  },
  report() {
    const data = tryRun("xcrun", ["simctl", "get_app_container", "booted", APP_ID, "data"]);
    if (!data) return null;
    const found = tryRun("find", [data.trim(), "-name", REPORT]);
    const path = found?.trim().split("\n")[0];
    return path ? tryRun("cat", [path]) : null;
  },
  logs() {
    return (
      tryRun("xcrun", [
        "simctl",
        "spawn",
        "booted",
        "log",
        "show",
        "--last",
        "5m",
        "--style",
        "compact",
        "--predicate",
        'process CONTAINS "grokhub"',
      ]) ?? ""
    );
  },
};

async function main() {
  const device = platform === "android" ? android : ios;
  const mock = await startMock();
  console.log(`mock model at ${MOCK_URL}`);
  try {
    device.launch();
    console.log(`launched ${APP_ID} on the ${platform} device`);
    const end = Date.now() + TIMEOUT_MS;
    let report = null;
    while (!report && Date.now() < end) {
      await sleep(2000);
      report = device.report()?.trim() || null;
    }
    if (!report) throw new Error("the app wrote no smoke report");
    console.log(`report: ${report}`);
    if (!report.startsWith("ok ")) throw new Error(`the app's smoke run failed: ${report}`);
    if (report.includes("Pulse")) throw new Error("the phone showed the Pulse tab");
    const calls = mock.seen.filter((c) => c.path === "/v1/responses");
    if (!calls.some((c) => c.body.includes(PROMPT))) {
      throw new Error(`the mock never got the prompt (${calls.length} model calls)`);
    }
    if (calls.some((c) => c.auth !== "Bearer loopback-test")) {
      throw new Error("a model call carried a real credential");
    }
    console.log(`the mock model got the prompt in ${calls.length} call(s)`);
    console.log("phone smoke ok");
  } catch (err) {
    console.error(device.logs().split("\n").slice(-80).join("\n"));
    throw err;
  } finally {
    mock.server.close();
  }
}

main().catch((err) => {
  console.error(err.message);
  process.exit(1);
});
