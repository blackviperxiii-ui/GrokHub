import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_FIRST_RUN_V2,
  finishFirstRunV2,
  FIRST_RUN_V2_ORDER,
  FIRST_TASKS,
  loadFirstRunV2,
  nextStep,
  prevStep,
  reopenFirstRunV2,
  saveFirstRunV2,
  SUGGESTED_APPS,
} from "./firstRunV2Api";

beforeEach(() => {
  localStorage.clear();
});

describe("firstRunV2 progress", () => {
  it("defaults to pending at apps", () => {
    expect(loadFirstRunV2()).toEqual(DEFAULT_FIRST_RUN_V2);
    expect(SUGGESTED_APPS).toHaveLength(8);
    expect(FIRST_TASKS.length).toBeGreaterThanOrEqual(2);
  });

  it("round-trips progress and reopen resets to apps", () => {
    saveFirstRunV2({ stage: "pending", step: "skills" });
    expect(loadFirstRunV2()).toEqual({ stage: "pending", step: "skills" });
    expect(reopenFirstRunV2()).toEqual({ stage: "pending", step: "apps" });
    expect(finishFirstRunV2(true).stage).toBe("skipped");
    expect(finishFirstRunV2(false).stage).toBe("done");
  });

  it("walks steps forward and back without skipping sandbox", () => {
    expect(FIRST_RUN_V2_ORDER).toContain("sandbox");
    expect(FIRST_RUN_V2_ORDER[0]).toBe("apps");
    expect(nextStep("apps")).toBe("skills");
    expect(prevStep("apps")).toBeNull();
    expect(prevStep("skills")).toBe("apps");
    expect(nextStep("tour")).toBe("task");
    expect(nextStep("task")).toBe("done");
  });

  it("reads a saved layout step as the apps step", () => {
    localStorage.setItem("shell.firstRunV2", JSON.stringify({ stage: "pending", step: "layout" }));
    expect(loadFirstRunV2()).toEqual({ stage: "pending", step: "apps" });
  });
});
