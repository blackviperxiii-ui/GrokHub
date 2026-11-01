import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ConnectorRow, ConnectorsChanged } from "./api";

const api = vi.hoisted(() => ({
  listConnectors: vi.fn<() => Promise<ConnectorRow[]>>(),
  checkConnectors: vi.fn<() => Promise<ConnectorRow[]>>(),
  restartConnector: vi.fn<(name: string) => Promise<ConnectorsChanged>>(),
  signOutConnector: vi.fn<(name: string) => Promise<ConnectorsChanged>>(),
  disconnectConnector: vi.fn<(name: string) => Promise<ConnectorsChanged>>(),
  setConnectorEnabled: vi.fn<(name: string, on: boolean) => Promise<ConnectorsChanged>>(),
  signInConnector: vi.fn<(name: string) => Promise<ConnectorsChanged>>(),
  connectorCatalog: vi.fn(),
  addCatalogConnector: vi.fn(),
  addConnectorConfig: vi.fn(),
  saveConnectorKey: vi.fn(),
  answerConnectorKey: vi.fn(),
}));

vi.mock("./api", () => api);

import { Connectors } from "./Connectors";

function row(over: Partial<ConnectorRow>): ConnectorRow {
  return {
    name: "box",
    status: "not checked",
    tools: 0,
    error: "",
    detail: "node fake-mcp.mjs",
    account: "",
    needsSignIn: false,
    canDisconnect: true,
    enabled: true,
    ...over,
  };
}

