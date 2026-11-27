// Connectors → Phone and chat apps (src-tauri/src/channels.rs): a Telegram or
// Discord bot the user made, its token in the OS keychain, and the senders
// paired with it. Desktop only: the web app and the tests get `null`.
import { call, inShell } from "./transport";

export type ChannelKind = "telegram" | "discord" | "slack" | "signal" | "email";

/** One row of the shell's channel table (msg_channels::KINDS) and its state. */
export interface ChannelState {
  kind: ChannelKind;
  /** What the page calls it. */
  name: string;
  /** The setup box's label and hint. */
  setupLabel: string;
  setupHint: string;
  /** It can be connected in this build; the others show as coming next. */
  live: boolean;
  /** A token is saved. */
  connected: boolean;
  /** Listening for messages now. */
  running: boolean;
  problem: string | null;
}

export interface PendingCode {
  channel: ChannelKind;
  code: string;
  label: string;
  expiresMs: number;
}

export interface PairedSender {
  channel: ChannelKind;
  sender: string;
  /** Approval asks go to this sender. */
  asks: boolean;
  /** They have written to the bot, so asks have a chat to go to. */
  reachable: boolean;
}

export interface ChannelWebhook {
  name: string;
  enabled: boolean;
  filters: number;
  /** Where the sender posts on the hub this computer shares. */
  path: string;
}

/** A message or webhook the assistant already answered. */
export interface ChannelRecent {
  source: "Telegram" | "Discord" | "Webhook" | "Unknown";
  /** The sender's label or the webhook's name. */
  from: string;
  text: string;
  answer: string;
  problem: string | null;
  doneMs: number;
}

export interface Channels {
  channels: ChannelState[];
  pending: PendingCode[];
  paired: PairedSender[];
  webhooks: ChannelWebhook[];
  /** Newest first. */
  recent: ChannelRecent[];
  /** A new webhook's secret, only in the answer that made it. */
  newSecret: string | null;
  /** What the last step did. */
  status: string | null;
}

/** A QR that pairs the phone which scans it with the Telegram bot. */
export interface StartQr {
  /** The `t.me` link the QR holds, for a phone that can't scan. */
  link: string;
  /** The QR as an SVG document. */
  svg: string;
  expiresMs: number;
}

export async function getChannels(): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("get_channels");
}

export async function connectChannel(kind: ChannelKind, token: string): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("connect_channel", { kind, token });
}

/** Desktop only: each call makes a new link and the last one stops working. */
export async function telegramQr(): Promise<StartQr | null> {
  if (!inShell()) return null;
  return call<StartQr>("telegram_qr");
}

/** A QR the Signal app scans to link this computer's signal-cli as a device. */
export interface LinkQr {
  /** The QR as an SVG document. */
  svg: string;
  /** The number the phone must be signed in to. */
  account: string;
}

/** Desktop only: asks the local signal-cli daemon for a device link for `number`. */
export async function signalLinkStart(number: string): Promise<LinkQr | null> {
  if (!inShell()) return null;
  return call<LinkQr>("signal_link_start", { number });
}

/** Desktop only: waits up to three minutes for the phone to scan, then connects. */
export async function signalLinkFinish(): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("signal_link_finish");
}

export async function disconnectChannel(kind: ChannelKind): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("disconnect_channel", { kind });
}

export async function approveChannelCode(code: string): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("approve_channel_code", { code });
}

export async function setChannelAsks(kind: ChannelKind, sender: string, on: boolean): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("set_channel_asks", { kind, sender, on });
}

export async function unpairChannelSender(kind: ChannelKind, sender: string): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("unpair_channel_sender", { kind, sender });
}

/** The answer carries the new secret once, in `newSecret`. */
export async function addChannelWebhook(name: string): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("add_channel_webhook", { name });
}

export async function removeChannelWebhook(name: string): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("remove_channel_webhook", { name });
}
