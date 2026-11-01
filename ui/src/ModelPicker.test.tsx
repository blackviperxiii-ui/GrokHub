import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ModelList } from "./api";

const api = vi.hoisted(() => ({
  listModels: vi.fn<(refresh?: boolean) => Promise<ModelList>>(),
}));
vi.mock("./api", () => api);

import { ModeChips, REMOTE_MODEL_TIP } from "./Modes";
import { ModelPicker } from "./ModelPicker";

const LIST: ModelList = {
  current: "grok-4.3",
  options: [
    { id: "grok-4.3", label: "Grok 4.3", source: "catalog" },
    { id: "grok-4.7", label: "Grok 4.7", source: "catalog" },
    { id: "grok-5-preview", label: "grok-5-preview", source: "account" },
  ],
  note: null,
};

function chips(over: Partial<Parameters<typeof ModeChips>[0]> = {}) {
  return (
    <ModeChips
      mode="chat"
      onMode={() => {}}
      approvals="ask"
      onApprovals={() => {}}
      busy={false}
      remote={false}
      model="grok-4.3"
      onModel={() => {}}
      {...over}
    />
  );
}

describe("ModelPicker", () => {
  beforeEach(() => {
    api.listModels.mockReset();
    api.listModels.mockResolvedValue(LIST);
  });

  it("lists the models with search and picks one", async () => {
    const onPick = vi.fn();
    render(<ModelPicker value="grok-4.3" onPick={onPick} />);
    await userEvent.click(screen.getByRole("button", { name: "grok-4.3" }));
    expect(api.listModels).toHaveBeenCalledWith(false);
    const models = await screen.findByRole("list", { name: "Models" });
    expect(Array.from(models.querySelectorAll(".choice-label"), (n) => n.textContent)).toEqual([
      "Grok 4.3",
      "Grok 4.7",
      "grok-5-preview",
    ]);
    expect(screen.getByText("Your account")).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Search models"), "4.7");
    expect(Array.from(models.querySelectorAll(".choice-label"), (n) => n.textContent)).toEqual([
      "Grok 4.7",
    ]);
    await userEvent.click(screen.getByRole("button", { name: /Grok 4\.7/ }));
    expect(onPick).toHaveBeenCalledWith("grok-4.7");
    expect(screen.queryByRole("dialog", { name: "Pick a model" })).toBeNull();
  });

  it("Other… takes a typed id, trimmed", async () => {
    const onPick = vi.fn();
    render(<ModelPicker value="grok-4.3" onPick={onPick} />);
    await userEvent.click(screen.getByRole("button", { name: "grok-4.3" }));
    await screen.findByRole("list", { name: "Models" });
    await userEvent.click(screen.getByRole("button", { name: "Other…" }));
    expect(screen.getByRole("button", { name: "Use" })).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Model id"), "  my-tuned-grok ");
    await userEvent.click(screen.getByRole("button", { name: "Use" }));
    expect(onPick).toHaveBeenCalledWith("my-tuned-grok");
  });

  it("Refresh reads the account's list again and the note shows", async () => {
    api.listModels.mockResolvedValue({ ...LIST, note: "Add an xAI API key." });
    render(<ModelPicker value="grok-4.3" onPick={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "grok-4.3" }));
    expect(await screen.findByText("Add an xAI API key.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(api.listModels).toHaveBeenLastCalledWith(true);
  });

  it("Esc closes the list", async () => {
    render(<ModelPicker value="grok-4.3" onPick={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "grok-4.3" }));
    await screen.findByRole("list", { name: "Models" });
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Pick a model" })).toBeNull();
  });
});

describe("the composer's model chip", () => {
  beforeEach(() => {
    api.listModels.mockReset();
    api.listModels.mockResolvedValue(LIST);
  });

  it("picking a model calls onModel", async () => {
    const onModel = vi.fn();
    render(chips({ onModel }));
    await userEvent.click(screen.getByRole("button", { name: "Model: grok-4.3" }));
    await userEvent.click(await screen.findByRole("button", { name: /Grok 4\.7/ }));
    expect(onModel).toHaveBeenCalledWith("grok-4.7");
  });

  it("is disabled while a reply runs", () => {
    render(chips({ busy: true }));
    expect(screen.getByRole("button", { name: "Model: grok-4.3" })).toBeDisabled();
  });

  it("only shows the model on the web app", () => {
    render(chips({ remote: true }));
    expect(screen.getByText("Model: grok-4.3")).toHaveAttribute("title", REMOTE_MODEL_TIP);
    expect(screen.queryByRole("button", { name: "Model: grok-4.3" })).toBeNull();
  });

  it("is hidden until Settings loads", () => {
    render(chips({ model: null }));
    expect(screen.queryByText(/^Model:/)).toBeNull();
  });
});
