import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CLEAR_CHAT_QUESTION } from "./ClearChat";
import type { ChatTab } from "./api";

const api = vi.hoisted(() => ({
  listChatTabs: vi.fn<() => Promise<ChatTab[]>>(),
  listArchivedChats: vi.fn<() => Promise<ChatTab[]>>(),
  listDeletedChats: vi.fn<() => Promise<ChatTab[]>>(),
  chatTabSwitch: vi.fn<(id: string) => Promise<ChatTab>>(),
  chatTabPin: vi.fn<(id: string) => Promise<boolean>>(),
  chatTabRename: vi.fn<(id: string, title: string) => Promise<void>>(),
  chatTabArchive: vi.fn<(id: string) => Promise<ChatTab>>(),
  chatTabUnarchive: vi.fn<(id: string) => Promise<ChatTab>>(),
  chatTabSoftDelete: vi.fn<(id: string) => Promise<ChatTab>>(),
  chatTabRestoreDeleted: vi.fn<(id: string) => Promise<ChatTab>>(),
}));

vi.mock("./api", () => api);

import { CHAT_TABS_LABEL, ChatTabs, HangoutHeader, MAIN_TAB_HINT, NEW_CHAT_TAB } from "./ChatTabs";

function tab(id: string, title: string, pinned = false, main = false): ChatTab {
  return { id, title, pinned, updated: 1, main };
}

