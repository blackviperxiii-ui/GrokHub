#!/usr/bin/env node
/** Follow-up queue, protocol strip, version compare, trail hygiene. */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const root = process.cwd();
const follow = await import(pathToFileURL(path.join(root, "src/lib/follow-up.ts")).href);
const { enqueueFollowUp, takeFollowUp, shouldStopOnSubmit, MAX_FOLLOW_UPS } = follow;

assert.equal(MAX_FOLLOW_UPS, 2);
assert.deepEqual(enqueueFollowUp([], "  hi  "), ["hi"]);
assert.deepEqual(enqueueFollowUp(["hi"], "hi"), ["hi"]);
assert.deepEqual(enqueueFollowUp(["a"], "b"), ["a", "b"]);
assert.deepEqual(enqueueFollowUp(["a", "b"], "c"), ["b", "c"]);
assert.deepEqual(takeFollowUp(["one", "two"]), { next: "one", rest: ["two"] });
assert.deepEqual(takeFollowUp([]), { next: null, rest: [] });
assert.equal(shouldStopOnSubmit(true, ""), true);
assert.equal(shouldStopOnSubmit(true, "  continue  "), false);
assert.equal(shouldStopOnSubmit(false, "x"), false);

const { versionNewer } = await import(
  pathToFileURL(path.join(root, "src/lib/version-compare.ts")).href
);
assert.equal(versionNewer("1.1.29", "1.1.28"), true);
assert.equal(versionNewer("1.1.28", "1.1.29"), false);
assert.equal(versionNewer("v1.1.29", "1.1.29"), false);

const { stripToolProtocolForUser } = await import(
  pathToFileURL(path.join(root, "src/lib/strip-tool-protocol.ts")).href
);
const leaked = [
  "Here is what I found.",
  "HOST_RESULT (authoritative):",
  "$ ls -la",
  "exit 0",
  "COMPUTER_RESULT (authoritative — coordinates are screenshot pixels):",
  "A screenshot image is attached.",
  "CONNECTOR_RESULT:",
  "ok",
].join("\n");
const clean = stripToolProtocolForUser(leaked);
assert.match(clean, /Here is what I found/);
assert.doesNotMatch(clean, /HOST_RESULT/);
assert.doesNotMatch(clean, /COMPUTER_RESULT/);
assert.doesNotMatch(clean, /CONNECTOR_RESULT/);
assert.doesNotMatch(clean, /^\$ ls/m);
assert.doesNotMatch(clean, /^exit 0/m);

const trail = await import(pathToFileURL(path.join(root, "src/lib/tool-status.ts")).href);
const running = trail.toolRunningMarkdown({ kind: "host", command: "ls -la" });
assert.match(running, /Looking at files/);
assert.doesNotMatch(running, /`ls/);
const results = trail.toolResultMarkdown({
  kind: "host",
  outputs: ["$ ls -la\nexit 0 · 12ms · /\ntotal 4"],
});
assert.match(results, /Looking at files/);
assert.doesNotMatch(results, /`ls/);
const parallel = trail.toolParallelMarkdown({ kind: "host", commands: ["ls -la"] });
assert.doesNotMatch(parallel, /`ls/);

const grokSrc = fs.readFileSync(path.join(root, "src/lib/grok.ts"), "utf8");
assert.match(grokSrc, /stripToolProtocolForUser/);

const storeSrc = fs.readFileSync(path.join(root, "src/lib/store.ts"), "utf8");
assert.match(storeSrc, /pendingFollowUps/);
assert.match(storeSrc, /enqueueFollowUp/);
assert.match(storeSrc, /Reached the tool-round limit/);
assert.match(storeSrc, /applyLockedDesktop\(/);
assert.doesNotMatch(storeSrc, /Settings → Agent/);
assert.doesNotMatch(storeSrc, /Settings → Memory/);

const chatSrc = fs.readFileSync(path.join(root, "src/components/views/ChatView.tsx"), "utf8");
assert.match(chatSrc, /shouldStopOnSubmit/);
assert.match(chatSrc, /type a follow-up/);

const updateSrc = fs.readFileSync(path.join(root, "src/lib/update.ts"), "utf8");
assert.match(updateSrc, /releases\/latest/);
assert.match(updateSrc, /versionNewer/);

console.log("smoke-polish OK");
