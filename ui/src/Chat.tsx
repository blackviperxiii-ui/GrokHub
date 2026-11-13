import {
  memo,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import {
  addAutomation,
  answerElicit,
  codeSessionPlan,
  answerPermission,
  cancelChat,
  closeChat,
  deleteChat,
  exportChat,
  forgetMessage,
  getHangoutMode,
  getLastChat,
  getSettings,
  helperQuestions,
  loadChat,
  onChatEvent,
  onEventGap,
  readTurn,
  replyPick,
  runSlash,
  saveSettings,
  sendChat,
  setHangoutMode,
  sideChat,
  steerChat,
  tellMain,
  type Answer,
  type ChatEvent,
  type ElicitAction,
  type HangoutMode,
  type SavedFile,
  type SessionMode,
  type Settings,
  type SlashEffect,
  type SlashHit,
  type QuickChip,
  type Transcript,
  type TurnRead,
} from "./api";
import { ChipRow, useChipRow } from "./Chips";
import { ChatTabs, HangoutHeader } from "./ChatTabs";
import { GoalAsk } from "./Goals";
import { ATTACH_MAX, AttachButton, AttachChips, useAttachments } from "./AttachChips";
import {
  type Cut,
  cutFor,
  DELETE_NEEDS_EARLIER,
  DeleteMessageDialog,
  MESSAGE_MENU_LABEL,
  MessageMenu,
  withoutCut,
} from "./DeleteMessage";
import { requestImagine } from "./imagineAsk";
import type { View } from "./Palette";
import { Markdown } from "./Markdown";
import { displayText } from "./md";
import { appendDraft, Mic } from "./Mic";
import { useVoiceMode, VoiceButton } from "./VoiceMode";
import { ElicitCard, type ElicitItem } from "./Modes";
import { SIDE_MARK, SideCards, withSide, type Side } from "./Side";
import { NyxDock } from "./NyxDock";
import { NyxFace } from "./NyxFace";
import { Orb, orbFor, orbLabel } from "./Orb";
import { activityOf, useNyx } from "./nyx";
import { PRODUCT_NAME } from "./brand";
import { isSlashCommand, SlashMenu, useSlashHits } from "./SlashMenu";
import { needsSettings } from "./ui";
import { SavedCard } from "./SavedCard";
import { settingAsk } from "./autoSetupApi";
import { Episode } from "./Episode";
import { ScreenshotOffer } from "./ScreenshotOffer";
import { openImport } from "./onboardApi";
import { EvidenceCard, FindingsCard, RecapCard } from "./TurnExtras";
import { Inbox } from "./Inbox";
import { modelResume, onModelBack, type ModelResume } from "./modelWait";
import { BudgetCard } from "./BudgetCard";

export type Item =
  | { kind: "user"; text: string }
  | { kind: "assistant"; text: string; streaming: boolean }
  | { kind: "tool"; id: string; title: string; status: string; detail: string }
  | {
      kind: "permission";
      id: string;
      title: string;
      action: string;
      reason: string;
      /** "ended": the turn stopped before anyone answered, so nothing ran;
       * "halted": Halt stopped it. */
      answer: Answer | Ended | null;
      /** A helper asked; it waits past the end of the reply. */
      helper?: boolean;
    }
  /** A saved chat's tool steps, named on one muted line instead of cards. */
  | { kind: "tools"; text: string }
  /** Halt stopped this reply. */
  | { kind: "halted" }
  /** What a slash command did. It stays in the log and never reaches the model. */
  | { kind: "slash"; command: string; text: string }
  /** `/export` saved this chat; the card opens or shows the file. */
  | { kind: "saved"; saved: SavedFile }
  /** A destructive slash command asks first; Yes runs `yes`. */
  | { kind: "confirm"; command: string; prompt: string; yes: string }
  /** A report_findings card under the reply (Order 733). */
  | Extract<ChatEvent, { kind: "findings" }>
  /** The reply's Finished block as a card (Order 733). */
  | Extract<ChatEvent, { kind: "recap" }>
  /** The evidence receipt for the turn that just ended (Order 733). */
  | Extract<ChatEvent, { kind: "evidence" }>
  | ElicitItem;

type Ended = "ended" | "halted";

/** The stop reason of a turn Halt ended (`agent::HALTED`). */
export const HALTED_STOP = "halted";
export const HALTED_LINE = "Halted. This reply stopped, and nothing more ran.";

/** Steer now or Remove on the `index`th queued message; a new `n` runs it. */
export interface QueueAct {
  op: "steer" | "remove";
  index: number;
  n: number;
}

/** Text to put back in the box, after what is typed there. */
function withQueued(draft: string, texts: readonly string[]): string {
  const back = texts.join("\n\n");
  return draft.trim() ? `${draft.trimEnd()}\n\n${back}` : back;
}

export interface Prefill {
  text: string;
  n: number;
  conversationId?: string;
  /** Send it as a chat message instead of filling the box (the Command
   * line's run); behind a running reply it waits its turn. */
  send?: boolean;
}

const PLACEHOLDER = `Message ${PRODUCT_NAME}`;

/** `/view-plan` before any plan was made. */
export const NO_PLAN = "No plan yet. Use Plan mode.";

/** Bare `/plan`: the next message plans first. */
export const PLAN_NEXT = "Your next message gets a plan first.";
/** Bare `/chat`: the next message is answered straight away. */
export const CHAT_NEXT = "Your next message gets a straight answer, with no plan.";
/** `/undo` with no reply running: the session only grows, so there is nothing to take back. */
export const UNDO_NA = "N/A on native threads: the session is append-only.";
/** Bare `/btw`: side questions need no command. */
export const BTW_HINT =
  "No command needed: ask your question while a reply runs and it's answered on the side.";
/** A permission card's Stop asking me, once the setting moved. */
export const STOP_ASKING_DONE =
  "Allowed. From now on only risky things ask. Settings → Defaults changes it back.";

export const CHAT_EMPTY = `Ask ${PRODUCT_NAME} anything.`;
/** The Code tab's agent window, before its first message. */
export const AGENT_EMPTY = "Tell the agent what to build or fix in this project.";
export const AGENT_PLACEHOLDER = "Message the agent";
/** Messages typed in the agent window while a reply runs. */
export function waitingLine(count: number): string {
  return count === 1 ? "1 message waits for this reply to end." : `${count} messages wait for this reply to end.`;
}
export const SIGNED_OUT =
  `${PRODUCT_NAME} chats with your xAI API key. It's kept in this device's keychain, never in a file.`;
export const SIGNED_OUT_GROK =
  `Sign in with your Grok account, or add an xAI API key. Either one is kept in this device's keychain, never in a file.`;
export const SIGNED_OUT_KEY_HINT = "No key yet? Make one at console.x.ai.";
export const SIGNED_OUT_REMOTE =
  `${PRODUCT_NAME} on your computer isn't signed in yet. Sign in there, then chat here.`;

/** One primary sign-in button and one hint, on every platform. The web app
 * signs in on the computer, so it only says so. */
function SignedOut({
  remote,
  grok,
  onSignIn,
}: {
  remote: boolean;
  /** This device offers Sign in with Grok. */
  grok: boolean;
  onSignIn: () => void;
}) {
  if (remote) return <p className="empty">{SIGNED_OUT_REMOTE}</p>;
  return (
    <div className="signed-out" role="region" aria-label="Sign in">
      <h2>Sign in to start</h2>
      <p>{grok ? SIGNED_OUT_GROK : SIGNED_OUT}</p>
      <button type="button" onClick={onSignIn}>
        {grok ? "Sign in with Grok" : "Add your API key"}
      </button>
      <p className="note">{SIGNED_OUT_KEY_HINT}</p>
    </div>
  );
}

/** A saved chat's lines as the log shows them. */
export function itemsFrom(transcript: Transcript): Item[] {
  return transcript.lines.map((line): Item => {
    if (line.role === "user") return { kind: "user", text: line.text };
    if (line.role === "tools") return { kind: "tools", text: line.text };
    return { kind: "assistant", text: line.text, streaming: false };
  });
}

/** How many more older lines each Show earlier loads. */
export const EARLIER_STEP = 200;

export const OPEN_BUSY = "A reply is still running. Stop it or wait, then open the chat again.";

export const EVENTS_LOST = `Lost touch with ${PRODUCT_NAME} on your computer for a while. Part of this reply may be missing.`;
export const MISSION_BUSY = "A reply is still running. Stop it or wait, then press To mission again.";
/** Slash lines that leave the running reply alone, so they run beside it. */
const BESIDE_REPLY = /^\/(queue|bg|background|send|consult)(\s|$)/i;
/** Every other slash line typed during a reply stays in the box with this. */
export const SLASH_WAITS = "That command waits for the reply. /stop ends it.";
/** The web app has no /stop. */
export const SLASH_WAITS_REMOTE = "That command waits for the reply.";
/** Enter again on a slash line whose answer is still out. */
export const SLASH_RUNNING = "That command is still running.";

/** Whether a slash line runs while a reply does: /stop and /undo act on the
 * reply (the web app has neither), the rest leave it alone. */
function runsBesideReply(line: string, remote?: boolean): boolean {
  return (/^\/(stop|undo)$/i.test(line) && !remote) || BESIDE_REPLY.test(line);
}

export function newConversationId(): string {
  return `shell-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** Folds one engine event into the log. Errors and Done are handled by the caller. */
export function applyEvent(items: Item[], event: ChatEvent): Item[] {
  switch (event.kind) {
    case "text": {
      const last = items[items.length - 1];
      if (last?.kind === "assistant" && last.streaming) {
        return [...items.slice(0, -1), { ...last, text: last.text + event.text }];
      }
      return [...items, { kind: "assistant", text: event.text, streaming: true }];
    }
    case "tool": {
      const card: Item = { ...event };
      const at = items.findIndex((item) => item.kind === "tool" && item.id === event.id);
      if (at < 0) return [...items, card];
      return items.map((item, i) => (i === at ? card : item));
    }
    case "permission":
      return [...items, { ...event, answer: null }];
    // Deny on a notification: the card shows what ran, unless the turn already ended or was halted.
    case "answered":
      return items.map((item) =>
        item.kind === "permission" && item.id === event.id && item.answer !== "ended" && item.answer !== "halted"
          ? { ...item, answer: event.answer }
          : item,
      );
    // The plan comes again on its approval card; one copy is enough.
    case "elicit":
      return [...items, { ...event, answer: null }];
    case "findings":
    case "evidence":
      return [...items, { ...event }];
    // The Finished block leaves the bubble for its card: the last reply's
    // text becomes the reply without the block.
    case "recap": {
      let at = -1;
      for (let i = items.length - 1; i >= 0; i--) {
        if (items[i].kind === "assistant") {
          at = i;
          break;
        }
      }
      const swapped =
        at < 0
          ? items
          : items.map((item, i) =>
              i === at && item.kind === "assistant" ? { ...item, text: event.body } : item,
            );
      return [...swapped, { ...event }];
    }
    // A reply's follow-ups join the chip row under the box (`useChipRow`).
    default:
      return items;
  }
}

/** Adds a command's line to the log. A reply that is streaming stays the last
 * item, so its next words join the same bubble: the line goes just above it. */
function withNotice(items: Item[], notice: Item): Item[] {
  const last = items[items.length - 1];
  if (last?.kind === "assistant" && last.streaming) return [...items.slice(0, -1), notice, last];
  return [...items, notice];
}

/** The turn ended: stop streaming, and retire cards nobody answered. A
 * helper's question still waits, unless Halt stopped everything. */
function settle(items: Item[], how: Ended = "ended"): Item[] {
  return items.map((item) => {
    if (item.kind === "assistant" && item.streaming) return { ...item, streaming: false };
    const asks = item.kind === "permission" || item.kind === "elicit";
    if (asks && item.answer === null && (how === "halted" || !item.helper)) return { ...item, answer: how };
    return item;
  });
}

/** A helper's question, added once: it can come after the reply ended, and
 * again when the chat opens. */
export function withHelperAsk(items: Item[], event: ChatEvent): Item[] {
  if (event.kind !== "permission" && event.kind !== "elicit") return items;
  const shown = items.some((item) => (item.kind === "permission" || item.kind === "elicit") && item.id === event.id);
  return shown ? items : [...items, { ...event, answer: null, helper: true }];
}

/** Halt ended the turn: its cards read halted, and one line says so. */
function settleHalted(items: Item[]): Item[] {
  const settled = settle(items, "halted");
  return settled.at(-1)?.kind === "halted" ? settled : [...settled, { kind: "halted" }];
}

const ANSWERED: Record<Answer | Ended, string> = {
  allow: "Allowed once",
  always: "Always allowed",
  deny: "Denied",
  ended: "Not run: the turn ended",
  halted: "Not run: halted",
};

// Typing in the composer re-renders Chat, and a reply streaming in re-renders it
// on every chunk. These take plain values and are memoized, so only the message
// that changed repaints and a long conversation stays cheap to type under.
/** Opens a bubble's menu at a point on the page. */
type MenuAt = (x: number, y: number) => void;

/** Right-click opens the bubble's menu where the pointer is. */
function contextMenuAt(onMenu?: MenuAt) {
  if (!onMenu) return undefined;
  return (event: React.MouseEvent) => {
    event.preventDefault();
    onMenu(event.clientX, event.clientY);
  };
}

/** The quiet ··· on a bubble; its menu opens under it. The dots come from
 *  CSS so the bubble's text stays the message alone (copy, smoke checks). */
function MoreButton({ onMenu }: { onMenu: MenuAt }) {
  return (
    <button
      type="button"
      className="msg-more"
      aria-label={MESSAGE_MENU_LABEL}
      title={MESSAGE_MENU_LABEL}
      onClick={(event) => {
        const box = event.currentTarget.getBoundingClientRect();
        onMenu(box.left, box.bottom);
      }}
    />
  );
}

const UserBubble = memo(function UserBubble({ text, onMenu }: { text: string; onMenu?: MenuAt }) {
  return (
    <div className="bubble user" data-role="user" onContextMenu={contextMenuAt(onMenu)}>
      {text}
      {onMenu && <MoreButton onMenu={onMenu} />}
    </div>
  );
});

const AssistantBubble = memo(function AssistantBubble({
  text,
  streaming,
  onMenu,
}: {
  text: string;
  streaming: boolean;
  onMenu?: MenuAt;
}) {
  // Capped like the cabin's bubble, so a runaway stream can't stall the page.
  return (
    <div
      className={`bubble assistant${streaming ? " streaming" : ""}`}
      data-role="assistant"
      onContextMenu={contextMenuAt(onMenu)}
    >
      <Markdown text={displayText(text)} />
      {onMenu && <MoreButton onMenu={onMenu} />}
    </div>
  );
});

const ToolCard = memo(function ToolCard({
  title,
  status,
  detail,
}: {
  title: string;
  status: string;
  detail?: string;
}) {
  return (
    <div className="card tool" data-status={status}>
      <span className="card-title">{title}</span>
      <span className="card-status">{status}</span>
      {detail && <pre className="card-detail">{detail}</pre>}
    </div>
  );
});

/** The lead's request to merge a helper's work. It asks every time, so its
 * card has no button that says it will stop asking. */
const MERGE_TOOL = "merge_subagent_work";

function PermissionCard({
  item,
  onAnswer,
  onStopAsking,
}: {
  item: Extract<Item, { kind: "permission" }>;
  onAnswer: (answer: Answer) => void;
  /** Allow this and ask less from now on; absent when it can't change here. */
  onStopAsking?: () => void;
}) {
  const merge = item.title === MERGE_TOOL;
  return (
    <div className="card permission" role="group" aria-label={`Approve ${item.title}`}>
      <div className="card-title">
        {merge ? (
          `${PRODUCT_NAME} wants to merge a helper's work into the project`
        ) : (
          <>
            {PRODUCT_NAME} wants to run <code>{item.title}</code>
          </>
        )}
      </div>
      {item.action && <pre className="card-action">{item.action}</pre>}
      {item.reason && <p className="card-reason">{item.reason}</p>}
      {item.answer ? (
        <p className="card-answer">{ANSWERED[item.answer]}</p>
      ) : (
        <div className="card-buttons">
          <button type="button" onClick={() => onAnswer("allow")}>
            Allow once
          </button>
          {!merge && (
            <button type="button" className="quiet" onClick={() => onAnswer("always")}>
              Always allow
            </button>
          )}
          <button type="button" className="quiet danger" onClick={() => onAnswer("deny")}>
            Deny
          </button>
          {!merge && onStopAsking && (
            <button
              type="button"
              className="link"
              title="Allows this, and from now on only risky things ask. Settings → Defaults changes it back."
              onClick={onStopAsking}
            >
              Stop asking me
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/** One entry in the log. It and Chat are compiled ("use memo"), and Chat passes
 * the same handlers every render, so a reply streaming in redraws only the
 * bubble it changes; a card above it is not drawn again. Cards come in as a
 * transition and fade in where the webview has view transitions. */
function LogItem({
  item,
  busy,
  onMenu,
  onAnswer,
  onAnswerCard,
  onConfirm,
  onStopAsking,
  onFix,
  onChipPick,
  nyxUrl,
  editable,
}: {
  item: Item;
  busy: boolean;
  /** The agent window: a plan card can be edited and sent back. */
  editable: boolean;
  /** The bubble's Delete message… menu; absent where it can't be offered. */
  onMenu?: MenuAt;
  /** Nyx's face beside each reply, when Nyx is on. */
  nyxUrl?: string;
  onAnswer: (id: string, answer: Answer) => void;
  onAnswerCard: (item: ElicitItem, action: ElicitAction, value: string, field?: string) => void;
  /** Yes or Cancel on a slash command's question. */
  onConfirm: (item: Item, yes: boolean) => void;
  /** Stop asking me on a permission card, while the setting is Ask me first. */
  onStopAsking?: (id: string) => void;
  /** A findings pill: put the fix in the composer, unsent. */
  onFix: (text: string) => void;
  /** A reply chip was picked; the shell remembers it for the next card. */
  onChipPick: (key: string) => void;
}) {
  "use memo";
  switch (item.kind) {
    case "user":
      return <UserBubble text={item.text} onMenu={busy ? undefined : onMenu} />;
    case "assistant": {
      const menu = busy || item.streaming ? undefined : onMenu;
      if (nyxUrl)
        return (
          <div className="nyx-row">
            <NyxFace url={nyxUrl} size={32} />
            <AssistantBubble text={item.text} streaming={Boolean(item.streaming)} onMenu={menu} />
          </div>
        );
      return <AssistantBubble text={item.text} streaming={Boolean(item.streaming)} onMenu={menu} />;
    }
    case "tool":
      return <ToolCard title={item.title} status={item.status} detail={item.detail} />;
    case "permission":
      return (
        <PermissionCard
          item={item}
          onAnswer={(a) => onAnswer(item.id, a)}
          onStopAsking={onStopAsking && (() => onStopAsking(item.id))}
        />
      );
    case "elicit":
      return (
        <ElicitCard
          key={item.id}
          item={item}
          editable={editable}
          onAnswer={(action, value, field) => onAnswerCard(item, action, value, field)}
          onChipPick={onChipPick}
        />
      );
    case "findings":
      return <FindingsCard event={item} onFix={onFix} />;
    case "recap":
      return <RecapCard event={item} />;
    case "evidence":
      return <EvidenceCard event={item} />;
    case "slash":
      return (
        <div className="card notice slash-notice" role="status" aria-label={`${item.command} result`}>
          <span className="card-title">{item.command}</span>
          <p className="card-detail-line plain">{item.text}</p>
        </div>
      );
    case "confirm":
      return (
        <div className="card notice slash-notice" role="group" aria-label={`${item.command} asks first`}>
          <span className="card-title">{item.command}</span>
          <p className="card-detail-line plain">{item.prompt}</p>
          <div className="card-buttons">
            <button type="button" className="quiet danger" onClick={() => onConfirm(item, true)}>
              Yes
            </button>
            <button type="button" onClick={() => onConfirm(item, false)}>
              Cancel
            </button>
          </div>
        </div>
      );
    case "saved":
      return <SavedCard saved={item.saved} />;
    case "halted":
      return (
        <p className="halted-line" role="status">
          {HALTED_LINE}
        </p>
      );
    case "tools":
      return (
        <p className="tools-ran" data-role="tools">
          Tools ran: {item.text}
        </p>
      );
  }
}

/** `prefill`: text another page put in the box (Pulse's Run, Journal's To
 * mission). A new `n` replaces the draft; nothing is sent until the person
 * sends it. With `conversationId` the chat moves to that conversation first,
 * like Start over, unless it is already there. `active`: the Chat tab is
 * showing, so the approvals chips read Settings again. `remote`: the web app,
 * where the approvals stay the computer's. Finished automation runs go to the
 * Notifications menu, not this log. `open`: a saved chat History picked;
 * a new `n` shows it, and the next message carries it on. `deleted`: a chat
 * History deleted (`ids`: every chat it has deleted, since Delete all reports
 * many at once); if the open one is among them, a new chat starts. `clear`: the
 * Chat tab's Clear chat, already confirmed; a new `n` deletes the open chat's
 * saved history for good and starts a new one. On the desktop
 * the chat open at the last close comes back on mount. */
export function Chat({
  onOpenSettings,
  onSignIn = onOpenSettings,
  onOpenView,
  onOpenSection,
  onOpenPalette,
  onQueue,
  queueAct,
  prefill,
  open,
  deleted,
  clear,
  active = true,
  remote = false,
  grokSignIn = false,
  embedded = false,
  lead = false,
  onConversation,
  onBusy,
  onRan,
}: {
  onOpenSettings: () => void;
  /** The signed-out card's button: Settings → Account. */
  onSignIn?: () => void;
  /** `/memory`, `/connectors` and the like open their page, and so does a
   * chip that goes somewhere. `false` back: this app has no such page. */
  onOpenView?: (view: View) => boolean | void;
  /** `/plugins` opens Settings → Labs. */
  onOpenSection?: (section: "labs") => void;
  /** `/palette` opens the command palette. */
  onOpenPalette?: () => void;
  /** The messages waiting behind the running reply, for the Activity menu. */
  onQueue?: (conversationId: string, texts: string[]) => void;
  /** Steer now or Remove, pressed on a queued row in the Activity menu. A new
   * `n` runs it once. */
  queueAct?: QueueAct;
  prefill?: Prefill;
  open?: { id: string; n: number };
  deleted?: { id: string; ids?: string[]; n: number };
  clear?: { n: number };
  active?: boolean;
  remote?: boolean;
  /** Settings → Account offers Sign in with Grok here. */
  grokSignIn?: boolean;
  /** The Code tab's agent window: it starts on a new session instead of the
   * last chat, has no dock, and names itself Agent. */
  embedded?: boolean;
  /** The agent window's session has a lead: its plan card can be edited. */
  lead?: boolean;
  /** The conversation on screen, at the start and each time it changes. */
  onConversation?: (conversationId: string) => void;
  /** A reply started or ended. */
  onBusy?: (busy: boolean) => void;
  /** A slash command ran here: it may have moved the app to another folder. */
  onRan?: () => void;
}) {
  "use memo";
  const [conversationId, setConversationId] = useState(newConversationId);
  const [items, setItems] = useState<Item[]>([]);
  const [draft, setDraft] = useState("");
  /** Hangout TALK / TYPE — keeps the draft when switching (card 732). */
  const [hangoutMode, setHangout] = useState<HangoutMode>("type");
  /** Older lines of a reopened chat not shown yet. */
  const [earlier, setEarlier] = useState(0);
  /** The person already moved on, so the last chat must not land on top. */
  const touched = useRef(false);

  const [busy, setBusy] = useState(false);
  /** The chat whose reply is running. Another chat opened meanwhile (Clear
   * chat, or deleting the open one) shows Send, and its messages wait. */
  const [runIn, setRunIn] = useState<string>();
  /** The chat on screen now, read after a reply ends. */
  const openNow = useRef(conversationId);
  useEffect(() => {
    openNow.current = conversationId;
  }, [conversationId]);
  const busyHere = busy && runIn === conversationId;
  const [error, setError] = useState<string | null>(null);
  const busyRef = useRef(false);
  /** Messages sent while a reply runs wait here, in order, and show only in
   * the Activity menu. The next one sends when the reply ends. */
  const [queue, setQueue] = useState<string[]>([]);
  const queueRef = useRef<string[]>([]);
  const voice = useVoiceMode();
  const voiceSession = voice.session;
  /** The queued message whose turn came: it sends after this render. */
  const [nextUp, setNextUp] = useState<{ text: string; n: number; as?: SessionMode }>();
  /** `/plan` or `/chat` named how the next message runs; otherwise the app reads it. */
  const nextAs = useRef<SessionMode | undefined>(undefined);
  /** Questions answered beside the running reply. */
  const [sides, setSides] = useState<Side[]>([]);
  const sideN = useRef(0);
  /** Slash lines sent and not yet answered: Enter again starts no second run. */
  const slashing = useRef(new Set<string>());
  /** The line shown the moment a message typed mid-reply is routed. */
  const [ack, setAck] = useState<string | null>(null);
  /** A reply's follow-ups landed: the chip row is asked for again. */
  const [followed, setFollowed] = useState(0);
  /** The game goal the last reply noticed, asked about under it. */
  const [goalAsk, setGoalAsk] = useState<{ id: string; title: string } | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const attach = useAttachments(conversationId, { remote, active });
  const { rows: slashRows, fresh: slashFresh } = useSlashHits(draft);
  const [slashAt, setSlashAt] = useState(0);
  // Escape hides the list until the draft changes.
  const [slashShut, setSlashShut] = useState<string | null>(null);
  const slashOpen = slashRows.length > 0 && slashShut !== draft;
  const slashPick = Math.min(slashAt, slashRows.length - 1);
  const row = useChipRow({
    surface: "composer",
    // The computer refuses a browser chips for a chat it has not sent in, so
    // the web app's new chat asks for the new-chat row.
    conversationId: remote && !items.some((it) => it.kind === "user") ? undefined : conversationId,
    draft,
    turn: followed,
    busy,
    active,
  });
  // Nyx's pose follows what the chat does, not each chunk of text.
  const activity = activityOf(items, busy);
  const nyx = useNyx(activity, active);

  useEffect(() => {
    if (active) getSettings().then(setSettings, () => {});
  }, [active]);

  /** A bubble's open menu, and the delete waiting on its confirm. */
  const [msgMenu, setMsgMenu] = useState<{ i: number; at: { x: number; y: number } } | null>(null);
  const [askDelete, setAskDelete] = useState<Cut | null>(null);

  function pickDelete(i: number) {
    const cut = cutFor(items, i, earlier);
    if (cut) setAskDelete(cut);
    else setError(DELETE_NEEDS_EARLIER);
  }

  /** The saved chat first: if it refuses, the log keeps the message. */
  async function deleteMessage(cut: Cut) {
    setAskDelete(null);
    try {
      await forgetMessage(conversationId, cut);
      setItems((now) => withoutCut(now, cut));
    } catch (err) {
      setError(String(err));
    }
  }

  /** Side answers belong to the chat on screen: leaving it stops them. */
  function leaveSides() {
    for (const side of sides) if (side.live) void cancelChat(side.id);
    setSides([]);
    setAck(null);
  }

  /** A new conversation; the old one stays saved for History. */
  function changeQueue(next: string[]) {
    queueRef.current = next;
    setQueue(next);
  }

  function startFresh() {
    // Messages lined up in the chat being left go back in the box, never
    // into the new chat by themselves.
    const waiting = queueRef.current;
    if (waiting.length > 0) {
      changeQueue([]);
      setDraft((now) => withQueued(now, waiting));
    }
    leaveSides();
    touched.current = true;
    setGoalAsk(null);
    setItems([]);
    setEarlier(0);
    setError(null);
    setConversationId(newConversationId());
  }

  function show(transcript: Transcript) {
    leaveSides();
    setGoalAsk(null);
    setConversationId(transcript.id);
    setItems(itemsFrom(transcript));
    setEarlier(transcript.earlier);
    setError(null);
  }

  // The saved chat shows only if nothing was typed or picked meanwhile.
  const restoreLast = useEffectEvent((last: Transcript | null) => {
    if (last && !touched.current) show(last);
  });

  useEffect(() => {
    if (remote || embedded) return;
    getLastChat().then(
      (last) => restoreLast(last),
      () => {},
    );
  }, [remote, embedded]);

  useEffect(() => {
    if (remote || embedded) return;
    getHangoutMode().then(setHangout, () => {});
  }, [remote, embedded]);

  function pickHangout(mode: HangoutMode) {
    setHangout(mode);
    setHangoutMode(mode).catch((err) => setError(String(err)));
  }

  useEffect(() => {
    onConversation?.(conversationId);
  }, [conversationId, onConversation]);
  useEffect(() => {
    onBusy?.(busy);
  }, [busy, onBusy]);

  // Only a new pick acts; the conversation it leaves is the latest, not a
  // dependency.
  const openPicked = useEffectEvent((id: string) => {
    if (id === conversationId) return;
    if (busyRef.current) {
      setError(OPEN_BUSY);
      return;
    }
    touched.current = true;
    closeChat(conversationId).catch(() => {});
    loadChat(id).then(show, (err) => setError(String(err)));
  });

  useEffect(() => {
    if (open) openPicked(open.id);
  }, [open]);

  // Deleting the chat on screen starts a new one; deleting another leaves it.
  const dropDeleted = useEffectEvent((ids: string[]) => {
    if (ids.includes(conversationId)) startFresh();
  });

  useEffect(() => {
    if (!deleted) return;
    const ids = deleted.ids ?? [deleted.id];
    // After this render, so the reset is one render of its own.
    const timer = window.setTimeout(() => dropDeleted(ids), 0);
    return () => window.clearTimeout(timer);
  }, [deleted]);

  // Clear chat: stop any reply, delete the open chat's saved history, start over.
  const clearOpen = useEffectEvent(() => {
    const id = conversationId;
    touched.current = true;
    deleteChat(id).then(startFresh, (err) => setError(String(err)));
  });

  useEffect(() => {
    if (clear) clearOpen();
  }, [clear]);

  /** Reloads the saved chat with more of its start. The log has at least as
   * many items as the saved lines it shows, so nothing shown drops off. */
  async function showEarlier() {
    if (busyRef.current) return;
    try {
      show(await loadChat(conversationId, items.length + EARLIER_STEP));
    } catch (err) {
      setError(String(err));
    }
  }

  // Only a new prefill acts; switching conversations must not re-run it, so the
  // conversation it reads is the latest, not a dependency.
  const applyPrefill = useEffectEvent((next: Prefill) => {
    const to = next.conversationId;
    const switched = !!to && to !== conversationId;
    if (switched) {
      if (busyRef.current) {
        setError(MISSION_BUSY);
        return;
      }
      touched.current = true;
      closeChat(conversationId).catch(() => {});
      leaveSides();
      setGoalAsk(null);
      setItems([]);
      setEarlier(0);
      setError(null);
      setConversationId(to);
    }
    if (next.send && next.text) {
      touched.current = true;
      if (busyRef.current) changeQueue([...queueRef.current, next.text]);
      else setNextUp((now) => ({ text: next.text, n: (now?.n ?? 0) + 1, as: "chat" }));
      return;
    }
    // Opening the chat that is already open brings no text: keep what is typed.
    setDraft((now) => (switched || next.text ? next.text : now));
  });
  useEffect(() => {
    if (prefill) applyPrefill(prefill);
  }, [prefill]);

  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    let stopGap: (() => void) | undefined;
    const sidePrefix = `${conversationId}${SIDE_MARK}`;
    void onChatEvent(({ conversationId: id, event, helper }) => {
      // A side answer streams into its own card, never the chat.
      if (id.startsWith(sidePrefix)) {
        if (event.kind === "text")
          setSides((now) => now.map((s) => (s.id === id ? { ...s, answer: s.answer + event.text } : s)));
        else if (event.kind === "error") setSides((now) => now.map((s) => (s.id === id ? { ...s, failed: true } : s)));
        return;
      }
      // A helper's question shows whether or not a reply runs.
      if (helper) {
        if (id === conversationId) setItems((now) => withHelperAsk(now, event));
        return;
      }
      // An event that lands after the turn ended must not reopen it.
      if (id !== conversationId || !busyRef.current) return;
      if (event.kind === "error") setError(event.message);
      else if (event.kind === "suggestions") setFollowed((n) => n + 1);
      else if (event.kind === "goal") setGoalAsk({ id: event.id, title: event.title });
      else if (event.kind === "done") setItems(event.stop === HALTED_STOP ? settleHalted : settle);
      else if (event.kind === "text") {
        voiceSession.feed(event.text);
        setItems((now) => applyEvent(now, event));
      }
      // Not a transition: one would hold the page's next updates while the
      // card eases in, and typing in the box then loses its first letters.
      else setItems((now) => applyEvent(now, event));
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    helperQuestions(conversationId)
      .then((asks) => {
        if (!dropped && asks.length > 0) setItems((now) => asks.reduce(withHelperAsk, now));
      })
      .catch(() => {});
    void onEventGap(() => {
      if (busyRef.current) setError(EVENTS_LOST);
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stopGap = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
      stopGap?.();
    };
  }, [conversationId, voiceSession]);

  useEffect(() => {
    endRef.current?.scrollIntoView?.({ block: "end" });
  }, [items]);

  function startOver() {
    if (busy) return;
    // A browser's chat with no message sent is refused; the new chat starts anyway.
    closeChat(conversationId).catch(() => {});
    startFresh();
  }

  /** Sends `text` and `files`. `as` (`/plan <text>`, a plan's follow-up chip)
   * names how it runs; otherwise `/plan` or `/chat` before it did, or the app
   * reads the message. `fromDraft` empties the box and its files; a tapped
   * chip leaves them. `again` sends a paused ask once more: it is already
   * on screen, so it adds no line. */
  async function deliver(
    text: string,
    files: typeof attach.files,
    fromDraft: boolean,
    as?: SessionMode,
    again = false,
  ) {
    if ((!text && files.length === 0) || busy) return;
    touched.current = true;
    setGoalAsk(null);
    const unread = files.find((f) => f.status);
    if (unread) {
      setError(`${unread.name}: ${unread.status}`);
      return;
    }
    const said = [text, ...files.map((f) => f.label)].filter(Boolean).join("\n");
    if (!again) setItems((now) => [...now, { kind: "user", text: said }]);
    if (fromDraft) {
      setDraft("");
      attach.clear();
    }
    setError(null);
    setAck(null);
    busyRef.current = true;
    setBusy(true);
    const from = conversationId;
    setRunIn(from);
    const runs = as ?? nextAs.current;
    nextAs.current = undefined;
    let stop = "";
    try {
      const paths = files.map((f) => f.path);
      stop = await (paths.length ? sendChat(conversationId, text, runs, paths) : sendChat(conversationId, text, runs));
    } catch (err) {
      setError(String(err));
    }
    // After the catch rather than in a finally, which React Compiler can't compile yet.
    busyRef.current = false;
    setBusy(false);
    setRunIn(undefined);
    voiceSession.replyDone("");
    // Another chat opened meanwhile: what waits now was typed there, so it
    // goes, and the lines on screen are that chat's, not this reply's.
    const moved = openNow.current !== from;
    if (!moved) setItems(stop === HALTED_STOP ? settleHalted : settle);
    // The reply ended well: the next queued message goes. Halt, Stop or a
    // failure sends nothing more; what waited goes back in the box.
    const waiting = queueRef.current;
    if (waiting.length === 0) return;
    if (moved || (stop && stop !== HALTED_STOP && stop !== "cancelled")) {
      changeQueue(waiting.slice(1));
      setNextUp((now) => ({ text: waiting[0], n: (now?.n ?? 0) + 1 }));
    } else {
      changeQueue([]);
      setDraft((now) => withQueued(now, waiting));
    }
  }

  // No model in your plan answered: once one is back, this chat's paused
  // ask goes again by itself, once, with a quiet note naming who answers.
  const [modelBack, setModelBack] = useState(0);
  const resumeAsk = useEffectEvent((back: ModelResume) => {
    void deliver(back.ask, [], false, undefined, true);
    setAck(back.note);
  });
  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    void onModelBack(() => setModelBack((n) => n + 1)).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, []);
  useEffect(() => {
    if (busy) return;
    let dropped = false;
    modelResume(conversationId)
      .then((back) => {
        if (!dropped && back) resumeAsk(back);
      })
      .catch(() => {});
    return () => {
      dropped = true;
    };
  }, [modelBack, conversationId, busy]);

  // Voice mode: what it hears goes as a message, or waits behind a running
  // reply; what it has to say shows where errors do.
  const showVoiceNote = (note: string) => setError(note);
  useEffect(() =>
    voiceSession.handlers((words) => {
      if (busyRef.current) changeQueue([...queueRef.current, words]);
      else void deliver(words, [], false);
    }, showVoiceNote),
  );

  /** Stop: the reply ends, and the messages lined up behind it come back to
   * the box instead of sending. */
  function stopReply() {
    const waiting = queueRef.current;
    if (waiting.length > 0) {
      changeQueue([]);
      setDraft((now) => withQueued(now, waiting));
    }
    void cancelChat(conversationId);
  }

  /** Remove puts the words back in an empty box, so nothing typed is lost. */
  function removeQueued(index: number) {
    const text = queueRef.current[index];
    if (text === undefined) return;
    changeQueue(queueRef.current.filter((_, i) => i !== index));
    setDraft((now) => (now.trim() ? now : text));
  }

  /** Steer now: the words join the running reply at its next step and show
   * in the chat as sent. If the reply ended first they stay in line. */
  async function steerQueued(index: number) {
    const text = queueRef.current[index];
    if (text === undefined) return;
    let took: boolean;
    try {
      took = await steerChat(conversationId, text);
    } catch (err) {
      setError(String(err));
      return;
    }
    if (!took || queueRef.current[index] !== text) return;
    changeQueue(queueRef.current.filter((_, i) => i !== index));
    setItems((now) => [...now, { kind: "user", text }]);
  }

  async function answer(id: string, choice: Answer) {
    const mark = (from: Answer | null, to: Answer | null) =>
      setItems((now) =>
        now.map((item) =>
          item.kind === "permission" && item.id === id && item.answer === from
            ? { ...item, answer: to }
            : item,
        ),
      );
    mark(null, choice);
    try {
      await answerPermission(conversationId, id, choice);
    } catch (err) {
      // It never reached the agent, which is still waiting: give the buttons back.
      mark(choice, null);
      setError(String(err));
    }
  }

  /** Stop asking me: allow this step, and Ask me first becomes Only ask for
   * risky things. Settings → Defaults is where it changes back. */
  async function stopAsking(id: string) {
    if (!settings || settings.approvals !== "ask") return;
    void answer(id, "allow");
    try {
      // Read again first: a typed setting ask or the cabin may have changed
      // the other settings since this chat loaded them.
      const now = await getSettings();
      setSettings(await saveSettings(now.closeToTray, now.workspace, "auto", now.desktopControl));
      setAck(STOP_ASKING_DONE);
    } catch (err) {
      setError(String(err));
    }
  }

  async function answerCard(item: ElicitItem, action: ElicitAction, value: string, field?: string) {
    const mark = (from: ElicitAction | null, to: ElicitAction | null) =>
      setItems((now) =>
        now.map((it) =>
          it.kind === "elicit" && it.id === item.id && it.answer === from ? { ...it, answer: to } : it,
        ),
      );
    mark(null, action);
    try {
      await answerElicit(conversationId, item.id, action, field ?? item.fieldName, value);
    } catch (err) {
      // It never reached the agent, which is still asking: give the card back.
      mark(action, null);
      setError(String(err));
    }
  }

  /** A question typed during a reply gets its own read-only answer in an
   * "On the side" card; the reply, its chat and its history stay as they are. */
  async function askOnTheSide(question: string) {
    sideN.current += 1;
    const id = `${conversationId}${SIDE_MARK}${sideN.current.toString(36)}`;
    const ask = [...items].reverse().find((item) => item.kind === "user");
    const reply = items.at(-1);
    const soFar = reply?.kind === "assistant" ? reply.text : "";
    setSides((now) => withSide(now, { id, question, answer: "", live: true, failed: false }));
    let failed = false;
    try {
      await sideChat(conversationId, id, question, ask?.kind === "user" ? ask.text : "", soFar);
    } catch {
      failed = true;
    }
    setSides((now) => now.map((s) => (s.id === id ? { ...s, live: false, failed: s.failed || failed } : s)));
  }

  function stopSide(side: Side) {
    void cancelChat(side.id);
  }

  function dismissSide(side: Side) {
    setSides((now) => now.filter((s) => s.id !== side.id));
  }

  /** Tell the main task: the side answer joins the running reply at its next
   * step, waits its turn behind it, or goes as the next message. */
  async function tellSide(side: Side) {
    dismissSide(side);
    try {
      const told = await tellMain(conversationId, side.question, side.answer);
      if (told.steered) setItems((now) => [...now, { kind: "user", text: told.text }]);
      else if (busyRef.current) changeQueue([...queueRef.current, told.text]);
      else void deliver(told.text, [], false, "chat");
    } catch (err) {
      setError(String(err));
    }
  }

  /** A message typed while a reply runs, as the app read it: a side
   * question, a correction for the work, stop, or a new job that waits. */
  async function routeRunning(typed: string, read: TurnRead) {
    const text = read.text.trim() || typed;
    setAck(read.ack || null);
    switch (read.route) {
      case "side":
        void askOnTheSide(text);
        return;
      case "stop":
        stopReply();
        return;
      case "steer": {
        const took = await steerChat(conversationId, text).catch(() => false);
        if (took) {
          setItems((now) => [...now, { kind: "user", text }]);
          return;
        }
        // The reply ended first: the words wait their turn instead.
        break;
      }
      case "queue":
        break;
    }
    changeQueue([...queueRef.current, text]);
  }

  /** Runs a slash line and applies what it did. `box` is what the box held
   * when the line was sent. */
  async function slash(line: string, box = line) {
    if (slashing.current.has(line)) {
      setAck(SLASH_RUNNING);
      return;
    }
    setError(null);
    slashing.current.add(line);
    let effect: SlashEffect;
    try {
      effect = await runSlash(line, conversationId);
    } catch (err) {
      slashing.current.delete(line);
      setAck((now) => (now === SLASH_RUNNING ? null : now));
      setError(String(err));
      return;
    }
    slashing.current.delete(line);
    onRan?.();
    setAck((now) => (now === SLASH_RUNNING ? null : now));
    // Only the line that was sent leaves the box: words typed while it ran stay.
    setDraft((now) => (now.trim() === box.trim() ? "" : now));
    const command = line.split(/\s+/)[0];
    switch (effect.kind) {
      case "newChat":
        startOver();
        break;
      case "mode": {
        const text = effect.text?.trim();
        if (effect.mode === "btw") {
          // A typed side question: beside the reply when one runs, else just answered.
          if (!text) setItems((now) => [...now, { kind: "slash", command, text: BTW_HINT }]);
          else if (busyRef.current && !remote) void askOnTheSide(text);
          else void deliver(text, [], false, "chat");
        } else if (text) {
          void deliver(text, [], false, effect.mode);
        } else {
          nextAs.current = effect.mode;
          setItems((now) => [
            ...now,
            { kind: "slash", command, text: effect.mode === "plan" ? PLAN_NEXT : CHAT_NEXT },
          ]);
        }
        break;
      }
      case "open":
        if (onOpenView?.(effect.view) === false) {
          setItems((now) => withNotice(now, { kind: "slash", command, text: `${command} works on your computer.` }));
        }
        break;
      case "imagine":
        if (onOpenView?.("imagine") === false) {
          setItems((now) => withNotice(now, { kind: "slash", command, text: `${command} works on your computer.` }));
        } else {
          requestImagine(effect.media, effect.prompt);
        }
        break;
      case "openSection":
        if (onOpenSection) onOpenSection(effect.section);
        else onOpenView?.("settings");
        break;
      case "import": {
        // Review opens with the workspace's notes; with none to read, the chat says so.
        const text = await openImport();
        if (text !== null) setItems((now) => withNotice(now, { kind: "slash", command, text }));
        break;
      }
      case "notice":
        setItems((now) => withNotice(now, { kind: "slash", command, text: effect.text }));
        break;
      case "export":
      case "exportAs":
        try {
          const saved =
            effect.kind === "export"
              ? await exportChat(conversationId)
              : await exportChat(conversationId, effect.format);
          setItems((now) => [...now, { kind: "saved", saved }]);
        } catch (err) {
          setError(String(err));
        }
        break;
      case "retry": {
        const last = [...items].reverse().find((it) => it.kind === "user");
        if (last?.kind === "user") void deliver(last.text, [], false);
        else setItems((now) => [...now, { kind: "slash", command, text: "Nothing to retry" }]);
        break;
      }
      case "undo":
        if (busyRef.current) {
          void cancelChat(conversationId);
          setItems((now) => [...now, { kind: "slash", command, text: "Undid in-flight reply" }]);
        } else {
          setItems((now) => [...now, { kind: "slash", command, text: UNDO_NA }]);
        }
        break;
      case "stop": {
        const running = busyRef.current;
        if (running) stopReply();
        setItems((now) => [...now, { kind: "slash", command, text: running ? "Stopped" : "Nothing is running" }]);
        break;
      }
      case "compact":
        void deliver("/compact", [], false, "chat");
        break;
      case "confirm":
        setItems((now) => [...now, { kind: "confirm", command, prompt: effect.prompt, yes: effect.yes }]);
        break;
      case "deleted":
        startFresh();
        setItems([{ kind: "slash", command, text: `Deleted ${effect.title}` }]);
        break;
      case "queue":
        if (busyRef.current) changeQueue([...queueRef.current, effect.text]);
        // Sent from the next render: the reply may have ended while the line
        // was read, and this render's `deliver` still sees it running.
        else setNextUp((now) => ({ text: effect.text, n: (now?.n ?? 0) + 1, as: "chat" }));
        break;
      case "palette":
        onOpenPalette?.();
        break;
      case "viewPlan": {
        const plan = [...items].reverse().find((it) => it.kind === "elicit" && it.server === "Plan");
        const shown = plan?.kind === "elicit" ? plan.message : NO_PLAN;
        // The agent window's plan is saved with the session, edits included.
        const saved = embedded ? await codeSessionPlan(conversationId).catch(() => null) : null;
        setItems((now) => [...now, { kind: "slash", command, text: saved || shown }]);
        break;
      }
      case "loop":
        try {
          await addAutomation(effect.seed);
          setItems((now) => [...now, { kind: "slash", command, text: `Scheduled: ${effect.seed}` }]);
          onOpenView?.("automations");
        } catch (err) {
          setItems((now) => [...now, { kind: "slash", command, text: String(err) }]);
        }
        break;
      case "openNote":
        setItems((now) => [...now, { kind: "slash", command, text: effect.text }]);
        onOpenView?.(effect.view);
        break;
      default: {
        const _unhandled: never = effect;
        return _unhandled;
      }
    }
  }

  /** Yes runs the command's own `yes` line; Cancel drops the question. */
  function settleConfirm(item: Item, yes: boolean) {
    if (item.kind !== "confirm") return;
    setItems((now) => now.filter((it) => it !== item));
    if (yes) void slash(item.yes);
    else setItems((now) => [...now, { kind: "slash", command: item.command, text: "Cancelled. Nothing changed." }]);
  }

  /** Tab fills a row in; Enter or a click runs one that takes no text. */
  function pickSlash(hit: SlashHit | undefined, run: boolean) {
    if (!hit) return;
    if (hit.disabled) {
      setError(`${hit.cmd}: ${hit.disabled}`);
      return;
    }
    if (!run || !hit.runOnPick) {
      setDraft(hit.insert);
      return;
    }
    // The same rule as a typed line: a command that waits for the reply says so.
    if (busy && !runsBesideReply(hit.cmd, remote)) {
      setError(remote ? SLASH_WAITS_REMOTE : SLASH_WAITS);
      return;
    }
    void slash(hit.cmd, draft);
  }

  // Declared after what they call: React Compiler skips a component that
  // calls a function declared further down.
  /** Sends the draft. A slash line runs instead of going to the model. */
  async function send(event?: FormEvent) {
    event?.preventDefault();
    const text = draft.trim();
    const files = attach.files;
    // While a reply runs, the app reads what the message is for (files wait
    // in the box). A typed /btw or /side asks on the side.
    if (busy) {
      if (runsBesideReply(text, remote)) {
        await slash(text);
        return;
      }
      if (!text) return;
      if (isSlashCommand(text) && !/^\/(btw|side)\s+\S/i.test(text)) {
        setError(remote ? SLASH_WAITS_REMOTE : SLASH_WAITS);
        return;
      }
      setDraft("");
      // The web app lines messages up behind the reply, as before. In a chat
      // opened while another chat's reply ends, the message waits for it.
      if (remote || !busyHere) {
        changeQueue([...queueRef.current, text]);
        return;
      }
      let read: TurnRead;
      try {
        read = await readTurn(text, conversationId);
      } catch {
        read = { route: "queue", text, ack: "" };
      }
      await routeRunning(text, read);
      return;
    }
    if (!text && files.length === 0) return;
    touched.current = true;
    if (isSlashCommand(text)) {
      await slash(text);
      return;
    }
    // "make the text bigger", "undo that": a setting, answered here.
    const said = files.length === 0 && !remote ? await settingAsk(text) : null;
    if (said) {
      setDraft("");
      setItems((now) => [...now, { kind: "user", text }, { kind: "assistant", text: said, streaming: false }]);
      return;
    }
    await deliver(text, files, true);
  }

  /** One tap rule per chip (`grokhub_core::ChipAction`): Go opens its page;
   * Ask sends its text when the box is empty and adds it to what is typed
   * otherwise. A follow-up from a plan's reply plans again. */
  function tapChip(chip: QuickChip) {
    row.picked(chip);
    if (chip.action === "go") {
      if (chip.view) onOpenView?.(chip.view as View);
      return;
    }
    const as = chip.mode === "plan" ? "plan" : undefined;
    if (draft.trim()) {
      setDraft(appendDraft(draft, chip.value));
      return;
    }
    touched.current = true;
    if (isSlashCommand(chip.value)) void slash(chip.value);
    else void deliver(chip.value, [], false, as);
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (slashOpen) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : -1;
        setSlashAt((slashPick + step + slashRows.length) % slashRows.length);
        return;
      }
      if (event.key === "Tab" && !event.shiftKey) {
        event.preventDefault();
        // A list still answering an earlier keystroke fills nothing in.
        if (slashFresh) pickSlash(slashRows[slashPick], false);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        setSlashShut(draft);
        return;
      }
      // Until the list answers this draft, Enter runs the line as typed.
      if (event.key === "Enter" && !event.shiftKey && slashFresh) {
        const hit = slashRows[slashPick];
        // A typed full command runs as typed; otherwise the pick is used.
        if (hit && hit.cmd !== draft.trim().toLowerCase()) {
          event.preventDefault();
          pickSlash(hit, true);
          return;
        }
      }
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void send();
    }
  }

  // The "…" bubble shows only while nothing else in the log is moving.
  const last = items[items.length - 1];
  const waiting =
    !(last?.kind === "assistant" && last.streaming) &&
    !(last?.kind === "permission" && last.answer === null) &&
    !(last?.kind === "elicit" && last.answer === null);

  useEffect(() => {
    onQueue?.(conversationId, queue);
  }, [conversationId, queue, onQueue]);
  const actN = useRef<number>(undefined);
  const applyQueueAct = useEffectEvent((act: QueueAct) => {
    if (act.n === actN.current) return;
    actN.current = act.n;
    if (act.op === "remove") removeQueued(act.index);
    else void steerQueued(act.index);
  });
  const sendNext = useEffectEvent((text: string, as?: SessionMode) => void deliver(text, [], false, as));
  useEffect(() => {
    if (!nextUp) return;
    const { text, as } = nextUp;
    const timer = window.setTimeout(() => sendNext(text, as), 0);
    return () => window.clearTimeout(timer);
  }, [nextUp]);
  useEffect(() => {
    if (!queueAct) return;
    const act = queueAct;
    // After this render, as Delete does, so the change is a render of its own.
    const timer = window.setTimeout(() => applyQueueAct(act), 0);
    return () => window.clearTimeout(timer);
  }, [queueAct]);

  return (
    <section
      className={embedded ? "chat embedded" : nyx ? "chat has-nyx" : "chat"}
      aria-label={embedded ? "Agent" : "Chat"}
      onDragOver={attach.onDragOver}
      onDrop={attach.onDrop}
    >
      {nyx && !embedded && <NyxDock url={nyx.url} mood={nyx.mood} />}
      {!remote && !embedded && (
        <>
          <ChatTabs
            activeId={conversationId}
            draftEmpty={items.length === 0}
            onSwitch={(id) => {
              if (id === conversationId) return;
              if (busyRef.current) {
                setError(OPEN_BUSY);
                return;
              }
              touched.current = true;
              closeChat(conversationId).catch(() => {});
              loadChat(id).then(show, (err) => setError(String(err)));
            }}
            onNew={startOver}
            onClear={() => {
              const id = conversationId;
              touched.current = true;
              deleteChat(id).then(startFresh, (err) => setError(String(err)));
            }}
            onError={setError}
          />
          <HangoutHeader mode={hangoutMode} onMode={pickHangout} />
        </>
      )}
      <Inbox
        onOpenChat={(id) => {
          if (id === conversationId) return;
          if (busyRef.current) {
            setError(OPEN_BUSY);
            return;
          }
          touched.current = true;
          closeChat(conversationId).catch(() => {});
          loadChat(id).then(show, (err) => setError(String(err)));
        }}
      />
      {/* Focusable so the keyboard can scroll a long conversation. */}
      <div className="log" role="log" aria-live="polite" tabIndex={0}>
        {items.length === 0 && !busy && settings && !settings.hasApiKey && !settings.signedIn ? (
          <SignedOut remote={remote} grok={grokSignIn} onSignIn={onSignIn} />
        ) : (
          items.length === 0 &&
          !busy && (
            <div className="nyx-empty">
              {nyx && <NyxFace url={nyx.url} size={96} />}
              <p className="empty">{embedded ? AGENT_EMPTY : CHAT_EMPTY}</p>
            </div>
          )
        )}
        {earlier > 0 && (
          <button
            type="button"
            className="link earlier"
            disabled={busy}
            onClick={() => void showEarlier()}
          >
            Show earlier messages ({earlier})
          </button>
        )}
        {items.map((item, i) => (
          <LogItem
            key={i}
            item={item}
            busy={busy}
            onMenu={remote ? undefined : (x, y) => setMsgMenu({ i, at: { x, y } })}
            nyxUrl={nyx?.url}
            onAnswer={answer}
            onAnswerCard={answerCard}
            editable={embedded && lead}
            onConfirm={settleConfirm}
            onStopAsking={!remote && settings?.approvals === "ask" ? stopAsking : undefined}
            onFix={(text) => setDraft(text)}
            onChipPick={(key) => {
              void Promise.resolve()
                .then(() => replyPick(key))
                .catch(() => {});
            }}
          />
        ))}
        {busy && waiting && (
          <div className="bubble assistant streaming" data-role="assistant">
            <Orb state={orbFor(activity)} label={orbLabel(activity)} />
          </div>
        )}
        <div ref={endRef} />
      </div>
      {msgMenu && (
        <MessageMenu at={msgMenu.at} onDelete={() => pickDelete(msgMenu.i)} onClose={() => setMsgMenu(null)} />
      )}
      {askDelete && (
        <DeleteMessageDialog
          reply={askDelete.reply}
          onConfirm={() => void deleteMessage(askDelete)}
          onClose={() => setAskDelete(null)}
        />
      )}
      {error && (
        <div className="error" role="alert">
          <span>{error}</span>
          {needsSettings(error) && (
            <button type="button" className="link" onClick={onOpenSettings}>
              Open Settings
            </button>
          )}
        </div>
      )}
      {ack && (
        <p className="note route-ack" role="status">
          {ack}
        </p>
      )}
      {embedded && queue.length > 0 && (
        <p className="note" role="status">
          {waitingLine(queue.length)}
        </p>
      )}
      <SideCards sides={sides} onStop={stopSide} onTell={(side) => void tellSide(side)} onDismiss={dismissSide} />
      <Episode
        key={conversationId}
        conversationId={conversationId}
        desktopOn={!remote && Boolean(settings?.desktopControl)}
        onResume={(text) => {
          // A reply still running or ending here takes the resume when it
          // ends (a send now would be dropped); otherwise it goes at once.
          if (busyRef.current) changeQueue([text, ...queueRef.current]);
          else setNextUp((now) => ({ text, n: (now?.n ?? 0) + 1 }));
        }}
      />
      <ScreenshotOffer
        conversationId={conversationId}
        active={active}
        remote={remote}
        busy={busy}
        onAttached={(outcome, prompt) => {
          attach.adopt([outcome]);
          if (outcome.staged) setDraft((now) => (now.trim() ? now : prompt));
        }}
      />
      <AttachChips files={attach.files} note={attach.note} onRemove={attach.remove} busy={busy} />
      <form className="composer slash-wrap" onSubmit={send}>
        {slashOpen && (
          <SlashMenu
            hits={slashRows}
            at={slashPick}
            onHover={setSlashAt}
            onPick={(hit) => pickSlash(hit, true)}
          />
        )}
        {!remote && (
          <AttachButton onPick={() => void attach.pick()} busy={busy} full={attach.files.length >= ATTACH_MAX} />
        )}
        <textarea
          aria-label={embedded ? AGENT_PLACEHOLDER : "Message"}
          placeholder={embedded ? AGENT_PLACEHOLDER : PLACEHOLDER}
          value={draft}
          rows={2}
          role={slashOpen ? "combobox" : undefined}
          aria-expanded={slashOpen ? true : undefined}
          aria-controls={slashOpen ? "slash-menu" : undefined}
          aria-activedescendant={slashOpen ? `slash-row-${slashPick}` : undefined}
          onChange={(e) => {
            setDraft(e.target.value);
            setSlashAt(0);
          }}
          onKeyDown={onKeyDown}
          onPaste={attach.onPaste}
        />
        {!remote && (
          <>
            <Mic onText={(words) => setDraft((now) => appendDraft(now, words))} onError={setError} busy={busy} active={active} />
            <VoiceButton state={voice.state} session={voice.session} />
          </>
        )}
        {busyHere ? (
          <>
            {draft.trim() && !isSlashCommand(draft.trim()) && (
              <button
                type="submit"
                className="quiet"
                title="A side question is answered beside this reply, a correction steers it, and a new job waits its turn"
              >
                Send
              </button>
            )}
            <button type="button" onClick={stopReply}>
              Stop
            </button>
          </>
        ) : (
          <button type="submit" disabled={!draft.trim() && (busy || attach.files.length === 0)}>
            Send
          </button>
        )}
        <button type="button" className="quiet" onClick={startOver} disabled={busy}>
          {embedded ? "New session" : "New chat"}
        </button>
      </form>
      {goalAsk && !busy ? (
        <GoalAsk key={goalAsk.id} id={goalAsk.id} title={goalAsk.title} onDone={() => setGoalAsk(null)} />
      ) : null}
      {!remote && <BudgetCard busy={busy} onOpenSettings={onOpenSettings} />}
      <ChipRow chips={row.chips} busy={busy} keys={active} onTap={tapChip} onHide={row.hide} />
    </section>
  );
}
