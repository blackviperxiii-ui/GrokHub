import {
  memo,
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
  ViewTransition,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import {
  answerElicit,
  answerPermission,
  cancelChat,
  closeChat,
  deleteChat,
  forgetMessage,
  getLastChat,
  getSettings,
  loadChat,
  setModel,
  onChatEvent,
  onEventGap,
  runSlash,
  saveSettings,
  sendChat,
  type Answer,
  type Approvals,
  type ChatEvent,
  type ElicitAction,
  type SessionMode,
  type Settings,
  type SlashEffect,
  type SlashHit,
  type Suggestion,
  type Transcript,
} from "./api";
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
import type { RunNotice } from "./Automations";
import { Markdown } from "./Markdown";
import { displayText } from "./md";
import { appendDraft, Mic } from "./Mic";
import { ElicitCard, ModeChips, type ElicitItem } from "./Modes";
import { NyxDock } from "./NyxDock";
import { NyxFace } from "./NyxFace";
import { activityOf, useNyx } from "./nyx";
import { PRODUCT_NAME } from "./brand";
import { onArrowFocus, usePeek } from "./peek";
import { isSlashCommand, SlashMenu, useSlashHits } from "./SlashMenu";
import { needsSettings } from "./ui";

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
    }
  | { kind: "suggest"; items: Suggestion[] }
  /** A saved chat's tool steps, named on one muted line instead of cards. */
  | { kind: "tools"; text: string }
  | ({ kind: "notice" } & RunNotice)
  /** Halt stopped this reply. */
  | { kind: "halted" }
  /** What a slash command did. It stays in the log and never reaches the model. */
  | { kind: "slash"; command: string; text: string }
  | ElicitItem;

type Ended = "ended" | "halted";

/** The stop reason of a turn Halt ended (`agent::HALTED`). */
export const HALTED_STOP = "halted";
export const HALTED_LINE = "Halted. This reply stopped, and nothing more ran.";

/** How much of a run's reply its notice shows; Automations has the rest. */
export const NOTICE_CHARS = 280;

/** At most this many follow-ups show under a reply. */
export const SUGGESTIONS_MAX = 3;

/** The next steps under a reply: one row that scrolls sideways when it runs
 * out of room, with the next chip peeking in. Left and Right move along it. */
