import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WebAccess as Shape } from "./api";

const api = vi.hoisted(() => ({
  getWebAccess: vi.fn<() => Promise<Shape>>(),
  setWebAccess: vi.fn<(on: boolean) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import { WEB_NOTE, WebAccess } from "./WebAccess";

const OFF: Shape = { on: false, code: "", links: [] };
const ON: Shape = {
  on: true,
  code: "0123456789abcdef0123456789abcdef0123456789abcdef",
  links: [
    "http://192.168.1.20:4620/#code=0123456789abcdef0123456789abcdef0123456789abcdef",
    "http://127.0.0.1:4620/#code=0123456789abcdef0123456789abcdef0123456789abcdef",
  ],
};

describe("WebAccess", () => {
  beforeEach(() => {
    api.getWebAccess.mockReset();
    api.setWebAccess.mockReset();
  });

  it("turns web access on, shows the pairing links and code, and turns it off", async () => {
    api.getWebAccess.mockResolvedValue(OFF);
    api.setWebAccess.mockImplementation(async (on) => (on ? ON : OFF));
    render(<WebAccess />);
    expect(screen.getByText(WEB_NOTE)).toBeInTheDocument();

    await userEvent.click(await screen.findByRole("button", { name: "Turn on web access" }));
    expect(api.setWebAccess).toHaveBeenLastCalledWith(true);
    expect(await screen.findByText(ON.links[0])).toBeInTheDocument();
    expect(screen.getByText(ON.links[1])).toBeInTheDocument();
    expect(screen.getByLabelText("Pairing code")).toHaveTextContent(ON.code);

    await userEvent.click(screen.getByRole("button", { name: "Turn off web access" }));
    expect(api.setWebAccess).toHaveBeenLastCalledWith(false);
    expect(await screen.findByRole("button", { name: "Turn on web access" })).toBeInTheDocument();
    expect(screen.queryByText(ON.links[0])).toBeNull();
  });

  it("says why web access didn't start", async () => {
    api.getWebAccess.mockResolvedValue(OFF);
    api.setWebAccess.mockRejectedValue("Web access couldn't start on port 4620: address in use");
    render(<WebAccess />);
    await userEvent.click(await screen.findByRole("button", { name: "Turn on web access" }));
    expect(
      await screen.findByText("Web access couldn't start on port 4620: address in use"),
    ).toBeInTheDocument();
  });
});
