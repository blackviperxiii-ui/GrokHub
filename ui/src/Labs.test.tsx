import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginAct, PluginMarket, PluginPage, PluginRow } from "./api";

const api = vi.hoisted(() => ({
  getPlugins: vi.fn<() => Promise<PluginPage>>(),
  pluginAct: vi.fn<(name: string, act: PluginAct) => Promise<PluginPage>>(),
  installPlugin: vi.fn<(source: string) => Promise<PluginPage>>(),
  fetchPluginMarket: vi.fn<(url: string) => Promise<PluginMarket>>(),
}));

vi.mock("./api", () => api);

import { CHANNEL_NOTE, Labs, PLUGINS_EMPTY, PLUGINS_TIP } from "./Labs";

function row(over: Partial<PluginRow> = {}): PluginRow {
  return {
    name: "ship-kit",
    version: "1.2.0",
    summary: "ship-kit 1.2.0 · skills: skills/ship",
    trusted: false,
    enabled: false,
    installed: true,
    ...over,
  };
}

function page(rows: PluginRow[], note = "", marketUrl = ""): PluginPage {
  return { rows, note, marketUrl };
}

function card(name: string) {
  const list = within(screen.getByRole("list", { name: "Installed plugins" }));
  return within(list.getByRole("listitem", { name }));
}

describe("Labs", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("labels plugins a preview, points to the cabin for the channel, and says what to do when empty", async () => {
    api.getPlugins.mockResolvedValue(page([]));
    render(<Labs />);
    expect(await screen.findByText(PLUGINS_EMPTY)).toBeVisible();
    expect(screen.getByText(CHANNEL_NOTE)).toBeVisible();
    expect(
      screen.getByRole("heading", { name: "Plugins Preview" }),
    ).toBeVisible();
    expect(screen.getByText("Preview")).toHaveAttribute("title", PLUGINS_TIP);
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
    render(<Labs />);

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
    render(<Labs />);
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
    });
    api.installPlugin.mockResolvedValue(
      page([row()], "Installed ship-kit. It stays off until you trust it."),
    );
    render(<Labs />);
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
    await userEvent.click(market.getByRole("button", { name: "Install" }));
    expect(api.installPlugin).toHaveBeenCalledWith(
      "https://example.com/ship-kit.git",
    );
    expect(card("ship-kit").getByText("Not trusted · off")).toBeVisible();
  });
});