function SuggestRow({
  items,
  busy,
  onPick,
}: {
  items: Suggestion[];
  busy: boolean;
  onPick: (prompt: string) => void;
}) {
  const row = useRef<HTMLDivElement>(null);
  usePeek(row, "x", items.length);
  return (
    <div className="card suggest" role="group" aria-label="Suggestions">
      <div ref={row} className="card-buttons" onKeyDown={(e) => onArrowFocus(e, "x")}>
        {items.map((s) => (
          <button
            key={s.label}
            type="button"
            className="quiet"
            title={s.prompt}
            disabled={busy}
            onClick={() => onPick(s.prompt)}
          >
            {s.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export interface Prefill {
  text: string;
  n: number;
  conversationId?: string;
}

const PLACEHOLDERS: Record<SessionMode, string> = {
  chat: `Message ${PRODUCT_NAME}`,
  plan: "Describe what to plan",
  btw: "Ask on the side",
};

export const CHAT_EMPTY = `Ask ${PRODUCT_NAME} anything.`;
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
    case "suggestions":
      if (event.items.length === 0) return items;
      return [...items, { kind: "suggest", items: event.items.slice(0, SUGGESTIONS_MAX) }];
    default:
      return items;
  }
}

/** The turn ended: stop streaming, and retire cards nobody answered. */
function settle(items: Item[], how: Ended = "ended"): Item[] {
  return items.map((item) => {
    if (item.kind === "assistant" && item.streaming) return { ...item, streaming: false };
    if (item.kind === "permission" && item.answer === null) return { ...item, answer: how };
    if (item.kind === "elicit" && item.answer === null) return { ...item, answer: how };
    return item;
  });
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

/** The quiet ··· on a bubble; its menu opens under it. */
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
    >
      ···
    </button>
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

function PermissionCard({
  item,
  onAnswer,
}: {
  item: Extract<Item, { kind: "permission" }>;
  onAnswer: (answer: Answer) => void;
}) {
  return (
    <div className="card permission" role="group" aria-label={`Approve ${item.title}`}>
      <div className="card-title">
        {PRODUCT_NAME} wants to run <code>{item.title}</code>
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
          <button type="button" className="quiet" onClick={() => onAnswer("always")}>
            Always allow
          </button>
          <button type="button" className="quiet danger" onClick={() => onAnswer("deny")}>
            Deny
          </button>
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
  onPick,
  onDraft,
  onOpenAutomations,
  nyxUrl,
}: {
  item: Item;
  busy: boolean;
  /** The bubble's Delete message… menu; absent where it can't be offered. */
  onMenu?: MenuAt;
  /** Nyx's face beside each reply, when Nyx is on. */
  nyxUrl?: string;
  onAnswer: (id: string, answer: Answer) => void;
  onAnswerCard: (item: ElicitItem, action: ElicitAction, value: string) => void;
  onPick: (prompt: string) => void;
  onDraft: (text: string) => void;
  onOpenAutomations?: () => void;
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
      return (
        <ViewTransition>
          <ToolCard title={item.title} status={item.status} detail={item.detail} />
        </ViewTransition>
      );
    case "permission":
      return (
        <ViewTransition>
          <PermissionCard item={item} onAnswer={(a) => onAnswer(item.id, a)} />
        </ViewTransition>
      );
    case "elicit":
      return (
        <ViewTransition>
          <ElicitCard item={item} onAnswer={(action, value) => onAnswerCard(item, action, value)} />
        </ViewTransition>
      );
    case "notice":
      return (
        <div
          className={item.ok ? "card notice" : "card notice warn"}
          role="status"
          aria-label={`${item.name} finished`}
        >
          <span className="card-title">{item.name}</span>
          <span className="card-status">{item.ok ? "Finished" : "Failed"}</span>
          <p className="card-detail-line plain">
            {item.text.length > NOTICE_CHARS
              ? `${item.text.slice(0, NOTICE_CHARS).trimEnd()}…`
              : item.text}
          </p>
          <div className="card-buttons">
            <button
              type="button"
              className="quiet"
              onClick={() =>
                onDraft(
                  item.ok
                    ? `About the "${item.name}" automation run: `
                    : `The "${item.name}" automation failed: ${item.text}. What went wrong and how do I fix it?`,
                )
              }
            >
              {item.ok ? "Ask about it" : "Find the cause"}
            </button>
            {onOpenAutomations && (
              <button type="button" className="link" onClick={onOpenAutomations}>
                Open Automations
              </button>
            )}
          </div>
        </div>
      );
    case "slash":
      return (
        <div className="card notice slash-notice" role="status" aria-label={`${item.command} result`}>
          <span className="card-title">{item.command}</span>
          <p className="card-detail-line plain">{item.text}</p>
        </div>
      );
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
    case "suggest":
      return <SuggestRow items={item.items} busy={busy} onPick={onPick} />;
  }
}

/** `prefill`: text another page put in the box (Pulse's Run, Journal's To
 * mission). A new `n` replaces the draft; nothing is sent until the person
 * sends it. With `conversationId` the chat moves to that conversation first,
 * like Start over, unless it is already there. `active`: the Chat tab is
 * showing, so the approvals chips read Settings again. `remote`: the web app,
 * where the approvals stay the computer's. `notice`: an automation run that
 * ended; a new `n` posts it in this chat. `open`: a saved chat History picked;
 * a new `n` shows it, and the next message carries it on. `deleted`: a chat
 * History deleted; if it is the open one, a new chat starts. `clear`: the
 * Chat tab's Clear chat, already confirmed; a new `n` deletes the open chat's
 * saved history for good and starts a new one. On the desktop
 * the chat open at the last close comes back on mount. */
export function Chat({
  onOpenSettings,
  onSignIn = onOpenSettings,
  onOpenAutomations,
  onOpenView,
  prefill,
  notice,
  open,
  deleted,
  clear,
  active = true,
  remote = false,
  grokSignIn = false,
}: {
  onOpenSettings: () => void;
  /** The signed-out card's button: Settings → Account. */
  onSignIn?: () => void;
  onOpenAutomations?: () => void;
  /** `/memory`, `/connectors` and the like open their page. */
  onOpenView?: (view: Extract<SlashEffect, { kind: "open" }>["view"]) => void;
  prefill?: Prefill;
  notice?: RunNotice & { n: number };
  open?: { id: string; n: number };
  deleted?: { id: string; n: number };
  clear?: { n: number };
  active?: boolean;
  remote?: boolean;
  /** Settings → Account offers Sign in with Grok here. */
  grokSignIn?: boolean;
}) {
  "use memo";
  const [conversationId, setConversationId] = useState(newConversationId);
  const [items, setItems] = useState<Item[]>([]);
  const [draft, setDraft] = useState("");
  /** Older lines of a reopened chat not shown yet. */
  const [earlier, setEarlier] = useState(0);
  /** The person already moved on, so the last chat must not land on top. */
  const touched = useRef(false);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const busyRef = useRef(false);
  const [mode, setMode] = useState<SessionMode>("chat");
  const [settings, setSettings] = useState<Settings | null>(null);
  const attach = useAttachments(conversationId, { remote, active });
  const slashRows = useSlashHits(draft);
  const [slashAt, setSlashAt] = useState(0);
  // Escape hides the list until the draft changes.
  const [slashShut, setSlashShut] = useState<string | null>(null);
  const slashOpen = slashRows.length > 0 && slashShut !== draft;
  const slashPick = Math.min(slashAt, slashRows.length - 1);
  // Nyx's pose follows what the chat does, not each chunk of text.
  const nyx = useNyx(activityOf(items, busy), active);

  useEffect(() => {
    if (active) getSettings().then(setSettings, () => {});
  }, [active]);

  async function pickModel(id: string) {
    try {
      setSettings(await setModel(id));
    } catch (err) {
      setError(String(err));
    }
  }

  async function setApprovals(approvals: Approvals) {
    if (!settings) return;
    try {
      setSettings(
        await saveSettings(
          settings.model,
          settings.closeToTray,
          settings.workspace,
          approvals,
          settings.desktopControl,
        ),
      );
    } catch (err) {
      setError(String(err));
    }
  }

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

  /** A new conversation; the old one stays saved for History. */
  function startFresh() {
    touched.current = true;
    setItems([]);
    setEarlier(0);
    setError(null);
    setConversationId(newConversationId());
  }

  function show(transcript: Transcript) {
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
    if (remote) return;
    getLastChat().then(
      (last) => restoreLast(last),
      () => {},
    );
  }, [remote]);

  // Only a new pick acts; the conversation it leaves is the latest, not a
  // dependency.
  const openPicked = useEffectEvent((id: string) => {
    if (id === conversationId) return;
    if (busyRef.current) {
      setError(OPEN_BUSY);
      return;
    }
    touched.current = true;
    void closeChat(conversationId);
    loadChat(id).then(show, (err) => setError(String(err)));
  });

  useEffect(() => {
    if (open) openPicked(open.id);
  }, [open]);

  // Deleting the chat on screen starts a new one; deleting another leaves it.
  const dropDeleted = useEffectEvent((id: string) => {
    if (id === conversationId) startFresh();
  });

  useEffect(() => {
    if (!deleted) return;
    const id = deleted.id;
    // After this render, so the reset is one render of its own.
    const timer = window.setTimeout(() => dropDeleted(id), 0);
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
    if (to && to !== conversationId) {
      if (busyRef.current) {
        setError(MISSION_BUSY);
        return;
      }
      touched.current = true;
      void closeChat(conversationId);
      setItems([]);
      setEarlier(0);
      setError(null);
      setConversationId(to);
    }
    setDraft(next.text);
  });
  useEffect(() => {
    if (prefill) applyPrefill(prefill);
  }, [prefill]);

  // A new `n` posts the run's notice, once.
  const [noticeN, setNoticeN] = useState<number>();
  if (notice && notice.n !== noticeN) {
    setNoticeN(notice.n);
    const { n: _n, ...run } = notice;
    setItems((now) => [...now, { kind: "notice", ...run }]);
  }
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    let stopGap: (() => void) | undefined;
    void onChatEvent(({ conversationId: id, event }) => {
      // An event that lands after the turn ended must not reopen it.
      if (id !== conversationId || !busyRef.current) return;
      if (event.kind === "error") setError(event.message);
      else if (event.kind === "done") setItems(event.stop === HALTED_STOP ? settleHalted : settle);
      else if (event.kind === "text") setItems((now) => applyEvent(now, event));
      // A card is a transition, so the webview can fade it in; text streams as is.
      else startTransition(() => setItems((now) => applyEvent(now, event)));
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
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
  }, [conversationId]);

  useEffect(() => {
    endRef.current?.scrollIntoView?.({ block: "end" });
  }, [items]);

  function startOver() {
    if (busy) return;
    void closeChat(conversationId);
    startFresh();
  }

  /** Sends `text` and `files` in the current mode, or in `as` (`/plan <text>`).
   * `fromDraft` empties the box and its files; a picked suggestion leaves them. */
  async function deliver(
    text: string,
    files: typeof attach.files,
    fromDraft: boolean,
    as?: SessionMode,
  ) {
    if ((!text && files.length === 0) || busy) return;
    touched.current = true;
    const unread = files.find((f) => f.status);
    if (unread) {
      setError(`${unread.name}: ${unread.status}`);
      return;
    }
    const said = [text, ...files.map((f) => f.label)].filter(Boolean).join("\n");
    // Suggestions belong to the reply they followed.
    setItems((now) => [...now.filter((item) => item.kind !== "suggest"), { kind: "user", text: said }]);
    if (fromDraft) {
      setDraft("");
      attach.clear();
    }
    setError(null);
    busyRef.current = true;
    setBusy(true);
    let stop = "";
    try {
      const paths = files.map((f) => f.path);
      stop = await (paths.length ? sendChat(conversationId, text, as ?? mode, paths) : sendChat(conversationId, text, as ?? mode));
    } catch (err) {
      setError(String(err));
    }
    // After the catch rather than in a finally, which React Compiler can't compile yet.
    busyRef.current = false;
    setBusy(false);
    setItems(stop === HALTED_STOP ? settleHalted : settle);
  }

  async function answer(id: string, choice: Answer) {
    setItems((now) =>
      now.map((item) =>
        item.kind === "permission" && item.id === id ? { ...item, answer: choice } : item,
      ),
    );
    try {
      await answerPermission(conversationId, id, choice);
    } catch (err) {
      setError(String(err));
    }
  }

  async function answerCard(item: ElicitItem, action: ElicitAction, value: string) {
    setItems((now) =>
      now.map((it) => (it.kind === "elicit" && it.id === item.id ? { ...it, answer: action } : it)),
    );
    // An approved plan leaves plan mode, as in the cabin.
    if (item.server === "Plan" && action === "accept") setMode("chat");
    try {
      await answerElicit(conversationId, item.id, action, item.fieldName, value);
    } catch (err) {
      setError(String(err));
    }
  }

  /** Runs a slash line and applies what it did. */
  async function slash(line: string) {
    setError(null);
    let effect: SlashEffect;
    try {
      effect = await runSlash(line);
    } catch (err) {
      setError(String(err));
      return;
    }
    setDraft("");
    const command = line.split(/\s+/)[0];
    switch (effect.kind) {
      case "newChat":
        startOver();
        break;
      case "mode":
        setMode(effect.mode);
        if (effect.text) void deliver(effect.text.trim(), [], false, effect.mode);
        break;
      case "open":
        onOpenView?.(effect.view);
        break;
      case "modelSet":
        getSettings().then(setSettings, () => {});
        setItems((now) => [...now, { kind: "slash", command, text: effect.text }]);
        break;
      case "notice":
        setItems((now) => [...now, { kind: "slash", command, text: effect.text }]);
        break;
    }
  }

  /** Tab fills a row in; Enter or a click runs one that takes no text. */
  function pickSlash(hit: SlashHit | undefined, run: boolean) {
    if (!hit) return;
    if (hit.disabled) {
      setError(`${hit.cmd}: ${hit.disabled}`);
      return;
    }
    if (run && hit.runOnPick) void slash(hit.cmd);
    else setDraft(hit.insert);
  }

  // Declared after what they call: React Compiler skips a component that
  // calls a function declared further down.
  /** Sends the draft, or `picked` from a suggestion (the draft stays).
   * A slash line runs instead of going to the model. */
  async function send(event?: FormEvent, picked?: string) {
    event?.preventDefault();
    const text = (picked ?? draft).trim();
    const files = picked === undefined ? attach.files : [];
    if ((!text && files.length === 0) || busy) return;
    touched.current = true;
    if (picked === undefined && isSlashCommand(text)) {
      await slash(text);
      return;
    }
    await deliver(text, files, picked === undefined);
  }

  function pick(prompt: string) {
    void send(undefined, prompt);
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
        pickSlash(slashRows[slashPick], false);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        setSlashShut(draft);
        return;
      }
      if (event.key === "Enter" && !event.shiftKey) {
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

  return (
    <section
      className={nyx ? "chat has-nyx" : "chat"}
      aria-label="Chat"
      onDragOver={attach.onDragOver}
      onDrop={attach.onDrop}
    >
      {nyx && <NyxDock url={nyx.url} mood={nyx.mood} />}
      {/* Focusable so the keyboard can scroll a long conversation. */}
      <div className="log" role="log" aria-live="polite" tabIndex={0}>
        {items.length === 0 && !busy && settings && !settings.hasApiKey && !settings.signedIn ? (
          <SignedOut remote={remote} grok={grokSignIn} onSignIn={onSignIn} />
        ) : (
          items.length === 0 &&
          !busy && (
            <div className="nyx-empty">
              {nyx && <NyxFace url={nyx.url} size={96} />}
              <p className="empty">{CHAT_EMPTY}</p>
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
            onPick={pick}
            onDraft={setDraft}
            onOpenAutomations={onOpenAutomations}
          />
        ))}
        {busy && waiting && (
          <div className="bubble assistant streaming" data-role="assistant">
            …
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
      <ModeChips
        mode={mode}
        onMode={setMode}
        approvals={settings?.approvals ?? null}
        onApprovals={(a) => void setApprovals(a)}
        busy={busy}
        remote={remote}
        model={settings?.model ?? null}
        onModel={(id) => void pickModel(id)}
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
          aria-label="Message"
          placeholder={PLACEHOLDERS[mode]}
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
          <Mic onText={(words) => setDraft((now) => appendDraft(now, words))} onError={setError} busy={busy} active={active} />
        )}
        {busy ? (
          <button type="button" onClick={() => void cancelChat(conversationId)}>
            Stop
          </button>
        ) : (
          <button type="submit" disabled={!draft.trim() && attach.files.length === 0}>
            Send
          </button>
        )}
        <button type="button" className="quiet" onClick={startOver} disabled={busy}>
          New chat
        </button>
      </form>
    </section>
  );
}
