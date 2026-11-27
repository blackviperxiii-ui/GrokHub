import "@testing-library/jest-dom/vitest";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginAct, PluginEntry, PluginMarket, PluginPage, PluginRow } from "./api";

const api = vi.hoisted(() => ({
  getPlugins: vi.fn<() => Promise<PluginPage>>(),
  pluginAct: vi.fn<(name: string, act: PluginAct) => Promise<PluginPage>>(),
  installPlugin: vi.fn<(source: string) => Promise<PluginPage>>(),
  fetchPluginMarket: vi.fn<(url: string) => Promise<PluginMarket>>(),
}));

vi.mock("./api", () => api);
// The Skills and Connectors tabs are those pages' own lists, tested there.
vi.mock("./Skills", () => ({
  Skills: ({ onOpenConnectors }: { onOpenConnectors?: () => void }) => (
    <div>
      Skills list
      <button type="button" onClick={onOpenConnectors}>
        Skill connectors
      </button>
    </div>
  ),
}));
vi.mock("./Connectors", () => ({ Connectors: () => <div>Connectors list</div> }));

import {
  MARKET_EMPTY,
  MARKET_NONE_MATCHED,
  Marketplace,
  PLUGINS_EMPTY,
  PLUGINS_MCP_NOTE,
  PLUGINS_TIP,
  PluginsTab,
  UPDATE_HINT,
  compareVersions,
  entryStatus,
  filterEntries,
} from "./Marketplace";

function row(over: Partial<PluginRow> = {}): PluginRow {
  return {
    name: "ship-kit",
    version: "1.2.0",
    summary: "ship-kit 1.2.0\nskills: skills/ship",
    description: "",
    parts: [{ label: "Skills", text: "skills/ship", warn: false }],
    trusted: false,
    enabled: false,
    installed: true,
    ...over,
  };
}

function page(rows: PluginRow[], note = "", marketUrl = ""): PluginPage {
  return { rows, note, marketUrl };
}

function entry(over: Partial<PluginEntry> = {}): PluginEntry {
  return {
    name: "ship-kit",
    version: "1.2.0",
    description: "Ship helpers",
    source: "https://example.com/ship-kit.git",
    ...over,
  };
}

function card(name: string) {
  const list = within(screen.getByRole("list", { name: "Installed plugins" }));
  return within(list.getByRole("listitem", { name }));
}

function marketCard(name: string) {
  const list = within(screen.getByRole("list", { name: "Marketplace" }));
  return within(list.getByRole("listitem", { name }));
}

beforeEach(() => {
  for (const fn of Object.values(api)) fn.mockReset();
});

describe("Marketplace", () => {
  it("opens on Plugins and moves between tabs by click, arrow key and the MCP note", async () => {
    api.getPlugins.mockResolvedValue(page([]));
    render(<Marketplace onUse={vi.fn()} />);
    expect(screen.getByRole("heading", { name: "Marketplace" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Plugins" })).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByText(PLUGINS_EMPTY)).toBeVisible();

    await userEvent.click(screen.getByRole("tab", { name: "Skills" }));
    expect(screen.getByText("Skills list")).toBeVisible();
    expect(screen.queryByText(PLUGINS_EMPTY)).toBeNull();

    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Connectors" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Connectors" })).toHaveFocus();
    expect(screen.getByText("Connectors list")).toBeVisible();

    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Plugins" })).toHaveAttribute("aria-selected", "true");
    await screen.findByText(PLUGINS_EMPTY);
    await userEvent.click(screen.getByRole("button", { name: "Connectors" }));
    expect(screen.getByText("Connectors list")).toBeVisible();

    await userEvent.click(screen.getByRole("tab", { name: "Skills" }));
    await userEvent.click(screen.getByRole("button", { name: "Skill connectors" }));
    expect(screen.getByText("Connectors list")).toBeVisible();
  });
});

