import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionMode, Settings, Staged, StageOutcome } from "./api";

const api = vi.hoisted(() => ({
  sendChat: vi.fn<(id: string, text: string, mode?: SessionMode, attachments?: string[]) => Promise<string>>(),
  stageAttachment: vi.fn<(id: string, name: string, data: string) => Promise<Staged>>(),
  stageAttachmentPaths: vi.fn<(id: string, paths: string[]) => Promise<StageOutcome[]>>(),
  pickAttachments: vi.fn<(id: string) => Promise<StageOutcome[]>>(),
  removeAttachment: vi.fn<(id: string, path: string) => Promise<void>>(),
  drop: null as ((paths: string[]) => void) | null,
}));

vi.mock("./api", () => ({
  // No chat saved from an earlier launch.
  getLastChat: () => Promise.resolve(null),
  loadChat: vi.fn(),
  sendChat: api.sendChat,
  stageAttachment: api.stageAttachment,
  stageAttachmentPaths: api.stageAttachmentPaths,
  pickAttachments: api.pickAttachments,
  removeAttachment: api.removeAttachment,
  onFileDrop: (handler: (paths: string[]) => void) => {
    api.drop = handler;
    return Promise.resolve(() => {});
  },
  getSettings: () =>
    Promise.resolve({
      model: "grok-4.7",
      closeToTray: true,
      hasApiKey: true,
      workspace: "",
      workspacePath: "/home/user",
      approvals: "ask",
      desktopControl: false,
      theme: "system",
      version: "3.5.0",
      summonHotkey: false,
      notifications: true,
    } satisfies Settings),
  saveSettings: vi.fn(),
  answerElicit: vi.fn(),
  answerPermission: vi.fn(),
  cancelChat: vi.fn(),
  closeChat: vi.fn(() => Promise.resolve()),
  onChatEvent: () => Promise.resolve(() => {}),
  onEventGap: () => Promise.resolve(() => {}),
}));

import { REMOTE_TOO_BIG } from "./AttachChips";
import { Chat } from "./Chat";

function staged(name: string, extra: Partial<Staged> = {}): Staged {
  const image = name.endsWith(".png");
  return {
    path: `/home/user/.config/sessions/shell-x/attach/1-0-${name}`,
    name,
    kind: image ? "image" : "text",
    label: `${image ? "Picture" : "File"} ${name}`,
    status: null,
    ...extra,
  };
}

function paste(files: File[]) {
  fireEvent.paste(screen.getByLabelText("Message"), { clipboardData: { files, types: ["Files"] } });
}

