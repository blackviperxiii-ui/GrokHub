import assert from "node:assert/strict";
const {
  detectJobKind,
  jobContractPrompt,
  isolateHostResultsForModel,
  buildChangedRetryNudge,
  detectTriedStrategies,
  tagErrorLayer,
} = await import("../src/lib/pcl-layers.ts");

assert.equal(detectJobKind("hi there"), "chat");
assert.equal(detectJobKind("scan my install and list processes"), "host_investigate");
assert.equal(detectJobKind("refactor this TypeScript component"), "code_build");
assert.ok(jobContractPrompt("host_investigate").includes("HOST_CMD"));

const long = "x".repeat(12000);
const iso = isolateHostResultsForModel([`$ ls\n${long}`], 4500);
assert.ok(iso.isolated);
assert.ok(iso.modelBlock.includes("HOST_RESULT"));
assert.ok(iso.modelBlock.length < long.length);

const tried = detectTriedStrategies("Let me check the system now.", []);
assert.ok(tried.includes("plan_only"));
const nudge = buildChangedRetryNudge(2, tried, { userPrompt: "fix cpu", hostAvailable: true });
assert.ok(/CHANGED STRATEGY|HOST_CMD/i.test(nudge));
assert.equal(tagErrorLayer(new Error("401 unauthorized")), "auth");
console.log("smoke-pcl OK");
