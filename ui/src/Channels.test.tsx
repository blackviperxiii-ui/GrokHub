import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChannelKind, ChannelState, Channels as Shape, LinkQr, StartQr } from "./channelsApi";

const api = vi.hoisted(() => ({
  getChannels: vi.fn<() => Promise<Shape | null>>(),
  connectChannel: vi.fn<(kind: ChannelKind, token: string) => Promise<Shape | null>>(),
  disconnectChannel: vi.fn<(kind: ChannelKind) => Promise<Shape | null>>(),
  approveChannelCode: vi.fn<(code: string) => Promise<Shape | null>>(),
  setChannelAsks: vi.fn<(kind: ChannelKind, sender: string, on: boolean) => Promise<Shape | null>>(),
  unpairChannelSender: vi.fn<(kind: ChannelKind, sender: string) => Promise<Shape | null>>(),
  addChannelWebhook: vi.fn<(name: string) => Promise<Shape | null>>(),
  removeChannelWebhook: vi.fn<(name: string) => Promise<Shape | null>>(),
  telegramQr: vi.fn<() => Promise<StartQr | null>>(),
  signalLinkStart: vi.fn<(number: string) => Promise<LinkQr | null>>(),
  signalLinkFinish: vi.fn<() => Promise<Shape | null>>(),
}));
vi.mock("./channelsApi", () => api);

import { Channels, QR_NOTE, SIGNAL_QR_NOTE } from "./Channels";

const row = (kind: ChannelKind, name: string, live: boolean, setupLabel = `${name} bot token`): ChannelState => ({
  kind,
  name,
  setupLabel,
  setupHint: live ? "Paste the token." : "Coming next.",
  live,
  connected: false,
  running: false,
  problem: null,
});

