import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_FIRST_RUN_V2,
  finishFirstRunV2,
  FIRST_RUN_V2_ORDER,
  FIRST_TASKS,
  layoutBlurb,
  loadFirstRunV2,
  nextStep,
  prevStep,
  reopenFirstRunV2,
  saveFirstRunV2,
  SUGGESTED_APPS,
} from "./firstRunV2";

beforeEach(() => {
  localStorage.clear();
});

describe("firstRunV2 progress", () => {
  it("defaults to pending at layout", () => {
    expect(loadFirstRunV2()).toEqual(DEFAULT_FIRST_RUN_V2);
    expect(SUGGESTED_APPS).toHaveLength(8);
    expect(FIRST_TASKS.length).toBeGreaterThanOrEqual(2);
  });

  it("round-trips progress and reopen resets to layout", () => {
    saveFirstRunV2({ stage: "pending", step: "apps" });
    expect(loadFirstRunV2()).toEqual({ stage: "pending", step: "apps" });
    expect(reopenFirstRunV2()).toEqual({ stage: "pending", step: "layout" });
    expect(finishFirstRunV2(true).stage).toBe("skipped");
    expect(finishFirstRunV2(false).stage).toBe("done");
  });

  it("walks steps forward and back without skipping sandbox", () => {
    expect(FIRST_RUN_V2_ORDER).toContain("sandbox");
    expect(nextStep("layout")).toBe("apps");
    expect(prevStep("layout")).toBeNull();
    expect(prevStep("apps")).toBe("layout");
    expect(nextStep("tour")).toBe("task");
    expect(nextStep("task")).toBe("done");
  });

  it("layout blurb names Simple and Advanced", () => {
    expect(layoutBlurb("simple")).toContain("More");
    expect(layoutBlurb("advanced")).toContain("groups");
  });
});
