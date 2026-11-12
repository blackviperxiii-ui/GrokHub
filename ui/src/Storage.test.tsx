import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { StorageAnswer, StorageView } from "./storageApi";

const api = vi.hoisted(() => ({
  getStorage: vi.fn<() => Promise<StorageView>>(),
  storageReset: vi.fn<() => Promise<StorageAnswer>>(),
  storageBringOver: vi.fn<() => Promise<StorageAnswer>>(),
  storageOpen: vi.fn<(key: string) => Promise<void>>(),
}));
vi.mock("./storageApi", () => api);

import { Storage } from "./Storage";

function view(over: Partial<StorageView> = {}): StorageView {
  return {
    filesLine: "Your files: /srv/files. Exports, recordings, images and reports go here.",
    canReset: true,
    folders: [
      { key: "Chats", line: "Chats: Exported chats" },
      { key: "Images", line: "Images: Saved images" },
    ],
    models: { key: "models", line: "Local models: /home/you/GrokHub/models" },
    waiting: "2 older items are still in the folders earlier versions used.",
    moves: ["/home/you/GrokHub-Work/imagine → /srv/files/Images"],
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  api.getStorage.mockResolvedValue(view());
  api.storageOpen.mockResolvedValue();
});

describe("Storage", () => {
  it("opens each folder and the models folder", async () => {
    render(<Storage />);
    const list = within(await screen.findByRole("list", { name: "Folders" }));
    await userEvent.click(within(list.getByRole("listitem", { name: "Images: Saved images" })).getByRole("button", { name: "Open" }));
    expect(api.storageOpen).toHaveBeenCalledWith("Images");
    await userEvent.click(
      within(list.getByRole("listitem", { name: "Local models: /home/you/GrokHub/models" })).getByRole("button", { name: "Open" }),
    );
    expect(api.storageOpen).toHaveBeenLastCalledWith("models");
  });

  it("asks before Reset and shows the line", async () => {
    api.storageReset.mockResolvedValue({ view: view({ canReset: false }), status: "New files go to /home/you/Documents/Loadout again." });
    render(<Storage />);
    await userEvent.click(await screen.findByRole("button", { name: "Reset to default" }));
    expect(api.storageReset).not.toHaveBeenCalled();
    await userEvent.click(within(screen.getByRole("group", { name: "Reset to the default folder?" })).getByRole("button", { name: "Reset" }));
    expect(api.storageReset).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("status")).toHaveTextContent("New files go to /home/you/Documents/Loadout again.");
    expect(screen.queryByRole("button", { name: "Reset to default" })).toBeNull();
  });

  it("names the source and destination before bringing files over", async () => {
    api.storageBringOver.mockResolvedValue({ view: view({ waiting: null, moves: [] }), status: "Moved 2 items." });
    render(<Storage />);
    await userEvent.click(await screen.findByRole("button", { name: "Bring them over" }));
    const ask = within(screen.getByRole("group", { name: "Bring them over?" }));
    expect(ask.getByText(/\/home\/you\/GrokHub-Work\/imagine → \/srv\/files\/Images/)).toBeInTheDocument();
    expect(api.storageBringOver).not.toHaveBeenCalled();
    await userEvent.click(ask.getByRole("button", { name: "Bring them over" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Moved 2 items.");
  });

  it("hides Reset on the default folder and Bring over with nothing waiting", async () => {
    api.getStorage.mockResolvedValue(view({ canReset: false, waiting: null, moves: [] }));
    render(<Storage />);
    expect(await screen.findByText("Your files: /srv/files. Exports, recordings, images and reports go here.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reset to default" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Bring them over" })).toBeNull();
  });
});
