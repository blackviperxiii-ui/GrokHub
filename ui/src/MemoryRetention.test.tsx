import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MemoryRetention as Shape } from "./api";

const api = vi.hoisted(() => ({
  getMemoryRetention: vi.fn<() => Promise<Shape>>(),
  setMemoryRetention: vi.fn<(days: number) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import { MemoryRetention, RETENTION_NOTE } from "./MemoryRetention";

function row(days = 90): Shape {
  return {
    days,
    label: days === 0 ? "Keep everything" : `${days} days`,
    choices: [
      { days: 30, label: "30 days" },
      { days: 60, label: "60 days" },
      { days: 90, label: "90 days" },
      { days: 180, label: "180 days" },
      { days: 365, label: "365 days" },
      { days: 0, label: "Keep everything" },
    ],
  };
}

describe("MemoryRetention", () => {
  beforeEach(() => {
    api.getMemoryRetention.mockReset().mockResolvedValue(row());
    api.setMemoryRetention.mockReset();
  });

  it("shows 90 days by default and saves a choice", async () => {
    api.setMemoryRetention.mockResolvedValue(row(180));
    render(<MemoryRetention />);
    expect(await screen.findByLabelText("Memory retention days")).toHaveValue("90");
    expect(screen.getByText(RETENTION_NOTE)).toBeVisible();
    await userEvent.selectOptions(screen.getByLabelText("Memory retention days"), "180");
    expect(api.setMemoryRetention).toHaveBeenCalledWith(180);
    expect(await screen.findByText("Memory retention: 180 days")).toBeVisible();
  });
});
