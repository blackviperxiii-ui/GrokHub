import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const shell = vi.hoisted(() => ({
  inShell: false,
  openUrl: vi.fn<(url: string) => Promise<void>>(() => Promise.resolve()),
}));

vi.mock("./api", () => ({ openUrl: shell.openUrl }));
vi.mock("./transport", () => ({ inShell: () => shell.inShell }));

import { COPIED_LABEL, COPY_LABEL, Markdown } from "./Markdown";

beforeEach(() => {
  shell.inShell = false;
  shell.openUrl.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("Markdown", () => {
  it("renders headings at their level", () => {
    render(<Markdown text={"# Top\n### Third **bold**"} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Top");
    // Headings show plain text, as in the cabin.
    expect(screen.getByRole("heading", { level: 3 }).textContent).toBe("Third bold");
  });

  it("renders bullet, numbered and task lists", () => {
    const { container } = render(<Markdown text={"- apple\n  - core\n3. third\n4) fourth\n- [x] done\n- [ ] todo"} />);
    const lists = screen.getAllByRole("list");
    expect(lists).toHaveLength(3);
    expect(lists[0].tagName).toBe("UL");
    expect(within(lists[0]).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["apple", "core"]);
    expect(lists[1].tagName).toBe("OL");
    const numbered = within(lists[1]).getAllByRole("listitem");
    expect(numbered.map((li) => li.getAttribute("value"))).toEqual(["3", "4"]);
    expect(numbered.map((li) => li.textContent)).toEqual(["3.third", "4.fourth"]);
    const boxes = within(lists[2]).getAllByRole("checkbox");
    expect(boxes.map((b) => (b as HTMLInputElement).checked)).toEqual([true, false]);
    expect(boxes.every((b) => (b as HTMLInputElement).disabled)).toBe(true);
    expect(container.querySelector(".md-dot.hollow")).not.toBeNull();
  });

  it("renders inline bold, italic, strike and code", () => {
    const { container } = render(<Markdown text={"**b** *i* ~~s~~ `c*d`"} />);
    expect(container.querySelector("strong")).toHaveTextContent("b");
    expect(container.querySelector("em")).toHaveTextContent("i");
    expect(container.querySelector("s")).toHaveTextContent("s");
    expect(container.querySelector("code.md-inline-code")).toHaveTextContent("c*d");
  });

  it("renders a code block with its language, colours and a Copy that copies the body", async () => {
    const writeText = vi.fn<(text: string) => Promise<void>>(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const body = 'let x = "hi"; // note\nx + 1';
    render(<Markdown text={"Run:\n```Rust\n" + body + "\n```"} />);
    const block = screen.getByRole("group", { name: "rust code" });
    expect(within(block).getByText("rust")).toBeInTheDocument();
    expect(block.querySelector("pre")!.textContent).toBe(body);
    expect(block.querySelector(".md-tok-keyword")).toHaveTextContent("let");
    expect(block.querySelector(".md-tok-str")).toHaveTextContent('"hi"');
    expect(block.querySelector(".md-tok-comment")).toHaveTextContent("// note");
    const copy = within(block).getByRole("button", { name: COPY_LABEL });
    await userEvent.click(copy);
    expect(writeText).toHaveBeenCalledWith(body);
    expect(copy).toHaveTextContent(COPIED_LABEL);
  });

  it("renders a half-streamed fence as code without crashing", () => {
    const { rerender } = render(<Markdown text={"Here:\n```py\nprint(1)\nprin"} />);
    const block = screen.getByRole("group", { name: "py code" });
    expect(block.querySelector("pre")!.textContent).toBe("print(1)\nprin");
    rerender(<Markdown text={"Here:\n```py\nprint(1)\nprint(2)\n```\nDone."} />);
    expect(screen.getByRole("group", { name: "py code" }).querySelector("pre")!.textContent).toBe(
      "print(1)\nprint(2)",
    );
    expect(screen.getByText("Done.")).toBeInTheDocument();
  });

  it("renders a table with a header, alignment and inline cells in a scroll region", () => {
    render(<Markdown text={"| Name | Size |\n|:--|--:|\n| **a** | 1 |\n| b |"} />);
    const region = screen.getByRole("region", { name: "Table" });
    const table = within(region).getByRole("table");
    const headers = within(table).getAllByRole("columnheader");
    expect(headers.map((h) => h.textContent)).toEqual(["Name", "Size"]);
    expect(headers[1]).toHaveStyle({ textAlign: "right" });
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(within(rows[1]).getAllByRole("cell").map((c) => c.textContent)).toEqual(["a", "1"]);
    expect(rows[1].querySelector("strong")).toHaveTextContent("a");
    expect(within(rows[2]).getAllByRole("cell").map((c) => c.textContent)).toEqual(["b", ""]);
  });

  it("renders quotes and rules", () => {
    const { container } = render(<Markdown text={"> quoted *text*\n---"} />);
    expect(container.querySelector("blockquote")).toHaveTextContent("quoted text");
    expect(container.querySelector("hr")).not.toBeNull();
  });

  it("links http(s) to a new tab in the web app", async () => {
    render(<Markdown text={"See [the docs](https://x.ai/docs) or https://example.com."} />);
    const docs = screen.getByRole("link", { name: "the docs" });
    expect(docs).toHaveAttribute("href", "https://x.ai/docs");
    expect(docs).toHaveAttribute("target", "_blank");
    expect(docs).toHaveAttribute("rel", "noopener noreferrer");
    expect(screen.getByRole("link", { name: "https://example.com" })).toHaveAttribute("href", "https://example.com");
    fireEvent.click(docs);
    expect(shell.openUrl).not.toHaveBeenCalled();
  });

  it("opens a link through open_url in the shell, not in the webview", () => {
    shell.inShell = true;
    render(<Markdown text={"[go](https://x.ai)"} />);
    const link = screen.getByRole("link", { name: "go" });
    const event = new MouseEvent("click", { bubbles: true, cancelable: true });
    act(() => {
      link.dispatchEvent(event);
    });
    expect(event.defaultPrevented).toBe(true);
    expect(shell.openUrl).toHaveBeenCalledWith("https://x.ai");
  });

  it("keeps javascript:, data: and file: links as text", () => {
    const text = "[a](javascript:alert(1)) [b](data:text/html,x) [c](file:///etc/passwd) javascript:alert(2)";
    const { container } = render(<Markdown text={text} />);
    expect(screen.queryByRole("link")).toBeNull();
    expect(container.querySelector("a")).toBeNull();
    expect(container.textContent).toBe(text);
  });

  it("shows a script tag and raw HTML as text, never as markup", () => {
    const text = "<script>window.pwned = true</script>\n<b>bold?</b> <img src=x onerror=\"window.pwned = true\">";
    const { container } = render(<Markdown text={text} />);
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("b")).toBeNull();
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText("<script>window.pwned = true</script>")).toBeInTheDocument();
    expect(screen.getByText('<b>bold?</b> <img src=x onerror="window.pwned = true">')).toBeInTheDocument();
    expect((window as unknown as { pwned?: boolean }).pwned).toBeUndefined();
  });

  it("shows HTML inside a code block as text too", () => {
    const { container } = render(<Markdown text={"```html\n<script>alert(1)</script>\n```"} />);
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("pre")!.textContent).toBe("<script>alert(1)</script>");
  });
});