const none: Shape = {
  channels: [
    row("telegram", "Telegram", true),
    row("discord", "Discord", true),
    row("slack", "Slack", false, "Slack tokens"),
    row("signal", "Signal", false, "Signal"),
    row("email", "Email", false, "Email"),
  ],
  pending: [],
  paired: [],
  webhooks: [],
  recent: [],
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

  it("shows a pairing QR for a connected Telegram bot, and a new one on asking", async () => {
    const connected: Shape = {
      ...none,
      channels: none.channels.map((ch) => (ch.live ? { ...ch, connected: true, running: true } : ch)),
    };
    api.getChannels.mockResolvedValue(connected);
    api.telegramQr
      .mockResolvedValueOnce({ link: "https://t.me/my_app_bot?start=aaa", svg: "<svg>one</svg>", expiresMs: 1 })
      .mockResolvedValueOnce({ link: "https://t.me/my_app_bot?start=bbb", svg: "<svg>two</svg>", expiresMs: 2 });
    render(<Channels />);
    // Only Telegram has a QR; Discord pairs by code.
    expect(await screen.findAllByRole("button", { name: "Pair a phone with a QR code" })).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "Pair a phone with a QR code" }));
    const figure = await screen.findByRole("figure", { name: "Telegram pairing QR code" });
    const img = screen.getByRole("img", { name: "QR code that opens the bot in Telegram and pairs your phone" });
    expect(img.getAttribute("src")).toBe(`data:image/svg+xml;charset=utf-8,${encodeURIComponent("<svg>one</svg>")}`);
    expect(figure.textContent).toContain(QR_NOTE);
    expect(figure.textContent).toContain("https://t.me/my_app_bot?start=aaa");
    await userEvent.click(screen.getByRole("button", { name: "Show a new QR code" }));
    expect(await screen.findByText("https://t.me/my_app_bot?start=bbb")).toBeTruthy();
    expect(api.telegramQr).toHaveBeenCalledTimes(2);
    await userEvent.click(screen.getByRole("button", { name: "Hide QR code" }));
    expect(screen.queryByRole("figure", { name: "Telegram pairing QR code" })).toBeNull();
  });

  it("says why when the QR can't be made", async () => {
    api.getChannels.mockResolvedValue({
      ...none,
      channels: [{ ...none.channels[0], connected: true, running: true }, ...none.channels.slice(1)],
    });
    api.telegramQr.mockRejectedValue("Telegram refused the bot token. Paste it again from BotFather.");
    render(<Channels />);
    await userEvent.click(await screen.findByRole("button", { name: "Pair a phone with a QR code" }));
    expect(await screen.findByText(/Telegram refused the bot token/)).toBeTruthy();
    expect(screen.queryByRole("figure", { name: "Telegram pairing QR code" })).toBeNull();
  });

  it("connects a bot and clears the token from the page", async () => {
    api.getChannels.mockResolvedValue(none);
    api.connectChannel.mockResolvedValue({
      ...none,
      channels: [{ ...none.channels[0], connected: true, running: true }, ...none.channels.slice(1)],
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

  it("links Signal from a QR for the typed number, and hides the QR when the phone is done", async () => {
    const signalLive: Shape = {
      ...none,
      channels: none.channels.map((ch) =>
        ch.kind === "signal" ? { ...ch, live: true, setupLabel: "Signal number" } : ch,
      ),
    };
    api.getChannels.mockResolvedValue(signalLive);
    api.signalLinkStart.mockResolvedValue({ svg: "<svg>link</svg>", account: "+15551234567" });
    let finish: (next: Shape) => void = () => {};
    api.signalLinkFinish.mockReturnValue(new Promise((done) => (finish = done)));
    render(<Channels />);
    const input = (await screen.findByLabelText("Signal number")) as HTMLInputElement;
    expect(input.type).toBe("tel");
    const link = screen.getByRole("button", { name: "Link with a QR code" });
    expect(link).toBeDisabled();
    await userEvent.type(input, "+1 555 123 4567");
    await userEvent.click(link);
    expect(api.signalLinkStart).toHaveBeenCalledWith("+1 555 123 4567");
    const figure = await screen.findByRole("figure", { name: "Signal link QR code" });
    expect(figure.textContent).toContain(SIGNAL_QR_NOTE);
    expect(screen.getByRole("img", { name: "QR code that links this computer to Signal for +15551234567" })).toBeTruthy();
    finish({
      ...signalLive,
      channels: signalLive.channels.map((ch) => (ch.kind === "signal" ? { ...ch, connected: true, running: true } : ch)),
      status: "Connected Signal as +15551234567. Only your Note to Self is read, and answers go back there.",
    });
    expect(await screen.findByText(/Connected Signal as \+15551234567/)).toBeTruthy();
    expect(screen.queryByRole("figure", { name: "Signal link QR code" })).toBeNull();
  });

  it("says why a Signal link failed and takes the QR down", async () => {
    api.getChannels.mockResolvedValue({
      ...none,
      channels: none.channels.map((ch) => (ch.kind === "signal" ? { ...ch, live: true, setupLabel: "Signal number" } : ch)),
    });
    api.signalLinkStart.mockResolvedValue({ svg: "<svg>link</svg>", account: "+15551234567" });
    api.signalLinkFinish.mockRejectedValue("That phone is signed in to +15559876543, not +15551234567.");
    render(<Channels />);
    await userEvent.type(await screen.findByLabelText("Signal number"), "+15551234567");
    await userEvent.click(screen.getByRole("button", { name: "Link with a QR code" }));
    expect(await screen.findByText(/That phone is signed in to \+15559876543/)).toBeTruthy();
    expect(screen.queryByRole("figure", { name: "Signal link QR code" })).toBeNull();
    expect((screen.getByLabelText("Signal number") as HTMLInputElement).value).toBe("+15551234567");
  });

  it("lists every kind from the table and offers setup only for live ones", async () => {
    api.getChannels.mockResolvedValue(none);
    render(<Channels />);
    for (const name of ["Telegram", "Discord", "Slack", "Signal", "Email"]) {
      expect(await screen.findByRole("listitem", { name })).toBeTruthy();
    }
    expect(screen.getAllByText("Coming next")).toHaveLength(3);
    expect(screen.getAllByText("Not connected")).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: "Connect" })).toHaveLength(2);
    expect(screen.queryByLabelText("Slack tokens")).toBeNull();
  });

  it("pairs a waiting sender by code, shows a wrong code, and unpairs", async () => {
    api.getChannels.mockResolvedValue({
      ...none,
      pending: [{ channel: "telegram", code: "ABCD-1234", label: "@user", expiresMs: 1 }],
    });
    api.approveChannelCode.mockRejectedValueOnce("That code doesn't match anyone waiting. Codes last an hour.");
    api.approveChannelCode.mockResolvedValueOnce({
      ...none,
      paired: [{ channel: "telegram", sender: "777", asks: false, reachable: false }],
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

    api.setChannelAsks.mockResolvedValue({
      ...none,
      paired: [{ channel: "telegram", sender: "777", asks: true, reachable: false }],
      status: "Approval asks for 777 on Telegram start once they write to the bot.",
    });
    const asks = screen.getByRole("button", { name: "Approval asks for 777 on Telegram" });
    expect(asks.textContent).toBe("Send approval asks");
    await userEvent.click(asks);
    expect(api.setChannelAsks).toHaveBeenCalledWith("telegram", "777", true);
    expect(await screen.findByText("Approval asks for 777 on Telegram start once they write to the bot.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Approval asks for 777 on Telegram" }).textContent).toBe(
      "Stop approval asks",
    );
    expect(screen.getByText("· asks start once they write to the bot")).toBeTruthy();

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

  it("lists recent answers and their problems", async () => {
    api.getChannels.mockResolvedValue({
      ...none,
      recent: [
        { source: "Webhook", from: "deploys", text: "", answer: "", problem: "No model is set up.", doneMs: 6 },
        { source: "Telegram", from: "@user", text: "is the build done?", answer: "Yes, it passed.", problem: null, doneMs: 5 },
      ],
    });
    render(<Channels />);
    const list = await screen.findByRole("list", { name: "Recent answers" });
    expect(list.querySelectorAll("li").length).toBe(2);
    expect(screen.getByText("Webhook · deploys")).toBeTruthy();
    expect(screen.getByText("No model is set up.")).toBeTruthy();
    expect(screen.getByText("Telegram · @user")).toBeTruthy();
    expect(screen.getByText("is the build done?")).toBeTruthy();
    expect(screen.getByText("Yes, it passed.")).toBeTruthy();
  });
});
