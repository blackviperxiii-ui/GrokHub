import { describe, expect, it } from "vitest";
import {
  CHAT_TITLE,
  DONE_TTL_MS,
  chatActivity,
  dockItems,
  dockSummary,
  pruneActivity,
  routineFinished,
  routineStarted,
  stepsLine,
  type ActivityItem,
} from "./activity";
import type { ChatEvent } from "./api";

function fold(events: ChatEvent[], start?: ActivityItem): ActivityItem | undefined {
  return events.reduce<ActivityItem | undefined>(
    (item, event, i) => chatActivity(item, { conversationId: "shell-1", event }, 1000 + i),
    start,
  );
}

const tool = (status: string, title = "Read README.md"): ChatEvent => ({
  kind: "tool",
  id: `t-${status}`,
  title,
  status,
  detail: "",
});

describe("chatActivity", () => {
  it("a turn runs, counts finished steps, and ends done", () => {
    const running = fold([{ kind: "text", text: "Looking" }, tool("in_progress"), tool("completed")]);
    expect(running).toEqual({
      key: "chat:shell-1",
      kind: "chat",
      title: CHAT_TITLE,
      status: "running",
      detail: "Read README.md",
      steps: 1,
      at: 1002,
    });
    expect(stepsLine(running!)).toBe("1 step done");
    const done = fold([{ kind: "done", stop: "end_turn" }], running);
    expect(done?.status).toBe("done");
    expect(done?.detail).toBe("Reply finished");
    expect(done?.steps).toBe(1);
  });

  it("a permission ask or a plan waits on the user", () => {
    expect(
      fold([{ kind: "permission", id: "p1", title: "Run npm test", action: "run", reason: "" }]),
    ).toMatchObject({ status: "waiting", detail: "Needs your OK: Run npm test" });
    const plan: ChatEvent = {
      kind: "elicit",
      id: "e1",
      server: "Plan",
      message: "Step 1",
      mode: "form",
      url: "",
      fieldName: null,
      fieldTitle: "",
      secret: false,
    };
    expect(fold([plan])).toMatchObject({ status: "waiting", detail: "Plan ready for your OK" });
  });

  it("a Deny from a notification puts the waiting turn back to running", () => {
    const ask: ChatEvent = { kind: "permission", id: "p1", title: "write", action: "a", reason: "" };
    const answered: ChatEvent = { kind: "answered", id: "p1", answer: "deny" };
    expect(fold([ask, answered])).toMatchObject({ status: "running", detail: "Thinking…" });
    // Nothing to resume: no row appears, and a finished one stays finished.
    expect(fold([answered])).toBeUndefined();
    const done = fold([ask, { kind: "done", stop: "end_turn" }]);
    expect(fold([answered], done)).toMatchObject({ status: "done", detail: "Reply finished" });
  });

  it("an error fails the turn and new work starts a fresh count", () => {
    const failed = fold([tool("completed"), { kind: "error", message: "The key was rejected." }]);
    expect(failed).toMatchObject({ status: "failed", detail: "The key was rejected.", steps: 1 });
    expect(fold([tool("completed", "Edit app.ts")], failed)).toMatchObject({
      status: "running",
      detail: "Edit app.ts",
      steps: 1,
    });
  });

  it("an error right after a finished turn starts from zero steps", () => {
    const done = fold([tool("completed"), tool("completed"), { kind: "done", stop: "end_turn" }]);
    expect(done).toMatchObject({ status: "done", steps: 2 });
    expect(fold([{ kind: "error", message: "The key was rejected." }], done)).toMatchObject({
      status: "failed",
      detail: "The key was rejected.",
      steps: 0,
    });
  });

  it("a stray done with no turn adds nothing, and suggestions change nothing", () => {
    expect(fold([{ kind: "done", stop: "end_turn" }])).toBeUndefined();
    const running = fold([{ kind: "text", text: "Hi" }]);
    expect(fold([{ kind: "suggestions", items: [] }], running)).toBe(running);
  });
});

describe("routines and the dock list", () => {
  it("a Run now run starts and ends with its result", () => {
    const start = routineStarted("auto-1", "summarize my inbox", 5);
    expect(start).toMatchObject({ key: "run:auto-1", kind: "routine", status: "running" });
    expect(routineFinished(start, false, "API key rejected", 9)).toMatchObject({
      status: "failed",
      detail: "API key rejected",
      at: 9,
    });
  });

  it("waiting leads, then running, then recent results; old results drop", () => {
    const now = 100 * DONE_TTL_MS;
    const at = (key: string, status: ActivityItem["status"], ago: number): ActivityItem => ({
      key,
      kind: "routine",
      title: key,
      status,
      detail: "",
      steps: 0,
      at: now - ago,
    });
    const items = [
      at("old-done", "done", DONE_TTL_MS + 1),
      at("done", "done", 5),
      at("run-a", "running", 50),
      at("run-b", "running", 10),
      at("wait", "waiting", 99),
      at("fail", "failed", 3),
    ];
    const shown = dockItems(items, now);
    expect(shown.map((i) => i.key)).toEqual(["wait", "run-b", "run-a", "fail", "done"]);
    expect(dockSummary(shown)).toBe("1 waiting · 2 running · 1 failed · 1 done");
    const map = Object.fromEntries(items.map((i) => [i.key, i]));
    expect(Object.keys(pruneActivity(map, now))).toEqual(["done", "run-a", "run-b", "wait", "fail"]);
    const fresh = Object.fromEntries(shown.map((i) => [i.key, i]));
    expect(pruneActivity(fresh, now)).toBe(fresh);
  });
});
