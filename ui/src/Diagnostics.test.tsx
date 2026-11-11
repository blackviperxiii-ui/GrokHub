import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatRow, DiagnosticsPage, DiagnosticsPreview, DiagnosticsSaved } from "./api";

const api = vi.hoisted(() => ({
  getDiagnostics: vi.fn<() => Promise<DiagnosticsPage>>(),
  setDiagnostics: vi.fn<(mode: boolean, logs: boolean) => Promise<DiagnosticsPage>>(),
  diagnosticsPreview: vi.fn<(chat: string | null) => Promise<DiagnosticsPreview>>(),
  diagnosticsSave: vi.fn<() => Promise<DiagnosticsSaved>>(),
  listChats: vi.fn<() => Promise<ChatRow[]>>(),
}));

vi.mock("./api", () => api);

import { Diagnostics, DIAGNOSTICS_NOTE, NO_CHAT, NO_CHAT_NOTE } from "./Diagnostics";

const LINES = ["Per turn: user text and final reply (secrets redacted)", "Nothing leaves this machine unless you copy or share the file"];
const off: DiagnosticsPage = { mode: false, includeLogs: false, preview: LINES };
const on: DiagnosticsPage = { mode: true, includeLogs: false, preview: LINES };
const SUMMARY = "Redacted: 2 keys, 1 email, 0 phones, 1 home path";
const shown: DiagnosticsPreview = {
  summary: SUMMARY,
  markdown: "# Diagnostics\nuser: my key is [redacted]",
  turns: 1,
};

describe("Diagnostics", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.listChats.mockResolvedValue([{ id: "shell-1", title: "Plan the trip", created: 1, updated: 2 }]);
  });

  it("is off by default and shows only the switch and its note", async () => {
    api.getDiagnostics.mockResolvedValue(off);
    render(<Diagnostics />);
    expect(await screen.findByText(DIAGNOSTICS_NOTE)).toBeVisible();
    expect(screen.getByRole("switch", { name: "Diagnostics mode" })).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByRole("button", { name: "Make a bundle" })).not.toBeInTheDocument();
  });

  it("turning it on lists what a bundle holds and keeps chat text out until a chat is picked", async () => {
    api.getDiagnostics.mockResolvedValue(off);
    api.setDiagnostics.mockResolvedValue(on);
    render(<Diagnostics />);
    await userEvent.click(await screen.findByRole("switch", { name: "Diagnostics mode" }));
    expect(api.setDiagnostics).toHaveBeenCalledWith(true, false);
    expect(await screen.findByText(LINES[0])).toBeVisible();
    expect(screen.getByRole("combobox", { name: "Chat to include" })).toHaveValue("");
    expect(screen.getByRole("option", { name: NO_CHAT })).toBeInTheDocument();
    expect(screen.getByText(NO_CHAT_NOTE)).toBeVisible();
  });

  it("the logs switch saves with the mode kept", async () => {
    api.getDiagnostics.mockResolvedValue(on);
    api.setDiagnostics.mockResolvedValue({ ...on, includeLogs: true });
    render(<Diagnostics />);
    await userEvent.click(await screen.findByRole("switch", { name: "Include recent app logs" }));
    expect(api.setDiagnostics).toHaveBeenCalledWith(true, true);
    expect(await screen.findByRole("switch", { name: "Include recent app logs" })).toHaveAttribute("aria-checked", "true");
  });

  it("one click previews the bundle for no chat, and Save writes it and clears the preview", async () => {
    api.getDiagnostics.mockResolvedValue(on);
    api.diagnosticsPreview.mockResolvedValue(shown);
    api.diagnosticsSave.mockResolvedValue({ path: "/x/a.zip", status: "Saved a.zip to Diagnostics" });
    render(<Diagnostics />);
    await userEvent.click(await screen.findByRole("button", { name: "Make a bundle" }));
    expect(api.diagnosticsPreview).toHaveBeenCalledWith(null);
    expect(await screen.findByText(SUMMARY)).toBeVisible();
    expect(screen.getByText(/my key is \[redacted\]/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved a.zip to Diagnostics")).toBeVisible();
    expect(screen.queryByLabelText("Diagnostics preview")).not.toBeInTheDocument();
  });

  it("a picked chat goes to the preview, and Copy summary copies the cabin's line", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    api.getDiagnostics.mockResolvedValue(on);
    api.diagnosticsPreview.mockResolvedValue(shown);
    render(<Diagnostics />);
    await userEvent.selectOptions(await screen.findByRole("combobox", { name: "Chat to include" }), "shell-1");
    await userEvent.click(screen.getByRole("button", { name: "Make a bundle" }));
    expect(api.diagnosticsPreview).toHaveBeenCalledWith("shell-1");
    await userEvent.click(await screen.findByRole("button", { name: "Copy summary" }));
    expect(writeText).toHaveBeenCalledWith(SUMMARY);
    expect(await screen.findByText("Summary copied")).toBeVisible();
  });

  it("Cancel drops the preview and writes nothing; a failed build says why", async () => {
    api.getDiagnostics.mockResolvedValue(on);
    api.diagnosticsPreview.mockResolvedValueOnce(shown).mockRejectedValueOnce("No turns to export");
    render(<Diagnostics />);
    await userEvent.click(await screen.findByRole("button", { name: "Make a bundle" }));
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(api.diagnosticsSave).not.toHaveBeenCalled();
    expect(screen.queryByLabelText("Diagnostics preview")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Make a bundle" }));
    expect(await screen.findByText("No turns to export")).toBeVisible();
  });
});
