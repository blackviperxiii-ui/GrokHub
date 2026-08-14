#!/usr/bin/env node
/** Readable live tool-trail copy (no network). */
import assert from "node:assert/strict";
import {
  humanizeStreamStatus,
  humanizeHostCommand,
  humanizeComputerCommand,
  summarizeToolOutput,
  streamStatusPill,
  toolRunningMarkdown,
  toolResultMarkdown,
  toolLoopWaitMarkdown,
  toolParallelMarkdown,
  hasToolTrailEvidence,
} from "../src/lib/tool-status.ts";

assert.equal(humanizeHostCommand("ls -la ~/.local/lib/grokhub"), "Looking at files");
assert.equal(humanizeHostCommand("uname -a && whoami"), "Checking this machine");
assert.equal(humanizeHostCommand("ps aux --sort=-%mem | head"), "Checking running programs");
assert.match(humanizeHostCommand("obscure-bin --foo"), /Running a command|Checking/);

assert.equal(humanizeComputerCommand("screenshot"), "Taking a screenshot");
assert.equal(humanizeComputerCommand("click 412 880"), "Clicking on the screen");
assert.equal(humanizeComputerCommand("type Hello"), "Typing");
assert.equal(humanizeComputerCommand("key ctrl+t"), "Pressing a key");

assert.equal(
  humanizeStreamStatus("Host: ls -la /tmp… (3s)"),
  "On your computer: looking at files",
);
assert.equal(
  humanizeStreamStatus("Computer: click 10 20…"),
  "On the desktop: clicking on the screen",
);
assert.equal(humanizeStreamStatus("Summarizing host results…"), "Reading the results…");
assert.equal(humanizeStreamStatus("Thinking…"), "Thinking…");
assert.equal(
  humanizeStreamStatus("Host: running 3 read-only cmds in parallel…"),
  "Checking 3 things on your computer…",
);
assert.equal(humanizeStreamStatus("Waiting for host approval…"), "Waiting for you to approve…");
assert.equal(
  humanizeStreamStatus("Tool loop · round 2 · calling model…"),
  "Reading those results to decide the next step…",
);
assert.equal(humanizeStreamStatus("Streaming · round 2…"), "Writing the next step…");
assert.equal(humanizeStreamStatus("Streaming · Auto"), "Writing a reply…");

assert.equal(streamStatusPill("Host: ls…"), "on your computer");
assert.equal(streamStatusPill("Computer: screenshot"), "on the desktop");
assert.equal(streamStatusPill("Thinking…"), "thinking");

const running = toolRunningMarkdown({
  kind: "host",
  command: "ls -la",
  preface: "I'll check the install.",
  step: { index: 1, total: 2 },
});
assert.match(running, /On your computer/);
assert.match(running, /Looking at files/);
assert.doesNotMatch(running, /stream is live/i);
assert.doesNotMatch(running, /### 🖥️/);
assert.doesNotMatch(running, /Running now/i);

const results = toolResultMarkdown({
  kind: "host",
  preface: "Checked your machine.",
  outputs: [
    "$ ls -la\nexit 0 · 12ms · /home\ntotal 4",
    "$ badcmd\nexit 127 · 3ms · /home\n[stderr]\nnot found",
  ],
  summarizing: true,
});
assert.match(results, /On your computer/);
assert.match(results, /Looking at files/);
assert.match(results, /done|ok/i);
assert.match(results, /failed|couldn't/i);
assert.doesNotMatch(results, /```shell/);
assert.doesNotMatch(results, /stream is live/i);
assert.match(results, /Reading the results/);

const wait = toolLoopWaitMarkdown("Almost there.", 2);
assert.match(wait, /Reading those results|next step/i);
assert.doesNotMatch(wait, /Tool round/);
assert.doesNotMatch(wait, /feeding results back to Grok/i);

const parsed = summarizeToolOutput("$ uname -a\nexit 0 · 8ms · /");
assert.equal(parsed.ok, true);
assert.match(parsed.label, /uname|machine/i);

const conn = summarizeToolOutput("CONNECTOR github list_issues\nok\n3 open");
assert.equal(conn.ok, true);
assert.match(conn.label, /github|Using/i);

const shot = summarizeToolOutput("COMPUTER screenshot\nok\nscreenshot 1280x253");
assert.equal(shot.ok, true);
assert.equal(shot.label, "Taking a screenshot");

const parallel = toolParallelMarkdown({
  kind: "host",
  preface: "I'll check a few things.",
  commands: ["ls -la", "uname -a"],
});
assert.match(parallel, /On your computer/);
assert.match(parallel, /at once/);
assert.match(parallel, /Looking at files/);
assert.doesNotMatch(parallel, /safe host commands/i);

assert.equal(hasToolTrailEvidence("**On your computer**\nLooking at files — `ls`"), true);
assert.equal(hasToolTrailEvidence("Checked your machine.\n**On your computer**"), true);
assert.equal(hasToolTrailEvidence("I'll look at files next."), false);
assert.equal(hasToolTrailEvidence("HOST_CMD: ls"), true);

console.log("smoke-tool-status OK");
