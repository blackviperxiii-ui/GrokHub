import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

const transport = vi.hoisted(() => ({
  call: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(),
  inShell: vi.fn<() => boolean>(),
}));
vi.mock("./transport", () => transport);

import { externalUrlOk, openExternal } from "./external";
import { ConfirmRow, ExternalLink, needsSettings, StatusLine } from "./ui";

afterEach(() => vi.restoreAllMocks());

describe("outside links", () => {
  it("only plain web pages count", () => {
    expect(externalUrlOk("https://example.com/a")).toBe(true);
    expect(externalUrlOk("http://localhost:3000")).toBe(true);
    expect(externalUrlOk("javascript:alert(1)")).toBe(false);
    expect(externalUrlOk("file:///etc/passwd")).toBe(false);
    expect(externalUrlOk("not a url")).toBe(false);
  });

  it("the shell asks the desktop to open it", async () => {
    transport.inShell.mockReturnValue(true);
    transport.call.mockResolvedValue(undefined);
    await openExternal("https://example.com/");
    expect(transport.call).toHaveBeenCalledWith("open_external", { url: "https://example.com/" });
  });

  it("the web app opens a tab here, never on the desktop", async () => {
    transport.inShell.mockReturnValue(false);
    transport.call.mockClear();
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    await openExternal("https://example.com/");
    expect(open).toHaveBeenCalledWith("https://example.com/", "_blank", "noopener,noreferrer");
    expect(transport.call).not.toHaveBeenCalled();
  });

  it("refuses anything else without calling out", async () => {
    transport.inShell.mockReturnValue(true);
    transport.call.mockClear();
    await expect(openExternal("javascript:alert(1)")).rejects.toThrow();
    expect(transport.call).not.toHaveBeenCalled();
  });

  it("a link click goes through the opener, not the webview", async () => {
    transport.inShell.mockReturnValue(true);
    transport.call.mockResolvedValue(undefined);
    render(<ExternalLink href="https://example.com/x">source</ExternalLink>);
    await userEvent.click(screen.getByRole("link", { name: "source" }));
    expect(transport.call).toHaveBeenCalledWith("open_external", { url: "https://example.com/x" });
  });
});

describe("chat errors", () => {
  it("offer Settings only for key and sign-in trouble", () => {
    expect(needsSettings("Sign in with Grok or add an API key.")).toBe(true);
    expect(needsSettings("API key rejected (HTTP 401): bad key")).toBe(true);
    expect(needsSettings("The system keychain is not available: no service")).toBe(true);
    expect(needsSettings("read_file failed: no such file")).toBe(false);
    expect(needsSettings("transport: connection reset")).toBe(false);
    expect(needsSettings("HTTP 500: server error")).toBe(false);
  });
});

describe("status line and confirm", () => {
  it("errors and results look different", () => {
    const { rerender } = render(<StatusLine note={{ text: "Saved.", error: false }} />);
    expect(screen.getByRole("status")).toHaveClass("status", "status--ok");
    rerender(<StatusLine note={{ text: "Failed.", error: true }} />);
    expect(screen.getByRole("status")).toHaveClass("status", "status--error");
    rerender(<StatusLine note={null} />);
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("the action runs only after it is confirmed", async () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <ConfirmRow question="Delete it?" action="Delete" onConfirm={onConfirm} onCancel={onCancel} />,
    );
    expect(screen.getByRole("group", { name: "Delete it?" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Keep" }));
    expect(onCancel).toHaveBeenCalledOnce();
    expect(onConfirm).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });
});
