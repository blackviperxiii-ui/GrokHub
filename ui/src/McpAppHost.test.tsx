import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { McpAppHost, type McpAppPayload } from "./McpAppHost";

const invoke = vi.fn();
const shell = vi.fn(() => true);

vi.mock("./transport", () => ({
  call: (...args: unknown[]) => invoke(...args),
  inShell: () => shell(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string, protocol: string) => `${protocol}://localhost/${path}`,
}));

vi.mock("./external", () => ({
  externalUrlOk: (url: string) => url.startsWith("https://"),
  openExternal: vi.fn(() => Promise.resolve()),
}));

const APP: McpAppPayload = {
  server: "inventory",
  tool: "show_inventory",
  resourceUri: "ui://inventory/dashboard",
  html: `<!DOCTYPE html><html><body><p>app</p>
<script>
parent.postMessage({jsonrpc:"2.0",id:1,method:"ui/initialize",params:{appInfo:{name:"t",version:"0"},protocolVersion:"2026-01-26"}},"*");
</script></body></html>`,
  prefersBorder: true,
  toolInput: { warehouse: "berlin" },
  toolResult: { content: [{ type: "text", text: "ok" }] },
};

describe("McpAppHost", () => {
  beforeEach(() => {
    shell.mockReturnValue(true);
    invoke.mockReset();
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "mcp_app_initialize") {
        return {
          protocolVersion: "2026-01-26",
          hostInfo: { name: "GrokHub", version: "0" },
          hostCapabilities: { serverTools: {}, serverResources: {} },
          hostContext: { theme: "system", displayMode: "inline" },
        };
      }
      return {};
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("renders a sandboxed iframe for the MCP App HTML", () => {
    const { container } = render(<McpAppHost id="call-1" app={APP} />);
    const frame = container.querySelector("iframe.mcp-app-frame");
    expect(frame).toBeTruthy();
    expect(frame?.getAttribute("sandbox")).toBe("allow-scripts");
    expect(container.querySelector(".mcp-app")?.className).toContain("bordered");
    expect(container.textContent).toContain("show_inventory");
  });

  it("answers ui/initialize through the host bridge", async () => {
    const { container } = render(<McpAppHost id="call-1" app={APP} />);
    const frame = container.querySelector("iframe.mcp-app-frame") as HTMLIFrameElement;
    // jsdom does not run srcDoc scripts; drive the bridge the same way the app would.
    window.dispatchEvent(
      new MessageEvent("message", {
        data: {
          jsonrpc: "2.0",
          id: 1,
          method: "ui/initialize",
          params: {
            appInfo: { name: "t", version: "0" },
            protocolVersion: "2026-01-26",
          },
        },
        source: frame.contentWindow,
      }),
    );
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("mcp_app_initialize"));
  });

  it("loads the app frame from its own page, so the app's inline script runs under that page's CSP", () => {
    const { container } = render(<McpAppHost id="call-1" app={APP} />);
    const frame = container.querySelector("iframe.mcp-app-frame");
    expect(frame?.getAttribute("src")).toBe("mcp-app://localhost/frame");
    expect(frame?.hasAttribute("srcdoc")).toBe(false);
    expect(frame?.getAttribute("sandbox")).toBe("allow-scripts");
  });

  it("loads the app frame from the desktop's web route in a browser", () => {
    shell.mockReturnValue(false);
    const { container } = render(<McpAppHost id="call-1" app={APP} />);
    const frame = container.querySelector("iframe.mcp-app-frame");
    expect(frame?.getAttribute("src")).toBe("/mcp-app-frame");
  });

  it("hands the app HTML to its frame once the frame says it is ready", () => {
    const { container } = render(<McpAppHost id="call-1" app={APP} />);
    const frame = container.querySelector("iframe.mcp-app-frame") as HTMLIFrameElement;
    const win = frame.contentWindow as Window;
    const post = vi.spyOn(win, "postMessage").mockImplementation(() => {});
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { jsonrpc: "2.0", method: "ui/notifications/sandbox-proxy-ready", params: {} },
        source: win,
      }),
    );
    expect(post.mock.calls).toEqual([
      [
        {
          jsonrpc: "2.0",
          method: "ui/notifications/sandbox-resource-ready",
          params: { html: APP.html },
        },
        "*",
      ],
    ]);
  });
});
