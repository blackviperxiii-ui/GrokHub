// Connectors → Phone and chat apps (src-tauri/src/channels.rs): a Telegram or
// Discord bot the user made, its token in the OS keychain, and the senders
// paired with it. Desktop only: the web app and the tests get `null`.
import { call, inShell } from "./transport";

export type ChannelKind = "telegram" | "discord";

export interface ChannelState {
  kind: ChannelKind;
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

export async function getChannels(): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("get_channels");
}

export async function connectChannel(kind: ChannelKind, token: string): Promise<Channels | null> {
  if (!inShell()) return null;
  return call<Channels>("connect_channel", { kind, token });
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
