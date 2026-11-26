/** In-chat host for an MCP App (`text/html;profile=mcp-app`). The HTML runs
 * in a sandboxed iframe and talks to this page over postMessage JSON-RPC:
 * ui/initialize, tool-input / tool-result, tools/call, ui/open-link. */
import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { call as invoke, inShell } from "./transport";
import { PRODUCT_NAME } from "./brand";
import { openExternal, externalUrlOk } from "./external";

export type McpAppPayload = {
  server: string;
  tool: string;
  resourceUri: string;
  html: string;
  prefersBorder?: boolean;
  toolInput: unknown;
  toolResult: unknown;
};

type JsonRpc =
  | { jsonrpc: "2.0"; id?: string | number | null; method: string; params?: unknown }
  | { jsonrpc: "2.0"; id: string | number | null; result?: unknown; error?: { code: number; message: string } };

const PROTOCOL = "2026-01-26";

/** The frame's page, served with a CSP of its own (src-tauri
 * mcp_app_frame.rs): a `srcdoc` frame would inherit this page's CSP, which
 * blocks the app's inline script. It asks for the app's HTML when it loads. */
function frameUrl(): string {
  return inShell() ? convertFileSrc("frame", "mcp-app") : "/mcp-app-frame";
}

export function McpAppHost({ id, app }: { id: string; app: McpAppPayload }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [ready, setReady] = useState(false);
  const [status, setStatus] = useState("");
  const pending = useRef(new Map<string | number, (msg: JsonRpc) => void>());
  const initialized = useRef(false);

  // A new app starts over: not ready, no status. Adjusted while rendering, so
  // the old app's state never paints under the new one.
  const source = `${id}\n${app.resourceUri}\n${app.html}`;
  const [shown, setShown] = useState(source);
  if (shown !== source) {
    setShown(source);
    setReady(false);
    setStatus("");
  }
  useEffect(() => {
    initialized.current = false;
  }, [source]);

  async function handleRequest(msg: Extract<JsonRpc, { method: string }>) {
    const win = frame.current?.contentWindow;
    if (!win) return;
    const reply = (payload: Omit<Extract<JsonRpc, { id: string | number | null }>, "jsonrpc">) => {
      win.postMessage({ jsonrpc: "2.0", ...payload }, "*");
    };
    switch (msg.method) {
      case "ui/notifications/sandbox-proxy-ready":
        win.postMessage(
          {
            jsonrpc: "2.0",
            method: "ui/notifications/sandbox-resource-ready",
            params: { html: app.html },
          },
          "*",
        );
        return;
      case "ui/initialize": {
        let hostInfo = { name: PRODUCT_NAME, version: "0" };
        let hostCapabilities = { serverTools: {}, serverResources: {} };
        try {
          const init = await invoke<{
            protocolVersion: string;
            hostInfo: { name: string; version: string };
            hostCapabilities: { serverTools: object; serverResources: object };
            hostContext: object;
          }>("mcp_app_initialize");
          hostInfo = init.hostInfo;
          hostCapabilities = init.hostCapabilities;
          reply({
            id: msg.id ?? null,
            result: {
              protocolVersion: init.protocolVersion ?? PROTOCOL,
              hostInfo,
              hostCapabilities,
              hostContext: init.hostContext,
            },
          });
        } catch {
          reply({
            id: msg.id ?? null,
            result: {
              protocolVersion: PROTOCOL,
              hostInfo,
              hostCapabilities,
              hostContext: { theme: "system", displayMode: "inline" },
            },
          });
        }
        return;
      }
      case "ui/notifications/initialized": {
        if (initialized.current) return;
        initialized.current = true;
        setReady(true);
        win.postMessage(
          {
            jsonrpc: "2.0",
            method: "ui/notifications/tool-input",
            params: { arguments: app.toolInput },
          },
          "*",
        );
        win.postMessage(
          {
            jsonrpc: "2.0",
            method: "ui/notifications/tool-result",
            params: app.toolResult,
          },
          "*",
        );
        return;
      }
      case "tools/call": {
        const params = (msg.params ?? {}) as { name?: string; arguments?: unknown };
        const name = typeof params.name === "string" ? params.name : "";
        try {
          const result = await invoke<unknown>("mcp_app_call_tool", {
            server: app.server,
            name,
            arguments: params.arguments ?? {},
          });
          reply({ id: msg.id ?? null, result });
          setStatus("");
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          reply({
            id: msg.id ?? null,
            error: { code: -32000, message },
          });
          setStatus(message);
        }
        return;
      }
      case "ui/open-link": {
        const params = (msg.params ?? {}) as { url?: string };
        const url = typeof params.url === "string" ? params.url : "";
        if (externalUrlOk(url)) {
          void openExternal(url).catch(() => {});
          reply({ id: msg.id ?? null, result: {} });
        } else {
          reply({
            id: msg.id ?? null,
            error: { code: -32602, message: "That link can't be opened." },
          });
        }
        return;
      }
      case "ui/update-model-context":
      case "ui/message":
      case "ui/notifications/size-changed":
      case "notifications/message":
        if ("id" in msg && msg.id != null) reply({ id: msg.id, result: {} });
        return;
      default:
        if ("id" in msg && msg.id != null) {
          reply({
            id: msg.id,
            error: { code: -32601, message: `method not supported: ${msg.method}` },
          });
        }
    }
  }

  useEffect(() => {
    function onMessage(ev: MessageEvent) {
      if (frame.current && ev.source !== frame.current.contentWindow) return;
      const msg = ev.data as JsonRpc;
      if (!msg || msg.jsonrpc !== "2.0") return;
      if ("method" in msg && msg.method) {
        void handleRequest(msg);
        return;
      }
      if ("id" in msg && msg.id != null) {
        const done = pending.current.get(msg.id);
        if (done) {
          pending.current.delete(msg.id);
          done(msg);
        }
      }
    }
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  });

  return (
    <div
      className={app.prefersBorder ? "mcp-app bordered" : "mcp-app"}
      data-mcp-app={id}
      data-ready={ready ? "1" : "0"}
    >
      <div className="mcp-app-head">
        <span className="mcp-app-title">{app.tool}</span>
        <span className="mcp-app-server">{app.server}</span>
      </div>
      <iframe
        key={source}
        ref={frame}
        className="mcp-app-frame"
        title={`MCP App ${app.tool}`}
        sandbox="allow-scripts"
        src={frameUrl()}
        referrerPolicy="no-referrer"
      />
      {status && <p className="mcp-app-status">{status}</p>}
    </div>
  );
}
