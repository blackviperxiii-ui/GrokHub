import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodeSecrets, SECRETS_DELETE_NOTE, SECRETS_NONE, SECRETS_NOTE } from "./CodeSecrets";

const api = vi.hoisted(() => ({
  codeSecretList: vi.fn<() => Promise<{ names: string[] }>>(),
  codeSecretSet: vi.fn<(name: string, value: string) => Promise<{ names: string[] }>>(),
  codeSecretDelete: vi.fn<(name: string, confirmed: boolean) => Promise<{ names: string[] }>>(),
}));

vi.mock("./api", () => api);

const VALUE = "abcd1234XYZ";

describe("Settings → Code → Secrets", () => {
  beforeEach(() => {
    api.codeSecretList.mockReset();
    api.codeSecretSet.mockReset();
    api.codeSecretDelete.mockReset();
  });

  it("lists names only and says where the values live", async () => {
    api.codeSecretList.mockResolvedValue({ names: ["API_KEY", "DEPLOY_TOKEN"] });
    render(<CodeSecrets />);
    const list = await screen.findByRole("list", { name: "Secret names" });
    expect(list).toHaveTextContent("API_KEY");
    expect(list).toHaveTextContent("DEPLOY_TOKEN");
    expect(screen.getByText(SECRETS_NOTE)).toBeInTheDocument();
  });

  it("adds a secret, sends the value once, and clears it from the screen", async () => {
    api.codeSecretList.mockResolvedValue({ names: [] });
    api.codeSecretSet.mockResolvedValue({ names: ["DEPLOY_TOKEN"] });
    const { container } = render(<CodeSecrets />);
    expect(await screen.findByText(SECRETS_NONE)).toBeInTheDocument();
    const add = screen.getByRole("button", { name: "Add" });
    expect(add).toBeDisabled();
    await userEvent.type(screen.getByRole("textbox", { name: "Name" }), "DEPLOY_TOKEN");
    const value = screen.getByLabelText("Value");
    expect(value).toHaveAttribute("type", "password");
    await userEvent.type(value, VALUE);
    await userEvent.click(add);
    expect(api.codeSecretSet).toHaveBeenCalledTimes(1);
    expect(api.codeSecretSet).toHaveBeenCalledWith("DEPLOY_TOKEN", VALUE);
    expect(await screen.findByRole("list", { name: "Secret names" })).toHaveTextContent("DEPLOY_TOKEN");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Value")).toHaveValue("");
    expect(container.innerHTML).not.toContain(VALUE);
  });

  it("replaces an existing secret's value under the same name", async () => {
    api.codeSecretList.mockResolvedValue({ names: ["DEPLOY_TOKEN"] });
    api.codeSecretSet.mockResolvedValue({ names: ["DEPLOY_TOKEN"] });
    render(<CodeSecrets />);
    await userEvent.click(await screen.findByRole("button", { name: "Replace DEPLOY_TOKEN" }));
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("DEPLOY_TOKEN");
    await userEvent.type(screen.getByLabelText("Value"), "new-value-42");
    await userEvent.click(screen.getByRole("button", { name: "Replace" }));
    expect(api.codeSecretSet).toHaveBeenCalledWith("DEPLOY_TOKEN", "new-value-42");
    expect(await screen.findByText("Replaced DEPLOY_TOKEN's value.")).toBeInTheDocument();
  });

  it("asks before deleting, and Cancel deletes nothing", async () => {
    api.codeSecretList.mockResolvedValue({ names: ["DEPLOY_TOKEN"] });
    api.codeSecretDelete.mockResolvedValue({ names: [] });
    render(<CodeSecrets />);
    await userEvent.click(await screen.findByRole("button", { name: "Delete DEPLOY_TOKEN" }));
    const confirm = screen.getByRole("group", { name: "Delete DEPLOY_TOKEN?" });
    expect(confirm).toHaveTextContent(SECRETS_DELETE_NOTE);
    expect(api.codeSecretDelete).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("group", { name: "Delete DEPLOY_TOKEN?" })).not.toBeInTheDocument();
    expect(api.codeSecretDelete).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: "Delete DEPLOY_TOKEN" }));
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(api.codeSecretDelete).toHaveBeenCalledTimes(1);
    expect(api.codeSecretDelete).toHaveBeenCalledWith("DEPLOY_TOKEN", true);
    expect(await screen.findByText(SECRETS_NONE)).toBeInTheDocument();
  });

  it("shows the shell's refusal and still clears the typed value", async () => {
    api.codeSecretList.mockResolvedValue({ names: [] });
    api.codeSecretSet.mockRejectedValue("A secret needs at least 4 characters so it can be hidden in output.");
    render(<CodeSecrets />);
    await screen.findByText(SECRETS_NONE);
    await userEvent.type(screen.getByRole("textbox", { name: "Name" }), "TOKEN");
    await userEvent.type(screen.getByLabelText("Value"), "abc{Enter}");
    expect(api.codeSecretSet).toHaveBeenCalledWith("TOKEN", "abc");
    expect(await screen.findByRole("status")).toHaveTextContent(
      "A secret needs at least 4 characters so it can be hidden in output.",
    );
    expect(screen.getByLabelText("Value")).toHaveValue("");
  });

  it("moves focus to Cancel when the delete card appears", async () => {
    api.codeSecretList.mockResolvedValue({ names: ["DEPLOY_TOKEN"] });
    render(<CodeSecrets />);
    await userEvent.click(await screen.findByRole("button", { name: "Delete DEPLOY_TOKEN" }));
    expect(screen.getByRole("group", { name: "Delete DEPLOY_TOKEN?" })).toBeInTheDocument();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel" }));
    expect(api.codeSecretDelete).not.toHaveBeenCalled();
  });
});
