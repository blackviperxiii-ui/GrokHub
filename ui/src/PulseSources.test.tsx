import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PulseSourcesView } from "./api";

const api = vi.hoisted(() => ({
  getPulseSources: vi.fn<() => Promise<PulseSourcesView>>(),
  savePulseSources:
    vi.fn<(view: PulseSourcesView) => Promise<PulseSourcesView>>(),
}));

vi.mock("./api", async (importOriginal) => {
  const original = await importOriginal<typeof import("./api")>();
  return { ...original, getPulseSources: api.getPulseSources, savePulseSources: api.savePulseSources };
});

import { PulseSources, SOURCES_NOTE } from "./PulseSources";

function view(over: Partial<PulseSourcesView> = {}): PulseSourcesView {
  return {
    dailyDrop: {
      enabled: true,
      hackerNews: true,
      reddit: false,
      webSearch: false,
      github: false,
      youtube: false,
      rss: false,
    },
    askOffersEnabled: true,
    hobbyPaths: { tips: true, projects: false },
    releaseRadarOn: true,
    ...over,
  };
}

function row(name: string) {
  const list = within(screen.getByRole("list", { name: "Source switches" }));
  return within(list.getByRole("listitem", { name }));
}

describe("PulseSources", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.savePulseSources.mockImplementation((v) => Promise.resolve(v));
  });

  it("renders the cabin's rows and toggles save", async () => {
    api.getPulseSources.mockResolvedValue(view());
    render(<PulseSources />);
    expect(await screen.findByText(SOURCES_NOTE)).toBeVisible();

    const radar = row("Release radar").getByRole("switch");
    expect(radar).toHaveAttribute("aria-checked", "true");
    await userEvent.click(radar);
    expect(api.savePulseSources).toHaveBeenCalledWith(
      expect.objectContaining({ releaseRadarOn: false }),
    );
    expect(row("Release radar").getByRole("switch")).toHaveAttribute(
      "aria-checked",
      "false",
    );

    const offers = row("Automate offers").getByRole("switch");
    await userEvent.click(offers);
    expect(api.savePulseSources).toHaveBeenLastCalledWith(
      expect.objectContaining({ askOffersEnabled: false }),
    );
  });

  it("shows an off hobby projects row as off", async () => {
    api.getPulseSources.mockResolvedValue(view());
    render(<PulseSources />);
    await screen.findByText(SOURCES_NOTE);
    expect(row("Hobby projects").getByRole("switch")).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(row("Hobby tips").getByRole("switch")).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });
});
