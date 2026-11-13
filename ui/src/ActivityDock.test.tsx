import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DOCK_MAX, queuedItems, type ActivityItem } from "./activity";
import { ActivityDock } from "./ActivityDock";

const NOW = 1_000_000;

function item(over: Partial<ActivityItem>): ActivityItem {
  return {
    key: "chat:shell-1",
    kind: "chat",
    title: "Chat reply",
    status: "running",
    detail: "Read README.md",
    steps: 2,
    at: NOW - 1000,
    ...over,
  };
}

describe("ActivityDock", () => {
  it("stays hidden with nothing running, waiting or just finished", () => {
    const { container } = render(
      <ActivityDock items={[]} now={NOW} onOpenChat={() => {}} onOpenAutomations={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("lists running, waiting and done items and opens the linked page", async () => {
    const onOpenChat = vi.fn();
    const onOpenAutomations = vi.fn();
    render(
      <ActivityDock
        now={NOW}
        onOpenChat={onOpenChat}
        onOpenAutomations={onOpenAutomations}
        items={[
          item({}),
          item({
            key: "run:auto-1",
            kind: "routine",
            title: "summarize my inbox",
            status: "done",
            detail: "3 new mails, none urgent.",
            steps: 0,
          }),
          item({ key: "chat:shell-2", title: "Fix the build", status: "waiting", detail: "Needs your OK: Run npm test" }),
        ]}
      />,
    );
    const dock = screen.getByRole("complementary", { name: "Activity" });
    const toggle = within(dock).getByRole("button", { name: /Activity/ });
    expect(toggle).toHaveTextContent("1 waiting · 1 running · 1 done");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(toggle);

    const rows = within(dock).getAllByRole("listitem");
    expect(rows.map((r) => r.getAttribute("aria-label"))).toEqual([
      "Fix the build",
      "Chat reply",
      "summarize my inbox",
    ]);
    expect(rows[0]).toHaveTextContent("Waiting on you");
    expect(rows[1]).toHaveTextContent("Running");
    expect(rows[1]).toHaveTextContent("2 steps done");
    expect(rows[2]).toHaveTextContent("3 new mails, none urgent.");

    await userEvent.click(within(rows[0]).getByRole("button", { name: "Review" }));
    expect(onOpenChat).toHaveBeenCalledTimes(1);
    await userEvent.click(toggle);
    await userEvent.click(
      within(within(dock).getByRole("listitem", { name: "summarize my inbox" })).getByRole("button", {
        name: "Open result",
      }),
    );
    expect(onOpenAutomations).toHaveBeenCalledTimes(1);
  });

  it("counts what does not fit", async () => {
    const many = Array.from({ length: DOCK_MAX + 2 }, (_, i) =>
      item({ key: `run:${i}`, kind: "routine", title: `job ${i}`, at: NOW - i }),
    );
    render(<ActivityDock items={many} now={NOW} onOpenChat={() => {}} onOpenAutomations={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: /Activity/ }));
    expect(screen.getByText("2 more")).toBeInTheDocument();
    expect(screen.getAllByRole("listitem", { name: /^job / })).toHaveLength(DOCK_MAX);
  });

  it("a queued row offers Steer now only while its reply can take it, and Remove", async () => {
    const queued = queuedItems("shell-c1", ["then deploy"]);
    const onSteer = vi.fn();
    const onRemove = vi.fn();
    const { rerender } = render(
      <ActivityDock
        items={queued}
        onOpenChat={() => {}}
        onOpenAutomations={() => {}}
        canSteer={() => false}
        onSteer={onSteer}
        onRemove={onRemove}
        now={1}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Activity, 1 queued" }));
    const row = screen.getByRole("listitem", { name: "then deploy" });
    expect(row).toHaveTextContent("Queued");
    expect(within(row).queryByRole("button", { name: "Steer now" })).toBeNull();
    await userEvent.click(within(row).getByRole("button", { name: "Remove" }));
    expect(onRemove).toHaveBeenCalledWith(queued[0]);
    rerender(
      <ActivityDock
        items={queued}
        onOpenChat={() => {}}
        onOpenAutomations={() => {}}
        canSteer={() => true}
        onSteer={onSteer}
        onRemove={onRemove}
        now={1}
      />,
    );
    await userEvent.click(within(screen.getByRole("listitem", { name: "then deploy" })).getByRole("button", { name: "Steer now" }));
    expect(onSteer).toHaveBeenCalledWith(queued[0]);
  });
});
