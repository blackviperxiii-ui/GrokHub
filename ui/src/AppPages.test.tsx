import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Capabilities, ChatEnvelope, MemoryNotes, Settings } from "./api";

// The real Chat, Settings and Memory pages in the real App: what a page keeps
// while another one shows.
const api = vi.hoisted(() => ({
  CORE_ONLY: {
    desktopControl: false,
    pulse: false,
    localAi: false,
    tray: false,
    webAccess: false,
    plugins: false,
  } as Capabilities,
  chatHandlers: [] as ((envelope: ChatEnvelope) => void)[],
  sentTo: [] as string[],
  getCapabilities: vi.fn<() => Promise<Capabilities>>(),
  getSettings: vi.fn<() => Promise<Settings>>(),
  getMemory: vi.fn<() => Promise<MemoryNotes>>(),
}));

vi.mock("./api", () => ({
  CORE_ONLY: api.CORE_ONLY,
  getCapabilities: api.getCapabilities,
  getSettings: api.getSettings,
  getMemory: api.getMemory,
  onChatEvent: (handler: (envelope: ChatEnvelope) => void) => {
    api.chatHandlers.push(handler);
    return Promise.resolve(() => {});
  },
  onEventGap: () => Promise.resolve(() => {}),
  // The turn stays open, so its events keep landing.
  sendChat: (conversationId: string) => {
    api.sentTo.push(conversationId);
    return new Promise<string>(() => {});
  },
}));
vi.mock("./Automations", () => ({ Automations: () => null }));

import { App } from "./App";

function settings(over: Partial<Settings> = {}): Settings {
  return {
    model: "grok-4.7",
    closeToTray: true,
    hasApiKey: true,
    workspace: "",
    workspacePath: "/home/user/work",
    approvals: "ask",
    desktopControl: false,
    theme: "system",
    version: "3.1.0",
    summonHotkey: false,
    notifications: true,
    ...over,
  };
}

function tab(name: string) {
  return within(screen.getByRole("navigation", { name: "Views" })).getByRole("button", { name });
}

describe("App pages", () => {
  beforeEach(() => {
    api.chatHandlers.length = 0;
    api.sentTo.length = 0;
    api.getCapabilities.mockReset().mockResolvedValue(api.CORE_ONLY);
    api.getSettings.mockReset().mockResolvedValue(settings());
    api.getMemory.mockReset();
  });

  it("Settings keeps an unsaved edit after Chat, and an untouched field takes the fresh value", async () => {
    render(<App />);
    await userEvent.click(tab("Settings"));
    const folder = await screen.findByLabelText("Work folder");
    await userEvent.clear(folder);
    await userEvent.type(folder, "/home/user/kept");

    // Chat's approvals chips saved "always" meanwhile.
    api.getSettings.mockResolvedValue(settings({ approvals: "always" }));
    await userEvent.click(tab("Chat"));
    expect(screen.getByLabelText("Work folder")).not.toBeVisible();
    await userEvent.click(tab("Settings"));

    await waitFor(() => expect(screen.getByRole("combobox", { name: "Approvals" })).toHaveValue("always"));
    expect(screen.getByLabelText("Work folder")).toHaveValue("/home/user/kept");
    expect(screen.getByLabelText("Work folder")).toBeVisible();
  });

  it("Settings opens again on the section that was open", async () => {
    render(<App />);
    await userEvent.click(tab("Settings"));
    const nav = await screen.findByRole("navigation", { name: "Settings sections" });
    await userEvent.click(within(nav).getByRole("button", { name: "Account" }));
    await userEvent.click(tab("Chat"));
    await userEvent.click(tab("Settings"));
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Account");
    expect(within(nav).getByRole("button", { name: "Account" })).toHaveAttribute("aria-pressed", "true");
  });

  it("a reply that streams in while Settings shows is in the chat log", async () => {
    render(<App />);
    await userEvent.type(screen.getByLabelText("Message"), "hello{Enter}");
    await waitFor(() => expect(api.sentTo).toHaveLength(1));
    await userEvent.click(tab("Settings"));
    expect(await screen.findByLabelText("Model")).toBeVisible();

    act(() => {
      for (const handler of api.chatHandlers) {
        handler({ conversationId: api.sentTo[0], event: { kind: "text", text: "Hi while away" } });
      }
    });
    await userEvent.click(tab("Chat"));
    expect(within(screen.getByRole("log")).getByText("Hi while away")).toBeVisible();
  });

  it("Memory keeps an unsaved note edit and reloads the rest when it shows again", async () => {
    const notes: MemoryNotes = {
      global: "I like short answers\n",
      workspace: "the build needs node 22\n",
      globalPath: "/cfg/memory/MEMORY.md",
      workspacePath: "/ws/MEMORY.md",
      amr: false,
      writesOn: true,
      rows: [],
    };
    api.getMemory.mockResolvedValue(notes);
    render(<App />);
    await userEvent.click(tab("Memory"));
    const project = await screen.findByLabelText("Project notes");
    await userEvent.type(project, "half typed");

    api.getMemory.mockResolvedValue({ ...notes, global: "I like long answers\n" });
    await userEvent.click(tab("Chat"));
    await userEvent.click(tab("Memory"));

    await waitFor(() => expect(screen.getByLabelText("Global notes")).toHaveValue("I like long answers\n"));
    expect(screen.getByLabelText("Project notes")).toHaveValue("the build needs node 22\nhalf typed");
    expect(api.getMemory).toHaveBeenCalledTimes(2);
  });
});
