import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Toggle } from "./Toggle";

function Harness({ start, onChange }: { start: boolean; onChange: (next: boolean) => void }) {
  const [on, setOn] = useState(start);
  return (
    <Toggle
      checked={on}
      onChange={(next) => {
        setOn(next);
        onChange(next);
      }}
    >
      Use idle time
    </Toggle>
  );
}

describe("Toggle", () => {
  it("is a labelled switch, not a checkbox", () => {
    render(<Harness start={false} onChange={() => {}} />);
    const sw = screen.getByRole("switch", { name: "Use idle time" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(sw).not.toBeChecked();
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("flips on a click, on its text, and from the keyboard", async () => {
    const seen = vi.fn<(next: boolean) => void>();
    render(<Harness start={false} onChange={seen} />);
    const sw = screen.getByRole("switch", { name: "Use idle time" });

    await userEvent.click(sw);
    expect(sw).toBeChecked();
    await userEvent.click(screen.getByText("Use idle time"));
    expect(sw).not.toBeChecked();
    sw.focus();
    await userEvent.keyboard(" ");
    expect(sw).toBeChecked();
    await userEvent.keyboard("{Enter}");
    expect(sw).not.toBeChecked();
    expect(seen.mock.calls).toEqual([[true], [false], [true], [false]]);
  });

  it("does nothing while disabled", async () => {
    const seen = vi.fn<(next: boolean) => void>();
    render(
      <Toggle checked onChange={seen} disabled>
        Never unload
      </Toggle>,
    );
    const sw = screen.getByRole("switch", { name: "Never unload" });
    await userEvent.click(sw);
    expect(sw).toBeChecked();
    expect(seen).not.toHaveBeenCalled();
  });
});
