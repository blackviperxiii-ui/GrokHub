import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CatalogRow, ConnectorRow, ConnectorsChanged } from "./api";

const api = vi.hoisted(() => ({
  connectorCatalog: vi.fn(),
  addCatalogConnector: vi.fn<(id: string, includeTools?: string[] | null) => Promise<ConnectorsChanged>>(),
  addConnectorConfig: vi.fn<(text: string) => Promise<ConnectorsChanged>>(),
  saveConnectorKey: vi.fn(),
  answerConnectorKey: vi.fn<(id: string, approve: boolean) => Promise<ConnectorsChanged>>(),
}));

vi.mock("./api", () => api);

import { ConnectorAdd } from "./ConnectorAdd";

function entry(over: Partial<CatalogRow>): CatalogRow {
  return {
    id: "brave-search",
    name: "Brave Search",
    publisher: "Brave",
    category: "Search",
    description: "Web search",
    auth: "key",
    keyHint: "Get one at the Brave dashboard.",
    target: "npx brave",
    installed: false,
    source: "https://github.com/brave/brave-search-mcp-server",
    permissions: "Sends search queries to Brave with your API key.",
    tools: [],
    ...over,
  };
}

function row(name: string): ConnectorRow {
  return {
    name,
    status: "not checked",
    tools: 0,
    error: "",
    detail: "",
    account: "",
    needsSignIn: false,
    canDisconnect: true,
    enabled: true,
    toolRows: [],
  };
}

describe("ConnectorAdd", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("searches the catalog, picks tools, adds one and saves its key after the card", async () => {
    api.connectorCatalog.mockResolvedValue({
      rows: [
        entry({}),
        entry({
          id: "deepwiki",
          name: "DeepWiki",
          description: "Repo wiki",
          auth: "none",
          tools: ["ask_wiki_question", "read_wiki_contents", "read_wiki_structure"],
          source: "https://docs.devin.ai/work-with-devin/deepwiki-mcp",
          permissions: "Sends public GitHub repo questions to DeepWiki.",
        }),
      ],
      note: null,
    });
    api.addCatalogConnector.mockResolvedValue({
      status: "Added Brave Search. Paste its key to finish",
      rows: [row("brave-search")],
    });
    api.saveConnectorKey.mockResolvedValue({
      id: "mcp-key-brave-search-1",
      name: "brave-search",
      action: "Save its API key (brave-search)",
    });
    api.answerConnectorKey.mockResolvedValue({
      status: "Nothing saved. brave-search's key was denied.",
      rows: [row("brave-search")],
    });
    const onChanged = vi.fn();
    render(<ConnectorAdd onChanged={onChanged} onClose={() => {}} />);
    expect(api.connectorCatalog).toHaveBeenCalledWith(false);
    await screen.findByRole("listitem", { name: "Brave Search" });
    await userEvent.type(screen.getByRole("textbox", { name: "Search connectors" }), "wiki");
    expect(screen.queryByRole("listitem", { name: "Brave Search" })).not.toBeInTheDocument();
    await userEvent.clear(screen.getByRole("textbox", { name: "Search connectors" }));

    const brave = screen.getByRole("listitem", { name: "Brave Search" });
    expect(brave).toHaveTextContent("Needs an API key");
    await userEvent.click(within(brave).getByRole("button", { name: "Add" }));
    const picker = await screen.findByRole("dialog", { name: "Pick tools for Brave Search" });
    expect(picker).toHaveTextContent("Tools load after connect");
    await userEvent.click(within(picker).getByRole("button", { name: "Add with selected tools" }));
    expect(api.addCatalogConnector).toHaveBeenCalledWith("brave-search", null);
    expect(onChanged).toHaveBeenCalled();
    const field = await within(brave).findByLabelText("Brave Search API key");
    expect(field).toHaveAttribute("type", "password");
    await userEvent.type(field, "sk-test-key");
    await userEvent.click(within(brave).getByRole("button", { name: "Save key" }));
    expect(api.saveConnectorKey).toHaveBeenCalledWith("brave-search", "sk-test-key");
    expect(field).toHaveValue("");

    const card = await screen.findByRole("group", { name: "Save the brave-search key?" });
    expect(api.answerConnectorKey).not.toHaveBeenCalled();
    await userEvent.click(within(card).getByRole("button", { name: "Deny" }));
    expect(api.answerConnectorKey).toHaveBeenCalledWith("mcp-key-brave-search-1", false);
    expect(await screen.findByRole("status")).toHaveTextContent("brave-search's key was denied");
  });

  it("writes an include list when a subset of tools is picked", async () => {
    api.connectorCatalog.mockResolvedValue({
      rows: [
        entry({
          id: "deepwiki",
          name: "DeepWiki",
          auth: "none",
          tools: ["ask_wiki_question", "read_wiki_contents", "read_wiki_structure"],
          source: "https://docs.devin.ai/work-with-devin/deepwiki-mcp",
          permissions: "Sends public GitHub repo questions to DeepWiki.",
        }),
      ],
      note: null,
    });
    api.addCatalogConnector.mockResolvedValue({
      status: "Added DeepWiki",
      rows: [row("deepwiki")],
    });
    render(<ConnectorAdd onChanged={() => {}} onClose={() => {}} />);
    const deep = await screen.findByRole("listitem", { name: "DeepWiki" });
    await userEvent.click(within(deep).getByRole("button", { name: "Add" }));
    const picker = await screen.findByRole("dialog", { name: "Pick tools for DeepWiki" });
    await userEvent.click(within(picker).getByRole("switch", { name: "DeepWiki read_wiki_contents on" }));
    await userEvent.click(within(picker).getByRole("button", { name: "Add with selected tools" }));
    expect(api.addCatalogConnector).toHaveBeenCalledWith("deepwiki", [
      "ask_wiki_question",
      "read_wiki_structure",
    ]);
  });

  it("refreshes only on click and adds from a pasted config", async () => {
    api.connectorCatalog.mockResolvedValue({ rows: [], note: null });
    api.addConnectorConfig.mockResolvedValue({ status: "Imported one, two", rows: [row("one"), row("two")] });
    render(<ConnectorAdd onChanged={() => {}} onClose={() => {}} />);
    expect(api.connectorCatalog).toHaveBeenCalledTimes(1);
    api.connectorCatalog.mockResolvedValue({ rows: [], note: "Updated 2 connectors from the official registry." });
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(api.connectorCatalog).toHaveBeenLastCalledWith(true);
    expect(await screen.findByRole("status")).toHaveTextContent("Updated 2 connectors");

    await userEvent.click(screen.getByRole("tab", { name: "Paste config" }));
    const box = screen.getByRole("textbox", { name: "mcpServers config" });
    await userEvent.click(box);
    await userEvent.paste('{"mcpServers":{"one":{"command":"echo"},"two":{"command":"echo"}}}');
    await userEvent.click(screen.getByRole("button", { name: "Add from config" }));
    expect(api.addConnectorConfig).toHaveBeenCalledWith(
      '{"mcpServers":{"one":{"command":"echo"},"two":{"command":"echo"}}}',
    );
    expect(await screen.findByRole("status")).toHaveTextContent("Imported one, two");
    expect(box).toHaveValue("");
  });
});
