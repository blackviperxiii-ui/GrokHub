import { render, screen } from "@testing-library/react";
import { Orb, orbFor, orbLabel, orbTheme } from "./Orb";

describe("orb", () => {
  it("picks the orb for what the chat is doing", () => {
    expect(orbFor("thinking")).toBe("working");
    expect(orbFor("tool:web_search")).toBe("searching");
    expect(orbFor("tool:Fetch https://example.com")).toBe("searching");
    expect(orbFor("tool:Browse the page")).toBe("searching");
    expect(orbFor("tool:read_file")).toBe("solving");
    expect(orbLabel("thinking")).toBe("Thinking");
    expect(orbLabel("tool:web_search")).toBe("Looking it up");
    expect(orbLabel("tool:read_file")).toBe("Working on it");
  });

  it("inks for the shell's theme", () => {
    expect(orbTheme(undefined)).toBe("auto");
    expect(orbTheme("light")).toBe("light");
    expect(orbTheme("ops-dark")).toBe("dark");
    expect(orbTheme("oled-black")).toBe("dark");
    expect(orbTheme("soft-charcoal")).toBe("dark");
  });

  it("hides from screen readers when the control around it is labelled", () => {
    const { container } = render(<Orb state="listening" />);
    expect(container.querySelector("canvas")).toHaveAttribute("aria-hidden", "true");
    expect(screen.queryByRole("img")).toBeNull();
  });
});