describe("ChatTabs", () => {
  beforeEach(() => {
    api.listChatTabs.mockReset().mockResolvedValue([tab("shell-a", "Alpha chat"), tab("shell-b", "Beta chat")]);
    api.listArchivedChats.mockReset().mockResolvedValue([]);
    api.listDeletedChats.mockReset().mockResolvedValue([]);
    api.chatTabSwitch.mockReset().mockImplementation(async (id) => tab(id, id));
    api.chatTabPin.mockReset().mockResolvedValue(true);
    api.chatTabArchive.mockReset().mockResolvedValue(tab("shell-a", "Alpha chat"));
    api.chatTabSoftDelete.mockReset().mockResolvedValue(tab("shell-a", "Alpha chat"));
    api.chatTabUnarchive.mockReset().mockResolvedValue(tab("shell-a", "Alpha chat"));
    api.chatTabRestoreDeleted.mockReset().mockResolvedValue(tab("shell-a", "Alpha chat"));
    api.chatTabRename.mockReset().mockResolvedValue();
  });

  it("renders the strip and switches tabs", async () => {
    const onSwitch = vi.fn();
    render(
      <ChatTabs
        activeId="shell-a"
        draftEmpty={false}
        onSwitch={onSwitch}
        onNew={() => {}}
        onClear={async () => {}}
        onError={() => {}}
      />,
    );
    expect(await screen.findByRole("tablist", { name: CHAT_TABS_LABEL })).toBeVisible();
    expect(screen.getByRole("tab", { name: /Alpha chat/i })).toHaveAttribute("aria-selected", "true");
    await userEvent.click(screen.getByRole("tab", { name: /Beta chat/i }));
    await waitFor(() => expect(api.chatTabSwitch).toHaveBeenCalledWith("shell-b"));
    expect(onSwitch).toHaveBeenCalledWith("shell-b");
  });

  it("Clear asks first before wiping", async () => {
    const onClear = vi.fn(async () => {});
    render(
      <ChatTabs
        activeId="shell-a"
        draftEmpty={false}
        onSwitch={() => {}}
        onNew={() => {}}
        onClear={onClear}
        onError={() => {}}
      />,
    );
    await screen.findByRole("tab", { name: /Alpha chat/i });
    await userEvent.pointer({
      keys: "[MouseRight>]",
      target: screen.getByRole("tab", { name: /Alpha chat/i }),
    });
    const menu = await screen.findByRole("menu", { name: "Chat tab" });
    await userEvent.click(within(menu).getByRole("menuitem", { name: /Clear chat/i }));
    expect(await screen.findByRole("alertdialog", { name: CLEAR_CHAT_QUESTION })).toBeVisible();
    expect(onClear).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Clear chat" }));
    expect(onClear).toHaveBeenCalledTimes(1);
    expect(onClear).toHaveBeenCalledWith("shell-a");
  });

  it("Main shows its mark, keeps its name when empty, and only offers Clear", async () => {
    api.listChatTabs.mockResolvedValue([
      tab("shell-main-1", "Main", true, true),
      tab("shell-a", "Alpha chat"),
    ]);
    const onClear = vi.fn(async () => {});
    render(
      <ChatTabs
        activeId="shell-main-1"
        draftEmpty
        onSwitch={() => {}}
        onNew={() => {}}
        onClear={onClear}
        onError={() => {}}
      />,
    );
    const main = await screen.findByRole("tab", { name: "Main" });
    expect(main).toHaveAttribute("title", MAIN_TAB_HINT);
    expect(main).toHaveClass("main");
    expect(screen.queryByRole("tab", { name: NEW_CHAT_TAB })).toBeNull();

    await userEvent.pointer({ keys: "[MouseRight>]", target: main });
    const menu = await screen.findByRole("menu", { name: "Chat tab" });
    expect(within(menu).getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Clear chat…"]);
    await userEvent.click(within(menu).getByRole("menuitem", { name: /Clear chat/i }));
    await userEvent.click(await screen.findByRole("button", { name: "Clear chat" }));
    expect(onClear).toHaveBeenCalledWith("shell-main-1");
    expect(api.chatTabArchive).not.toHaveBeenCalled();
    expect(api.chatTabSoftDelete).not.toHaveBeenCalled();
  });

  it("a failed Clear shows the error instead of throwing", async () => {
    const onError = vi.fn();
    render(
      <ChatTabs
        activeId="shell-a"
        draftEmpty={false}
        onSwitch={() => {}}
        onNew={() => {}}
        onClear={async () => {
          throw new Error("disk full");
        }}
        onError={onError}
      />,
    );
    const alpha = await screen.findByRole("tab", { name: /Alpha chat/i });
    await userEvent.pointer({ keys: "[MouseRight>]", target: alpha });
    const menu = await screen.findByRole("menu", { name: "Chat tab" });
    await userEvent.click(within(menu).getByRole("menuitem", { name: /Clear chat/i }));
    await userEvent.click(await screen.findByRole("button", { name: "Clear chat" }));
    await waitFor(() => expect(onError).toHaveBeenCalledWith("Error: disk full"));
  });

  it("restores an archived chat from tools", async () => {
    api.listArchivedChats.mockResolvedValue([tab("shell-old", "Old chat")]);
    const onSwitch = vi.fn();
    render(
      <ChatTabs
        activeId="shell-a"
        draftEmpty={false}
        onSwitch={onSwitch}
        onNew={() => {}}
        onClear={async () => {}}
        onError={() => {}}
      />,
    );
    await screen.findByRole("tablist", { name: CHAT_TABS_LABEL });
    await userEvent.click(screen.getByRole("button", { name: "Chat tools" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: /Restore Old chat/i }));
    await waitFor(() => expect(api.chatTabUnarchive).toHaveBeenCalledWith("shell-old"));
    expect(onSwitch).toHaveBeenCalledWith("shell-old");
  });

  it("empty open draft reads New chat", async () => {
    api.listChatTabs.mockResolvedValue([tab("shell-a", "Chat")]);
    render(
      <ChatTabs
        activeId="shell-a"
        draftEmpty
        onSwitch={() => {}}
        onNew={() => {}}
        onClear={async () => {}}
        onError={() => {}}
      />,
    );
    await waitFor(() => expect(api.listChatTabs).toHaveBeenCalled());
    expect(await screen.findByRole("tab", { name: NEW_CHAT_TAB })).toBeVisible();
    expect(screen.getByRole("tab", { name: NEW_CHAT_TAB })).toHaveAttribute("aria-selected", "true");
  });
});

describe("HangoutHeader", () => {
  it("TALK and TYPE keep calling onMode without clearing siblings", async () => {
    const onMode = vi.fn();
    const { rerender } = render(<HangoutHeader mode="type" onMode={onMode} />);
    expect(screen.getByRole("button", { name: "TYPE" })).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(screen.getByRole("button", { name: "TALK" }));
    expect(onMode).toHaveBeenCalledWith("talk");
    rerender(<HangoutHeader mode="talk" onMode={onMode} />);
    expect(screen.getByText(/Voice on/i)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "TYPE" }));
    expect(onMode).toHaveBeenCalledWith("type");
  });
});
