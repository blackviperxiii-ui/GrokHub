import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChannelKind, Channels as Shape } from "./channelsApi";

const api = vi.hoisted(() => ({
  getChannels: vi.fn<() => Promise<Shape | null>>(),
  connectChannel: vi.fn<(kind: ChannelKind, token: string) => Promise<Shape | null>>(),
  disconnectChannel: vi.fn<(kind: ChannelKind) => Promise<Shape | null>>(),
  approveChannelCode: vi.fn<(code: string) => Promise<Shape | null>>(),
  unpairChannelSender: vi.fn<(kind: ChannelKind, sender: string) => Promise<Shape | null>>(),
  addChannelWebhook: vi.fn<(name: string) => Promise<Shape | null>>(),
  removeChannelWebhook: vi.fn<(name: string) => Promise<Shape | null>>(),
}));
vi.mock("./channelsApi", () => api);

import { Channels } from "./Channels";

const none: Shape = {
  channels: [
    { kind: "telegram", connected: false, running: false, problem: null },
    { kind: "discord", connected: false, running: false, problem: null },
  ],
  pending: [],
  paired: [],
  webhooks: [],
  newSecret: null,
  status: null,
};

const TOKEN = "123456789:AAabcdefghijklmnopqrstuvwxyz0123456";

describe("Phone and chat apps", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("stays hidden in the web app", async () => {
    api.getChannels.mockResolvedValue(null);
    const { container } = render(<Channels />);
    await waitFor(() => expect(api.getChannels).toHaveBeenCalledTimes(1));
    expect(container.textContent).toBe("");
  });

  it("connects a bot and clears the token from the page", async () => {
    api.getChannels.mockResolvedValue(none);
    api.connectChannel.mockResolvedValue({
      ...none,
      channels: [{ kind: "telegram", connected: true, running: true, problem: null }, none.channels[1]],
      status: "Connected Telegram as @my_app_bot. Strangers who message it get a pairing code.",
    });
    render(<Channels />);
    const input = (await screen.findByLabelText("Telegram bot token")) as HTMLInputElement;
    expect(input.type).toBe("password");
    await userEvent.type(input, TOKEN);
    await userEvent.click(screen.getAllByRole("button", { name: "Connect" })[0]);
    expect(api.connectChannel).toHaveBeenCalledWith("telegram", TOKEN);
    expect(
      await screen.findByText("Connected Telegram as @my_app_bot. Strangers who message it get a pairing code."),
    ).toBeTruthy();
    expect(screen.getByText("Listening for messages")).toBeTruthy();
    expect(screen.queryByLabelText("Telegram bot token")).toBeNull();
  });

  it("pairs a waiting sender by code, shows a wrong code, and unpairs", async () => {
    api.getChannels.mockResolvedValue({
      ...none,
      pending: [{ channel: "telegram", code: "ABCD-1234", label: "@user", expiresMs: 1 }],
    });
    api.approveChannelCode.mockRejectedValueOnce("That code doesn't match anyone waiting. Codes last an hour.");
    api.approveChannelCode.mockResolvedValueOnce({
      ...none,
      paired: [{ channel: "telegram", sender: "777" }],
      status: "Paired @user on Telegram.",
    });
    api.unpairChannelSender.mockResolvedValue({ ...none, status: "Unpaired 777 on Telegram." });
    render(<Channels />);
    expect(await screen.findByText("@user on Telegram was sent a code")).toBeTruthy();

    await userEvent.type(screen.getByLabelText("Pairing code"), "WRONG-0000");
    await userEvent.click(screen.getByRole("button", { name: "Pair" }));
    expect(await screen.findByText("That code doesn't match anyone waiting. Codes last an hour.")).toBeTruthy();
    expect((screen.getByLabelText("Pairing code") as HTMLInputElement).value).toBe("WRONG-0000");

    await userEvent.clear(screen.getByLabelText("Pairing code"));
    await userEvent.type(screen.getByLabelText("Pairing code"), " abcd-1234 ");
    await userEvent.click(screen.getByRole("button", { name: "Pair" }));
    expect(api.approveChannelCode).toHaveBeenLastCalledWith("abcd-1234");
    expect(await screen.findByText("Paired @user on Telegram.")).toBeTruthy();
    expect((screen.getByLabelText("Pairing code") as HTMLInputElement).value).toBe("");

    await userEvent.click(screen.getByRole("button", { name: "Unpair 777 on Telegram" }));
    expect(api.unpairChannelSender).toHaveBeenCalledWith("telegram", "777");
    expect(await screen.findByText("Unpaired 777 on Telegram.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Unpair 777 on Telegram" })).toBeNull();
  });

  it("shows a new webhook's secret once and removes a webhook", async () => {
    const secret = "ab".repeat(32);
    const deploys = { name: "deploys", enabled: true, filters: 0, path: "/v1/hook/deploys" };
    api.getChannels.mockResolvedValue(none);
    api.addChannelWebhook.mockResolvedValue({
      ...none,
      webhooks: [deploys],
      newSecret: secret,
      status: "Added deploys. Copy the secret now: it isn't shown again.",
    });
    api.removeChannelWebhook.mockResolvedValue({ ...none, status: "Removed deploys. Its secret no longer works." });
    render(<Channels />);
    await userEvent.type(await screen.findByLabelText("Webhook name"), "deploys");
    await userEvent.click(screen.getByRole("button", { name: "Add webhook" }));
    expect(api.addChannelWebhook).toHaveBeenCalledWith("deploys");
    expect(await screen.findByText(secret)).toBeTruthy();
    expect(screen.getByText("POST /v1/hook/deploys")).toBeTruthy();

    await userEvent.click(screen.getByRole("button", { name: "Remove webhook deploys" }));
    expect(api.removeChannelWebhook).toHaveBeenCalledWith("deploys");
    expect(await screen.findByText("Removed deploys. Its secret no longer works.")).toBeTruthy();
    expect(screen.queryByText(secret)).toBeNull();
  });
});
