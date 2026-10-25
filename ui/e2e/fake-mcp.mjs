// A tiny stdio MCP server for the shell's tests: newline JSON-RPC with one
// tool, so the Connectors page has something real to connect to.
import { createInterface } from "node:readline";

const send = (msg) => process.stdout.write(`${JSON.stringify(msg)}\n`);
const reply = (id, result) => send({ jsonrpc: "2.0", id, result });

createInterface({ input: process.stdin }).on("line", (line) => {
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    return;
  }
  if (!msg.method || msg.method.startsWith("notifications/")) return;
  switch (msg.method) {
    case "initialize":
      return reply(msg.id, {
        protocolVersion: "2025-03-26",
        capabilities: { tools: {} },
        serverInfo: { name: "fake", version: "0" },
      });
    case "tools/list":
      return reply(msg.id, {
        tools: [
          {
            name: "echo",
            description: "Echo arguments",
            inputSchema: { type: "object", properties: {} },
          },
        ],
      });
    case "tools/call":
      return reply(msg.id, {
        content: [{ type: "text", text: JSON.stringify(msg.params?.arguments ?? {}) }],
      });
    case "ping":
      return reply(msg.id, {});
    default:
      return send({
        jsonrpc: "2.0",
        id: msg.id,
        error: { code: -32601, message: "method not supported" },
      });
  }
});