describe("Marketplace plugins", () => {
  it("marks what is installed or newer, and offers Install only for new entries", async () => {
    api.getPlugins.mockResolvedValue(
      page([row(), row({ name: "home-kit", version: "1.9.0" })], "", "https://example.com/index.json"),
    );
    api.fetchPluginMarket.mockResolvedValue({
      entries: [
        entry(),
        entry({ name: "home-kit", version: "1.10.0", source: "https://example.com/home-kit.git" }),
        entry({
          name: "deploy-kit",
          version: "0.1.0",
          description: "Deploy steps",
          source: "https://example.com/deploy-kit.git",
          author: "Ana",
          category: "Ops",
          keywords: ["release"],
        }),
      ],
      page: page([row(), row({ name: "home-kit", version: "1.9.0" })], "3 entries. Nothing was installed.", "https://example.com/index.json"),
      read: true,
    });
    render(<PluginsTab />);
    await screen.findByRole("list", { name: "Installed plugins" });
    await userEvent.click(screen.getByRole("button", { name: "Fetch list" }));

    expect(marketCard("ship-kit").getByText("Installed")).toBeVisible();
    expect(marketCard("ship-kit").queryByRole("button")).toBeNull();
    expect(marketCard("home-kit").getByText("Update")).toBeVisible();
    expect(marketCard("home-kit").getByText(UPDATE_HINT)).toBeVisible();
    expect(marketCard("home-kit").queryByRole("button")).toBeNull();
    expect(marketCard("deploy-kit").getByText("by Ana")).toBeVisible();
    expect(marketCard("deploy-kit").getByText("Ops")).toBeVisible();
    expect(marketCard("deploy-kit").getByRole("button", { name: "Install deploy-kit" })).toBeEnabled();
  });

  it("filters the index by search words and by category", async () => {
    api.getPlugins.mockResolvedValue(page([], "", "https://example.com/index.json"));
    api.fetchPluginMarket.mockResolvedValue({
      entries: [
        entry({ category: "Dev" }),
        entry({
          name: "deploy-kit",
          description: "Deploy steps",
          source: "https://example.com/deploy-kit.git",
          category: "Ops",
          keywords: ["release"],
        }),
      ],
      page: page([], "2 entries. Nothing was installed.", "https://example.com/index.json"),
      read: true,
    });
    render(<PluginsTab />);
    await screen.findByText(PLUGINS_EMPTY);
    await userEvent.click(screen.getByRole("button", { name: "Fetch list" }));
    const names = () =>
      within(screen.getByRole("list", { name: "Marketplace" }))
        .getAllByRole("listitem")
        .map((li) => li.getAttribute("aria-label"));
    expect(names()).toEqual(["ship-kit", "deploy-kit"]);

    const search = screen.getByRole("searchbox", { name: "Search the index" });
    await userEvent.type(search, "release");
    expect(names()).toEqual(["deploy-kit"]);
    await userEvent.clear(search);

    await userEvent.selectOptions(screen.getByLabelText("Category"), "Dev");
    expect(names()).toEqual(["ship-kit"]);

    await userEvent.type(search, "deploy");
    expect(screen.getByText(MARKET_NONE_MATCHED)).toBeVisible();
    expect(screen.queryByRole("list", { name: "Marketplace" })).toBeNull();
    expect(screen.queryByText(MARKET_EMPTY)).toBeNull();
  });

  it("labels plugins a preview and says what to do when empty", async () => {
    api.getPlugins.mockResolvedValue(page([]));
    render(<PluginsTab />);
    expect(await screen.findByText(PLUGINS_EMPTY)).toBeVisible();
    expect(screen.queryByText(/cabin/)).toBeNull();
    expect(
      screen.getByRole("heading", { name: "Plugins Preview" }),
    ).toBeVisible();
    expect(screen.getByText("Preview")).toHaveAttribute(
      "title",
      "A preview: plugins work, but how they install and trust may change.",
    );
    expect(PLUGINS_TIP).not.toMatch(/cabin/i);
    expect(screen.getByText(PLUGINS_MCP_NOTE)).toBeVisible();
    expect(
      screen.getByText("Nothing from the bundle runs. It installs off and untrusted."),
    ).toBeVisible();
    expect(
      screen.getByText("An https index URL. Fetch only lists entries; Install is a separate step."),
    ).toBeVisible();
    expect(screen.queryByText(MARKET_EMPTY)).toBeNull();
  });

  it("lists a bundle's parts under labels, shows its description once, and flags a refused part", async () => {
    api.getPlugins.mockResolvedValue(
      page([
        row({
          description: "Ship helpers",
          parts: [
            { label: "Skills", text: "skills/ship", warn: false },
            { label: "Refused", text: "hooks: path escapes the bundle", warn: true },
          ],
        }),
      ]),
    );
    render(<PluginsTab />);
    await screen.findByRole("list", { name: "Installed plugins" });
    expect(card("ship-kit").getByText("Ship helpers")).toBeVisible();
    // Its own line breaks show (white-space: pre-wrap).
    expect(card("ship-kit").getByText("Ship helpers")).toHaveClass("plugin-desc");
    expect(card("ship-kit").getAllByText("ship-kit 1.2.0")).toHaveLength(1);
    const terms = card("ship-kit").getAllByRole("term").map((t) => t.textContent);
    const defs = card("ship-kit").getAllByRole("definition").map((d) => d.textContent);
    expect(terms).toEqual(["Skills", "Refused"]);
    expect(defs).toEqual(["skills/ship", "hooks: path escapes the bundle"]);
    expect(card("ship-kit").getByText("hooks: path escapes the bundle")).toHaveClass("warn");
    expect(card("ship-kit").getByText("skills/ship")).not.toHaveClass("warn");
    expect(card("ship-kit").getByText("Not trusted · off · has refused parts")).toBeVisible();
    expect(card("ship-kit").getByText("Remove the installed copy.")).toBeVisible();
  });

  it("says when a bundle can't be trusted, and shows an unlabelled summary whole", async () => {
    api.getPlugins.mockResolvedValue(
      page([
        row({ parts: [{ label: "Can't be trusted", text: "hash mismatch", warn: true }] }),
        row({
          name: "odd-kit",
          installed: false,
          parts: [{ label: "", text: "odd-kit 9\nfree text", warn: false }],
        }),
      ]),
    );
    render(<PluginsTab />);
    await screen.findByRole("list", { name: "Installed plugins" });
    expect(card("ship-kit").getByText("Not trusted · off · can't be trusted")).toBeVisible();
    // No list of terms without a term: the whole summary is a paragraph.
    expect(card("odd-kit").queryByRole("term")).toBeNull();
    expect(card("odd-kit").queryByRole("definition")).toBeNull();
    const whole = card("odd-kit").getByText(/^odd-kit 9/);
    expect(whole.textContent).toBe("odd-kit 9\nfree text");
    expect(whole).toHaveClass("plugin-whole");
    expect(card("odd-kit").queryByText("Remove the installed copy.")).toBeNull();
  });

  it("opens the Connectors tab from the MCP note", async () => {
    api.getPlugins.mockResolvedValue(page([]));
    const open = vi.fn();
    render(<PluginsTab onOpenConnectors={open} />);
    await screen.findByText(PLUGINS_EMPTY);
    await userEvent.click(screen.getByRole("button", { name: "Connectors" }));
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("says the index has no entries only after a fetch that found none", async () => {
    api.getPlugins.mockResolvedValue(page([], "", "https://example.com/index.json"));
    api.fetchPluginMarket.mockResolvedValue({
      entries: [],
      page: page([], "0 entries. Nothing was installed.", "https://example.com/index.json"),
      read: true,
    });
    render(<PluginsTab />);
    await screen.findByText(PLUGINS_EMPTY);
    expect(screen.queryByText(MARKET_EMPTY)).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Fetch list" }));
    expect(await screen.findByText("No marketplace entries.")).toBeVisible();
  });

  it("a fetch that fails says why and never calls the index empty", async () => {
    api.getPlugins.mockResolvedValue(page([], "", "https://example.com/index.json"));
    api.fetchPluginMarket.mockResolvedValue({
      entries: [],
      page: page([], "marketplace fetch failed: 404", "https://example.com/index.json"),
      read: false,
    });
    render(<PluginsTab />);
    await screen.findByText(PLUGINS_EMPTY);
    await userEvent.click(screen.getByRole("button", { name: "Fetch list" }));
    expect(await screen.findByText("marketplace fetch failed: 404")).toBeVisible();
    expect(screen.queryByText(MARKET_EMPTY)).toBeNull();
  });

  it("a fetch that is refused after an empty one drops the empty note", async () => {
    api.getPlugins.mockResolvedValue(page([], "", "https://example.com/index.json"));
    api.fetchPluginMarket
      .mockResolvedValueOnce({
        entries: [],
        page: page([], "0 entries. Nothing was installed.", "https://example.com/index.json"),
        read: true,
      })
      .mockRejectedValueOnce(new Error("Can't reach your computer."));
    render(<PluginsTab />);
    await screen.findByText(PLUGINS_EMPTY);
    await userEvent.click(screen.getByRole("button", { name: "Fetch list" }));
    expect(await screen.findByText(MARKET_EMPTY)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Fetch list" }));
    expect(await screen.findByText(/Can't reach your computer\./)).toBeVisible();
    expect(screen.queryByText(MARKET_EMPTY)).toBeNull();
  });

  it("keeps the list up while a refetch runs and drops it when that fetch fails", async () => {
    api.getPlugins.mockResolvedValue(page([], "", "https://example.com/index.json"));
    let refuse: (err: Error) => void = () => {};
    api.fetchPluginMarket
      .mockResolvedValueOnce({
        entries: [
          {
            name: "ship-kit",
            version: "1.2.0",
            description: "Ship helpers",
            source: "https://example.com/ship-kit.git",
          },
        ],
        page: page([], "1 entries. Nothing was installed.", "https://example.com/index.json"),
        read: true,
      })
      .mockReturnValueOnce(new Promise((_, reject) => (refuse = reject)));
    render(<PluginsTab />);
    await screen.findByText(PLUGINS_EMPTY);
    const fetchList = screen.getByRole("button", { name: "Fetch list" });
    await userEvent.click(fetchList);
    expect(await screen.findByText("Ship helpers")).toBeVisible();
    await userEvent.click(fetchList);
    expect(api.fetchPluginMarket).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("list", { name: "Marketplace" })).toBeVisible();
    await act(async () => refuse(new Error("Can't reach your computer.")));
    expect(await screen.findByText(/Can't reach your computer\./)).toBeVisible();
    expect(screen.queryByRole("list", { name: "Marketplace" })).toBeNull();
    expect(screen.queryByText(MARKET_EMPTY)).toBeNull();
  });

  it("installs a bundle off, then trusts it, turns it on and off, and removes it", async () => {
    api.getPlugins.mockResolvedValue(page([]));
    api.installPlugin.mockResolvedValue(
      page([row()], "Installed ship-kit. It stays off until you trust it."),
    );
    api.pluginAct
      .mockResolvedValueOnce(
        page(
          [row({ trusted: true })],
          "Trusted ship-kit. It stays off until you enable it.",
        ),
      )
      .mockResolvedValueOnce(
        page([row({ trusted: true, enabled: true })], "Enabled ship-kit."),
      )
      .mockResolvedValueOnce(
        page([row({ trusted: true })], "Disabled ship-kit."),
      )
      .mockResolvedValueOnce(page([], "Removed ship-kit."));
    render(<PluginsTab />);

    const box = await screen.findByLabelText("Install a bundle");
    await userEvent.type(box, "https://example.com/ship-kit.git{Enter}");
    expect(api.installPlugin).toHaveBeenCalledWith(
      "https://example.com/ship-kit.git",
    );
    expect(box).toHaveValue("");
    expect(screen.getByRole("status")).toHaveTextContent(
      "Installed ship-kit. It stays off until you trust it.",
    );
    expect(card("ship-kit").getByText("Not trusted · off")).toBeVisible();
    expect(card("ship-kit").getByText("ship-kit 1.2.0")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Trust" }));
    expect(api.pluginAct).toHaveBeenLastCalledWith("ship-kit", "trust");
    expect(card("ship-kit").getByText("Trusted · off")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Turn on" }));
    expect(api.pluginAct).toHaveBeenLastCalledWith("ship-kit", "enable");
    expect(card("ship-kit").getByText("Trusted · on")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Turn off" }));
    expect(api.pluginAct).toHaveBeenLastCalledWith("ship-kit", "disable");
    expect(screen.getByRole("status")).toHaveTextContent("Disabled ship-kit.");

    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(api.pluginAct).toHaveBeenLastCalledWith("ship-kit", "remove");
    expect(screen.getByText(PLUGINS_EMPTY)).toBeVisible();
  });

  it("keeps the typed source when the install fails, and offers no Remove for a found bundle", async () => {
    api.getPlugins.mockResolvedValue(
      page([row({ name: "home-kit", installed: false })]),
    );
    api.installPlugin.mockResolvedValue(
      page(
        [row({ name: "home-kit", installed: false })],
        "marketplace entry is not an https, ssh, or file URL, or an absolute path",
      ),
    );
    render(<PluginsTab />);
    expect(await screen.findByText("home-kit 1.2.0")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();

    const box = screen.getByLabelText("Install a bundle");
    await userEvent.type(box, "relative/path{Enter}");
    expect(box).toHaveValue("relative/path");
    expect(screen.getByRole("status")).toHaveTextContent(
      "marketplace entry is not an https, ssh, or file URL, or an absolute path",
    );
  });

  it("fetches a marketplace list and installs an entry from it", async () => {
    api.getPlugins.mockResolvedValue(
      page([], "", "https://example.com/index.json"),
    );
    api.fetchPluginMarket.mockResolvedValue({
      entries: [
        {
          name: "ship-kit",
          version: "1.2.0",
          description: "Ship helpers",
          source: "https://example.com/ship-kit.git",
        },
      ],
      page: page(
        [],
        "1 entries. Nothing was installed.",
        "https://example.com/index.json",
      ),
      read: true,
    });
    api.installPlugin.mockResolvedValue(
      page([row()], "Installed ship-kit. It stays off until you trust it."),
    );
    render(<PluginsTab />);
    expect(await screen.findByLabelText("Marketplace index")).toHaveValue(
      "https://example.com/index.json",
    );
    await userEvent.click(screen.getByRole("button", { name: "Fetch list" }));
    expect(api.fetchPluginMarket).toHaveBeenCalledWith(
      "https://example.com/index.json",
    );
    expect(api.installPlugin).not.toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent(
      "1 entries. Nothing was installed.",
    );

    const market = within(screen.getByRole("list", { name: "Marketplace" }));
    expect(market.getByText("Ship helpers")).toBeVisible();
    await userEvent.click(market.getByRole("button", { name: "Install ship-kit" }));
    expect(api.installPlugin).toHaveBeenCalledWith(
      "https://example.com/ship-kit.git",
    );
    expect(card("ship-kit").getByText("Not trusted · off")).toBeVisible();
  });
});

describe("index helpers", () => {
  it("compares dotted versions as numbers", () => {
    expect(compareVersions("1.10.0", "1.9.0")).toBe(1);
    expect(compareVersions("v2.0", "2.0.0")).toBe(0);
    expect(compareVersions("1.2", "1.2.1")).toBe(-1);
  });

  it("says whether an entry is new, installed or an update", () => {
    const rows = [row({ version: "1.2.0" })];
    expect(entryStatus(entry({ name: "other" }), rows)).toBe("new");
    expect(entryStatus(entry({ version: "1.2.0" }), rows)).toBe("installed");
    expect(entryStatus(entry({ version: "1.1.0" }), rows)).toBe("installed");
    expect(entryStatus(entry({ version: "1.3.0" }), rows)).toBe("update");
  });

  it("matches every search word across name, description, author, category and keywords", () => {
    const all = [entry({ author: "Ana", keywords: ["release"] }), entry({ name: "deploy-kit", category: "Ops" })];
    expect(filterEntries(all, "ana release", "").map((e) => e.name)).toEqual(["ship-kit"]);
    expect(filterEntries(all, "", "Ops").map((e) => e.name)).toEqual(["deploy-kit"]);
    expect(filterEntries(all, "  ", "").length).toBe(2);
  });
});
