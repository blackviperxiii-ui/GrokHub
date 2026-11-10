import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CODE_SETTINGS_NONE, CodeSettings } from "./CodeSettings";

const api = vi.hoisted(() => ({
  codeTestRemembered: vi.fn<() => Promise<{ command: string | null }>>(),
  codeTestForget: vi.fn<() => Promise<{ command: string | null }>>(),
}));

vi.mock("./api", () => api);

describe("Settings → Code", () => {
  beforeEach(() => {
    api.codeTestRemembered.mockReset();
    api.codeTestForget.mockReset();
  });

  it("shows the remembered command and forgets it when the toggle is turned off", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: "cargo test" });
    api.codeTestForget.mockResolvedValue({ command: null });
    render(<CodeSettings />);
    expect(await screen.findByText(/Remembered command/)).toHaveTextContent("cargo test");
    const toggle = screen.getByRole("switch", { name: "Remember this project's test command" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await userEvent.click(toggle);
    expect(api.codeTestForget).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(CODE_SETTINGS_NONE)).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Remember this project's test command" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });

  it("leaves the forget toggle off and disabled when nothing is remembered", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    render(<CodeSettings />);
    expect(await screen.findByText(CODE_SETTINGS_NONE)).toBeInTheDocument();
    const toggle = screen.getByRole("switch", { name: "Remember this project's test command" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle).toBeDisabled();
  });
});
