import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

  it("Ctrl+Shift+A archives the open chat, or unarchives one opened from the archive", async () => {
    const onNew = vi.fn();
    const onSwitch = vi.fn();
    const view = render(
      <ChatTabs activeId="shell-a" draftEmpty={false} onSwitch={onSwitch} onNew={onNew} onClear={async () => {}} onError={() => {}} />,
    );
    await screen.findByRole("tab", { name: /Alpha chat/i });
    await userEvent.keyboard("{Control>}{Shift>}A{/Shift}{/Control}");
    await waitFor(() => expect(api.chatTabArchive).toHaveBeenCalledWith("shell-a"));
    expect(onNew).toHaveBeenCalledTimes(1);
    expect(api.chatTabUnarchive).not.toHaveBeenCalled();
    view.unmount();

    api.listArchivedChats.mockResolvedValue([tab("shell-old", "Old chat")]);
    render(
      <ChatTabs activeId="shell-old" draftEmpty={false} onSwitch={onSwitch} onNew={onNew} onClear={async () => {}} onError={() => {}} />,
    );
    await screen.findByRole("tab", { name: /Alpha chat/i });
    await userEvent.keyboard("{Control>}{Shift>}A{/Shift}{/Control}");
    await waitFor(() => expect(api.chatTabUnarchive).toHaveBeenCalledWith("shell-old"));
    expect(onSwitch).toHaveBeenCalledWith("shell-old");
    expect(api.chatTabArchive).toHaveBeenCalledTimes(1);
  });

  it("Ctrl+Shift+A waits out a reply, a held key, an open dialog and another letter on the A key", async () => {
    const props = { activeId: "shell-a", draftEmpty: false, onSwitch: () => {}, onNew: () => {}, onClear: async () => {}, onError: () => {} };
    const view = render(<ChatTabs {...props} busy />);
    await screen.findByRole("tab", { name: /Alpha chat/i });
    const press = (key: string, code: string, repeat = false) =>
      fireEvent.keyDown(window, { key, code, ctrlKey: true, shiftKey: true, repeat });
    press("A", "KeyA");
    view.rerender(<ChatTabs {...props} />);
    press("A", "KeyA", true);
    // AZERTY types Q where QWERTY has A.
    press("Q", "KeyA");
    view.rerender(
      <>
        <ChatTabs {...props} />
        <div role="dialog" aria-modal="true" />
      </>,
    );
    press("A", "KeyA");
    view.rerender(<ChatTabs {...props} />);
    await new Promise((done) => setTimeout(done, 0));
    expect(api.chatTabArchive).not.toHaveBeenCalled();
    // AZERTY's A key, at QWERTY's Q.
    press("A", "KeyQ");
    await waitFor(() => expect(api.chatTabArchive).toHaveBeenCalledWith("shell-a"));
    expect(api.chatTabArchive).toHaveBeenCalledTimes(1);
  });

  it("Ctrl+Shift+A looks up a chat the tab lists don't know yet", async () => {
    const onError = vi.fn();
    api.listChatTabs.mockResolvedValueOnce([tab("shell-b", "Beta chat")]);
    api.listChatTabs.mockResolvedValue([tab("shell-new", "Fresh chat"), tab("shell-b", "Beta chat")]);
    const view = render(
      <ChatTabs activeId="shell-new" draftEmpty={false} onSwitch={() => {}} onNew={() => {}} onClear={async () => {}} onError={onError} />,
    );
    await screen.findByRole("tab", { name: /Beta chat/i });
    // Saved since the strip loaded: it is found and archived.
    await userEvent.keyboard("{Control>}{Shift>}A{/Shift}{/Control}");
    await waitFor(() => expect(api.chatTabArchive).toHaveBeenCalledWith("shell-new"));
    view.unmount();
    api.chatTabArchive.mockClear();
    // Never saved (a local answer only): nothing to archive, and no error.
    render(
      <ChatTabs activeId="shell-local" draftEmpty={false} onSwitch={() => {}} onNew={() => {}} onClear={async () => {}} onError={onError} />,
    );
    await screen.findByRole("tab", { name: /Fresh chat/i });
    const calls = api.listChatTabs.mock.calls.length;
    await userEvent.keyboard("{Control>}{Shift>}A{/Shift}{/Control}");
    await waitFor(() => expect(api.listChatTabs.mock.calls.length).toBe(calls + 1));
    await new Promise((done) => setTimeout(done, 0));
    expect(api.chatTabArchive).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();
  });

  it("Ctrl+Shift+A leaves Main, an unsaved chat, and a hidden Chat tab alone", async () => {
    api.listChatTabs.mockResolvedValue([tab("main", "Main", true, true), tab("shell-b", "Beta chat")]);
    const view = render(
      <ChatTabs activeId="main" draftEmpty={false} onSwitch={() => {}} onNew={() => {}} onClear={async () => {}} onError={() => {}} />,
    );
    await screen.findByRole("tab", { name: /Beta chat/i });
    await userEvent.keyboard("{Control>}{Shift>}A{/Shift}{/Control}");
    view.rerender(
      <ChatTabs activeId="shell-new" draftEmpty onSwitch={() => {}} onNew={() => {}} onClear={async () => {}} onError={() => {}} />,
    );
    await userEvent.keyboard("{Control>}{Shift>}A{/Shift}{/Control}");
    view.rerender(
      <ChatTabs
        activeId="shell-b"
        active={false}
        draftEmpty={false}
        onSwitch={() => {}}
        onNew={() => {}}
        onClear={async () => {}}
        onError={() => {}}
      />,
    );
    await userEvent.keyboard("{Control>}{Shift>}A{/Shift}{/Control}");
    expect(api.chatTabArchive).not.toHaveBeenCalled();
    expect(api.chatTabUnarchive).not.toHaveBeenCalled();
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
