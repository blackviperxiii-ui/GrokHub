import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BoardTidy } from "./api";

const api = vi.hoisted(() => ({
  getWorkboardTidy: vi.fn<() => Promise<BoardTidy>>(),
  setWorkboardTidy: vi.fn<(days: number) => Promise<BoardTidy>>(),
}));

vi.mock("./api", () => api);

import { WorkboardTidy } from "./WorkboardTidy";

describe("WorkboardTidy", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("shows auto-archive after 30 days and auto-delete Off by default, and saves a choice", async () => {
    api.getWorkboardTidy.mockResolvedValue({ autoArchiveDays: 30, autoDeleteDays: 0 });
    api.setWorkboardTidy.mockResolvedValue({ autoArchiveDays: 30, autoDeleteDays: 90 });
    render(<WorkboardTidy />);
    expect(await screen.findByText("Auto-archive after 30 days")).toBeVisible();
    const pick = screen.getByLabelText("Auto-delete archived");
    expect(pick).toHaveValue("0");
    expect(screen.getByRole("option", { name: "Off" })).toBeVisible();

    await userEvent.selectOptions(pick, "90");
    expect(api.setWorkboardTidy).toHaveBeenCalledWith(90);
    expect(pick).toHaveValue("90");
    expect(await screen.findByText("Saved.")).toBeVisible();
  });

  it("shows why it could not read the setting", async () => {
    api.getWorkboardTidy.mockRejectedValue("locked");
    render(<WorkboardTidy />);
    expect(await screen.findByText("locked")).toBeVisible();
  });
});
