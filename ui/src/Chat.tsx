import { memo, useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import {
  answerElicit,
  answerPermission,
  cancelChat,
  closeChat,
  getSettings,
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
} from "./api";
import { ATTACH_MAX, AttachButton, AttachChips, useAttachments } from "./AttachChips";
import type { RunNotice } from "./Automations";
import { appendDraft, Mic } from "./Mic";
import { ElicitCard, ModeChips, type ElicitItem } from "./Modes";
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
      /** "ended": the turn stopped before anyone answered, so nothing ran. */
      answer: Answer | "ended" | null;
    }
  | { kind: "suggest"; items: Suggestion[] }
  | ({ kind: "notice" } & RunNotice)
  /** What a slash command did. It stays in the log and never reaches the model. */
  | { kind: "slash"; command: string; text: string }
  | ElicitItem;

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
    // Deny on a notification: the card shows what ran, unless the turn already ended.
    case "answered":
      return items.map((item) =>
        item.kind === "permission" && item.id === event.id && item.answer !== "ended"
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
function settle(items: Item[]): Item[] {
  return items.map((item) => {
    if (item.kind === "assistant" && item.streaming) return { ...item, streaming: false };
    if (item.kind === "permission" && item.answer === null) return { ...item, answer: "ended" };
    if (item.kind === "elicit" && item.answer === null) return { ...item, answer: "ended" };
    return item;
  });
}

const ANSWERED: Record<Answer | "ended", string> = {
  allow: "Allowed once",
  always: "Always allowed",
  deny: "Denied",
  ended: "Not run: the turn ended",
};

// Typing in the composer re-renders Chat, and a reply streaming in re-renders it
// on every chunk. These take plain values and are memoized, so only the message
// that changed repaints and a long conversation stays cheap to type under.
const UserBubble = memo(function UserBubble({ text }: { text: string }) {
  return (
    <div className="bubble user" data-role="user">
      {text}
    </div>
  );
});

const AssistantBubble = memo(function AssistantBubble({
  text,
  streaming,
}: {
  text: string;
  streaming: boolean;
}) {
  return (
    <div className={`bubble assistant${streaming ? " streaming" : ""}`} data-role="assistant">
      {text}
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

/** `prefill`: text another page put in the box (Pulse's Run, Journal's To
 * mission). A new `n` replaces the draft; nothing is sent until the person
 * sends it. With `conversationId` the chat moves to that conversation first,
 * like Start over, unless it is already there. `active`: the Chat tab is
 * showing, so the approvals chips read Settings again. `remote`: the web app,
 * where the approvals stay the computer's. `notice`: an automation run that
 * ended; a new `n` posts it in this chat. */
export function Chat({
  onOpenSettings,
  onSignIn = onOpenSettings,
  onOpenAutomations,
  onOpenView,
  prefill,
  notice,
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
  active?: boolean;
  remote?: boolean;
  /** Settings → Account offers Sign in with Grok here. */
  grokSignIn?: boolean;
}) {
  const [conversationId, setConversationId] = useState(newConversationId);
  const [items, setItems] = useState<Item[]>([]);
  const [draft, setDraft] = useState("");

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

  useEffect(() => {
    if (!prefill) return;
    const to = prefill.conversationId;
    if (to && to !== conversationId) {
      if (busyRef.current) {
        setError(MISSION_BUSY);
        return;
      }
      void closeChat(conversationId);
      setItems([]);
      setError(null);
      setConversationId(to);
    }
    setDraft(prefill.text);
    // Only a new prefill acts; switching conversations must not re-run it.
  }, [prefill]);
  useEffect(() => {
    if (!notice) return;
    const { n: _n, ...run } = notice;
    setItems((now) => [...now, { kind: "notice", ...run }]);
  }, [notice]);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    let stopGap: (() => void) | undefined;
    void onChatEvent(({ conversationId: id, event }) => {
      // An event that lands after the turn ended must not reopen it.
      if (id !== conversationId || !busyRef.current) return;
      if (event.kind === "error") setError(event.message);
      else if (event.kind === "done") setItems(settle);
      else setItems((now) => applyEvent(now, event));
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

  /** Sends the draft, or `picked` from a suggestion (the draft stays).
   * `as` sends in that mode, for `/plan <text>`. A slash line runs here. */
  async function send(event?: FormEvent, picked?: string, as?: SessionMode) {
    event?.preventDefault();
    const text = (picked ?? draft).trim();
    const files = picked === undefined ? attach.files : [];
    if ((!text && files.length === 0) || busy) return;
    if (picked === undefined && isSlashCommand(text)) {
      await slash(text);
      return;
    }
    const unread = files.find((f) => f.status);
    if (unread) {
      setError(`${unread.name}: ${unread.status}`);
      return;
    }
    const said = [text, ...files.map((f) => f.label)].filter(Boolean).join("\n");
    // Suggestions belong to the reply they followed.
    setItems((now) => [...now.filter((item) => item.kind !== "suggest"), { kind: "user", text: said }]);
    if (picked === undefined) {
      setDraft("");
      attach.clear();
    }
    setError(null);
    busyRef.current = true;
    setBusy(true);
    try {
      const paths = files.map((f) => f.path);
      await (paths.length ? sendChat(conversationId, text, as ?? mode, paths) : sendChat(conversationId, text, as ?? mode));
    } catch (err) {
      setError(String(err));
    } finally {
      busyRef.current = false;
      setBusy(false);
      setItems(settle);
    }
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
        if (effect.text) void send(undefined, effect.text, effect.mode);
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

  function startOver() {
    if (busy) return;
    void closeChat(conversationId);
    setItems([]);
    setError(null);
    setConversationId(newConversationId());
  }

  // The "…" bubble shows only while nothing else in the log is moving.
  const last = items[items.length - 1];
  const waiting =
    !(last?.kind === "assistant" && last.streaming) &&
    !(last?.kind === "permission" && last.answer === null) &&
    !(last?.kind === "elicit" && last.answer === null);

  return (
    <section className="chat" aria-label="Chat" onDragOver={attach.onDragOver} onDrop={attach.onDrop}>
      {/* Focusable so the keyboard can scroll a long conversation. */}
      <div className="log" role="log" aria-live="polite" tabIndex={0}>
        {items.length === 0 && !busy && settings && !settings.hasApiKey && !settings.signedIn ? (
          <SignedOut remote={remote} grok={grokSignIn} onSignIn={onSignIn} />
        ) : (
          items.length === 0 && !busy && <p className="empty">{CHAT_EMPTY}</p>
        )}
        {items.map((item, i) => {
          switch (item.kind) {
            case "user":
              return <UserBubble key={i} text={item.text} />;
            case "assistant":
              return <AssistantBubble key={i} text={item.text} streaming={Boolean(item.streaming)} />;
            case "tool":
              return (
                <ToolCard key={i} title={item.title} status={item.status} detail={item.detail} />
              );
            case "permission":
              return (
                <PermissionCard key={i} item={item} onAnswer={(a) => void answer(item.id, a)} />
              );
            case "elicit":
              return (
                <ElicitCard
                  key={i}
                  item={item}
                  onAnswer={(action, value) => void answerCard(item, action, value)}
                />
              );
            case "notice":
              return (
                <div
                  key={i}
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
                        setDraft(
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
                <div
                  key={i}
                  className="card notice slash-notice"
                  role="status"
                  aria-label={`${item.command} result`}
                >
                  <span className="card-title">{item.command}</span>
                  <p className="card-detail-line plain">{item.text}</p>
                </div>
              );
            case "suggest":
              return (
                <SuggestRow
                  key={i}
                  items={item.items}
                  busy={busy}
                  onPick={(prompt) => void send(undefined, prompt)}
                />
              );
          }
        })}
        {busy && waiting && (
          <div className="bubble assistant streaming" data-role="assistant">
            …
          </div>
        )}
        <div ref={endRef} />
      </div>
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