describe("Connectors", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("lists entries without starting them, then Check all connects them", async () => {
    api.listConnectors.mockResolvedValue([row({})]);
    api.checkConnectors.mockResolvedValue([row({ status: "connected", tools: 3 })]);
    render(<Connectors />);

    const box = await screen.findByRole("listitem", { name: "box" });
    expect(box).toHaveTextContent("Not checked");
    expect(box).toHaveTextContent("node fake-mcp.mjs");
    expect(api.checkConnectors).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: "Check all" }));
    expect(await within(box).findByText("Connected · 3 tools")).toBeInTheDocument();
    expect(api.checkConnectors).toHaveBeenCalledTimes(1);
  });

  it("shows a server's error and reconnects it", async () => {
    api.listConnectors.mockResolvedValue([
      row({ status: "error", error: "spawn /no/such/binary: not found" }),
    ]);
    api.restartConnector.mockResolvedValue({
      status: "Reconnected box",
      rows: [row({ status: "connected", tools: 1 })],
    });
    render(<Connectors />);

    const box = await screen.findByRole("listitem", { name: "box" });
    expect(box).toHaveTextContent("Not connected");
    expect(box).toHaveTextContent("spawn /no/such/binary: not found");

    await userEvent.click(within(box).getByRole("button", { name: "Reconnect" }));
    expect(api.restartConnector).toHaveBeenCalledWith("box");
    expect(await screen.findByRole("status")).toHaveTextContent("Reconnected box");
    expect(box).toHaveTextContent("Connected · 1 tool");
    expect(box).not.toHaveTextContent("not found");
  });

  it("disconnects only after the question is answered", async () => {
    api.listConnectors.mockResolvedValue([row({}), row({ name: "linear" })]);
    api.disconnectConnector.mockResolvedValue({
      status: "Disconnected box",
      rows: [row({ name: "linear" })],
    });
    render(<Connectors />);

    const box = await screen.findByRole("listitem", { name: "box" });
    await userEvent.click(within(box).getByRole("button", { name: "Disconnect…" }));
    expect(api.disconnectConnector).not.toHaveBeenCalled();
    const question = within(box).getByRole("group", { name: "Disconnect box?" });
    expect(question).toHaveTextContent("its sign-in and saved token are deleted");

    await userEvent.click(within(question).getByRole("button", { name: "Keep" }));
    expect(api.disconnectConnector).not.toHaveBeenCalled();
    expect(within(box).queryByRole("group")).not.toBeInTheDocument();

    await userEvent.click(within(box).getByRole("button", { name: "Disconnect…" }));
    await userEvent.click(within(box).getByRole("button", { name: "Disconnect" }));
    expect(api.disconnectConnector).toHaveBeenCalledWith("box");
    expect(await screen.findByRole("status")).toHaveTextContent("Disconnected box");
    expect(screen.queryByRole("listitem", { name: "box" })).not.toBeInTheDocument();
    expect(screen.getByRole("listitem", { name: "linear" })).toBeInTheDocument();
  });

  it("offers sign out when signed in and Sign in when it needs one", async () => {
    api.listConnectors.mockResolvedValue([
      row({ name: "linear", account: "Signed in to Linear as ada@example.com" }),
      row({ name: "notion", needsSignIn: true }),
      row({ name: "computer-use", canDisconnect: false }),
      row({ name: "off", status: "disabled", enabled: false }),
    ]);
    api.signOutConnector.mockResolvedValue({
      status: "Signed out of linear",
      rows: [row({ name: "linear", needsSignIn: true })],
    });
    render(<Connectors />);

    const notion = await screen.findByRole("listitem", { name: "notion" });
    expect(notion).toHaveTextContent("Needs a sign-in.");
    expect(within(notion).getByRole("button", { name: "Sign in" })).toBeInTheDocument();
    const desktop = screen.getByRole("listitem", { name: "computer-use" });
    expect(within(desktop).queryByRole("button", { name: "Disconnect…" })).not.toBeInTheDocument();
    const off = screen.getByRole("listitem", { name: "off" });
    expect(off).toHaveTextContent("Off");
    expect(within(off).queryByRole("button", { name: "Reconnect" })).not.toBeInTheDocument();

    const linear = screen.getByRole("listitem", { name: "linear" });
    expect(linear).toHaveTextContent("Signed in to Linear as ada@example.com");
    await userEvent.click(within(linear).getByRole("button", { name: "Sign out" }));
    expect(api.signOutConnector).toHaveBeenCalledWith("linear");
    expect(await screen.findByRole("status")).toHaveTextContent("Signed out of linear");
  });

  it("says when there are none and keeps Check all off", async () => {
    api.listConnectors.mockResolvedValue([]);
    render(<Connectors />);
    expect(await screen.findByText("No connectors yet. Add one from the catalog or paste a config.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check all" })).toBeDisabled();
  });
  it("the switch turns a connector off and Sign in waits for the browser", async () => {
    api.listConnectors.mockResolvedValue([row({}), row({ name: "notion", needsSignIn: true })]);
    api.setConnectorEnabled.mockResolvedValue({
      status: "Turned off box",
      rows: [row({ status: "disabled", enabled: false }), row({ name: "notion", needsSignIn: true })],
    });
    api.signInConnector.mockResolvedValue({
      status: "Signed in to Notion as ada@example.com",
      rows: [row({}), row({ name: "notion", account: "Signed in to Notion as ada@example.com" })],
    });
    render(<Connectors />);
    const box = await screen.findByRole("listitem", { name: "box" });
    const toggle = within(box).getByRole("switch", { name: "box on" });
    expect(toggle).toBeChecked();
    await userEvent.click(toggle);
    expect(api.setConnectorEnabled).toHaveBeenCalledWith("box", false);
    expect(await screen.findByRole("status")).toHaveTextContent("Turned off box");
    expect(within(screen.getByRole("listitem", { name: "box" })).getByRole("switch")).not.toBeChecked();

    const notion = screen.getByRole("listitem", { name: "notion" });
    await userEvent.click(within(notion).getByRole("button", { name: "Sign in" }));
    expect(api.signInConnector).toHaveBeenCalledWith("notion");
    expect(await screen.findByRole("status")).toHaveTextContent("Signed in to Notion as ada@example.com");
  });

  it("the web remote lists and checks but adds and switches nothing", async () => {
    api.listConnectors.mockResolvedValue([row({ name: "notion", needsSignIn: true })]);
    render(<Connectors remote />);
    const notion = await screen.findByRole("listitem", { name: "notion" });
    expect(screen.queryByRole("button", { name: "Add…" })).not.toBeInTheDocument();
    expect(within(notion).queryByRole("switch")).not.toBeInTheDocument();
    expect(within(notion).queryByRole("button", { name: "Sign in" })).not.toBeInTheDocument();
    expect(screen.getByText(/Add, sign in and switch connectors in Loadout on your computer/)).toBeInTheDocument();
  });
});