describe("composer attachments", () => {
  beforeEach(() => {
    for (const fn of [api.sendChat, api.stageAttachment, api.stageAttachmentPaths, api.pickAttachments, api.removeAttachment]) {
      fn.mockReset();
    }
    api.removeAttachment.mockResolvedValue();
    api.sendChat.mockResolvedValue("end_turn");
    api.drop = null;
  });

  it("a pasted file becomes a chip, sent as base64 under its name", async () => {
    api.stageAttachment.mockResolvedValue(staged("notes.md"));
    render(<Chat onOpenSettings={() => {}} />);
    paste([new File(["hello"], "notes.md", { type: "text/markdown" })]);

    expect(await screen.findByText("File notes.md")).toBeVisible();
    expect(api.stageAttachment).toHaveBeenCalledTimes(1);
    const [, name, data] = api.stageAttachment.mock.calls[0];
    expect([name, data]).toEqual(["notes.md", "aGVsbG8="]);
  });

  it("× removes the chip and its copy", async () => {
    const notes = staged("notes.md");
    api.stageAttachment.mockResolvedValue(notes);
    render(<Chat onOpenSettings={() => {}} />);
    paste([new File(["hello"], "notes.md")]);
    await screen.findByText("File notes.md");

    await userEvent.click(screen.getByRole("button", { name: "Remove notes.md" }));
    expect(screen.queryByText("File notes.md")).toBeNull();
    expect(api.removeAttachment).toHaveBeenCalledWith(api.stageAttachment.mock.calls[0][0], notes.path);
  });

  it("send takes the chips with the message, then clears them", async () => {
    const shot = staged("shot.png");
    api.stageAttachment.mockResolvedValue(shot);
    render(<Chat onOpenSettings={() => {}} />);
    paste([new File([new Uint8Array([0x89, 0x50])], "shot.png", { type: "image/png" })]);
    await screen.findByText("Picture shot.png");

    await userEvent.type(screen.getByLabelText("Message"), "what is this{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalled());
    const id = api.stageAttachment.mock.calls[0][0];
    expect(api.sendChat).toHaveBeenCalledWith(id, "what is this", "chat", [shot.path]);
    expect(screen.queryByRole("button", { name: "Remove shot.png" })).toBeNull();
    // The message shows what went with it.
    expect(screen.getByText(/what is this\s+Picture shot\.png/)).toHaveAttribute("data-role", "user");
  });

  it("a file alone can be sent", async () => {
    api.stageAttachment.mockResolvedValue(staged("notes.md"));
    render(<Chat onOpenSettings={() => {}} />);
    const send = screen.getByRole("button", { name: "Send" });
    expect(send).toBeDisabled();
    paste([new File(["hello"], "notes.md")]);
    await screen.findByText("File notes.md");
    expect(send).toBeEnabled();
    await userEvent.click(send);
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenCalledWith(expect.any(String), "", "chat", [staged("notes.md").path]),
    );
  });

  it("shows the desktop's refusal in its words", async () => {
    api.stageAttachment.mockRejectedValue("huge.png: file is too large");
    render(<Chat onOpenSettings={() => {}} />);
    paste([new File(["x"], "huge.png", { type: "image/png" })]);
    expect(await screen.findByRole("status")).toHaveTextContent("huge.png: file is too large");
    expect(screen.queryByRole("button", { name: "Remove huge.png" })).toBeNull();
  });

  it("a picture the model can't read shows the note and stops the send", async () => {
    api.stageAttachment.mockResolvedValue(staged("shot.png", { status: "model can't read this file" }));
    render(<Chat onOpenSettings={() => {}} />);
    paste([new File(["x"], "shot.png", { type: "image/png" })]);
    expect(await screen.findByText("model can't read this file")).toBeVisible();
    await userEvent.type(screen.getByLabelText("Message"), "look{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("shot.png: model can't read this file");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("the web app refuses a file over 700 KB before sending it, and has no +", async () => {
    render(<Chat onOpenSettings={() => {}} remote />);
    expect(screen.queryByRole("button", { name: "Attach files" })).toBeNull();
    paste([new File([new Uint8Array(700 * 1024 + 1)], "big.txt")]);
    expect(await screen.findByRole("status")).toHaveTextContent(`big.txt ${REMOTE_TOO_BIG}`);
    expect(api.stageAttachment).not.toHaveBeenCalled();
  });

  it("the + opens the picker; a drop on the window stages its paths", async () => {
    api.pickAttachments.mockResolvedValue([
      { staged: staged("a.txt"), error: null },
      { staged: null, error: "tool.exe: file type is not supported" },
    ]);
    api.stageAttachmentPaths.mockResolvedValue([{ staged: staged("b.txt"), error: null }]);
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Attach files" }));
    expect(await screen.findByText("File a.txt")).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("tool.exe: file type is not supported");

    await waitFor(() => expect(api.drop).not.toBeNull());
    api.drop?.(["/home/user/b.txt"]);
    expect(await screen.findByText("File b.txt")).toBeVisible();
    expect(api.stageAttachmentPaths).toHaveBeenCalledWith(expect.any(String), ["/home/user/b.txt"]);
  });

  it("an eleventh file is refused without asking the desktop", async () => {
    let n = 0;
    api.stageAttachment.mockImplementation(() => Promise.resolve(staged(`f${n++}.txt`)));
    render(<Chat onOpenSettings={() => {}} />);
    paste(Array.from({ length: 11 }, (_, i) => new File(["x"], `f${i}.txt`)));
    expect(await screen.findByRole("status")).toHaveTextContent("f10.txt: too many attachments");
    expect(api.stageAttachment).toHaveBeenCalledTimes(10);
    expect(screen.getByRole("button", { name: "Attach files" })).toBeDisabled();
  });
});
