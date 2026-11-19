import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BrowserAttach as Shape } from "./browserAttachApi";

const api = vi.hoisted(() => ({
  getBrowserAttach: vi.fn<() => Promise<Shape | null>>(),
  saveBrowserAttach: vi.fn<(endpoint: string, sites: string[]) => Promise<Shape | null>>(),
}));
vi.mock("./browserAttachApi", () => api);

import { BrowserAttach } from "./BrowserAttach";

const fresh: Shape = { endpoint: "http://127.0.0.1:9222", origins: [], active: false, problem: null };

describe("Your browser setting", () => {
  beforeEach(() => {
    api.getBrowserAttach.mockReset();
    api.saveBrowserAttach.mockReset();
  });

  it("stays hidden in the web app", async () => {
    api.getBrowserAttach.mockResolvedValue(null);
    const { container } = render(<BrowserAttach />);
    await waitFor(() => expect(api.getBrowserAttach).toHaveBeenCalledTimes(1));
    expect(container.textContent).toBe("");
  });

  it("keeps its own browser until a site is allowed, then works only there", async () => {
    api.getBrowserAttach.mockResolvedValue(fresh);
    api.saveBrowserAttach.mockResolvedValue({ ...fresh, origins: ["https://github.com"], active: true });
    render(<BrowserAttach />);
    expect(await screen.findByText("The browser tool uses its own browser.")).toBeTruthy();
    await userEvent.type(screen.getByLabelText("Allow a site"), "https://github.com/user/my-app");
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(api.saveBrowserAttach).toHaveBeenCalledWith("http://127.0.0.1:9222", ["https://github.com/user/my-app"]);
    expect(await screen.findByText("The browser tool works in your browser, on this site only.")).toBeTruthy();
    expect(screen.getByText("https://github.com")).toBeTruthy();
    expect((screen.getByLabelText("Allow a site") as HTMLInputElement).value).toBe("");
  });

  it("removes a site and says why a site was refused", async () => {
    api.getBrowserAttach.mockResolvedValue({
      ...fresh,
      origins: ["https://github.com", "https://docs.rs"],
      active: true,
    });
    api.saveBrowserAttach.mockResolvedValueOnce({ ...fresh, origins: ["https://docs.rs"], active: true });
    render(<BrowserAttach />);
    await userEvent.click(await screen.findByRole("button", { name: "Remove https://github.com" }));
    expect(api.saveBrowserAttach).toHaveBeenLastCalledWith("http://127.0.0.1:9222", ["https://docs.rs"]);
    await waitFor(() => expect(screen.queryByText("https://github.com")).toBeNull());

    api.saveBrowserAttach.mockRejectedValueOnce('"file:///etc/passwd" isn\'t an http(s) site');
    await userEvent.type(screen.getByLabelText("Allow a site"), "file:///etc/passwd");
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(await screen.findByText('"file:///etc/passwd" isn\'t an http(s) site')).toBeTruthy();
    expect((screen.getByLabelText("Allow a site") as HTMLInputElement).value).toBe("file:///etc/passwd");
  });
});
