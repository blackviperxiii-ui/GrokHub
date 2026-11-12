import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatArchive as Shape } from "./api";

const api = vi.hoisted(() => ({
  getChatArchive: vi.fn<() => Promise<Shape>>(),
  setChatArchive: vi.fn<(days: number) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import { ARCHIVE_NOTE, ChatArchive } from "./ChatArchive";

function row(days = 14): Shape {
  return {
    days,
    label: days === 0 ? "Off" : `${days} days`,
    choices: [
      { days: 0, label: "Off" },
      { days: 7, label: "7 days" },
      { days: 14, label: "14 days" },
      { days: 30, label: "30 days" },
      { days: 90, label: "90 days" },
    ],
  };
}

describe("ChatArchive", () => {
  beforeEach(() => {
    api.getChatArchive.mockReset().mockResolvedValue(row());
    api.setChatArchive.mockReset();
  });

  it("shows 14 days by default and saves Off", async () => {
    api.setChatArchive.mockResolvedValue(row(0));
    render(<ChatArchive />);
    expect(await screen.findByLabelText("Auto-archive idle chats days")).toHaveValue("14");
    expect(screen.getByText(ARCHIVE_NOTE)).toBeVisible();
    await userEvent.selectOptions(screen.getByLabelText("Auto-archive idle chats days"), "0");
    expect(api.setChatArchive).toHaveBeenCalledWith(0);
    expect(await screen.findByText("Auto-archive off")).toBeVisible();
  });
});
