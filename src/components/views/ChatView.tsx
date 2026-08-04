import { FolderOpen,
  Loader2,
  Download,
  Pencil,
  Paperclip,
  Send,
  Square,
  Sparkles,
  Terminal,
  Compass,
  Gauge,
  ShieldAlert,
  X,
  Mic,
  MicOff,
  RefreshCw,
  Copy,
  Reply,
  Trash2,
  Check,
  ThumbsUp,
  ThumbsDown,
  Search,
  RotateCcw,
} from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getMode } from "@/lib/modes";
import { tierMeta } from "@/lib/models-catalog";
import { buildQuickChips, type QuickChip } from "@/lib/quick-assistant";
import { contextFingerprint } from "@/lib/quick-assist-llm";
import { streamStatusPill } from "@/lib/tool-status";
import { estimateThreadContextPercent } from "@/lib/context-manager";
import { useGrokHub } from "@/lib/store";
import { cn } from "@/lib/utils";
import type { ChatMessage, ChatRole } from "@/lib/types";
import { RelativeTime } from "../RelativeTime";
import { HostGatewayBanner } from "../HostGatewayBanner";
import { EmojiPicker } from "../EmojiPicker";
import { SlashAutocomplete } from "../SlashAutocomplete";
import type { SlashDef } from "@/lib/slash-commands";
import { Button } from "../ui/button";
import { Textarea } from "../ui/textarea";
import { MarkdownBody } from "@/lib/markdown";
import { looksLikeIncompleteAgentTurn } from "@/lib/agent-finish";

function chipIcon(kind: QuickChip["kind"]) {
  if (kind === "shell") return Terminal;
  if (kind === "nav") return Compass;
  if (kind === "mode") return Gauge;
  return Sparkles;
}

const MAX_ATTACH_BYTES = 1_200_000;

async function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ""));
    reader.onerror = () => reject(reader.error || new Error("read failed"));
    reader.readAsDataURL(file);
  });
}

type MessageRowProps = {
  m: ChatMessage;
  busy: boolean;
  streamStatus: string | null;
  streamingMessageId: string | null;
  editingId: string | null;
  editDraft: string;
  copiedId: string | null;
  findHit?: boolean;
  isLastAssistant?: boolean;
  onJumpReply: (id: string | undefined) => void;
  onReply: (m: { id: string; content: string; role: ChatRole }) => void;
  onCopy: (id: string, content: string) => void;
  onStartEdit: (id: string, content: string) => void;
  onEditDraft: (v: string) => void;
  onSaveEdit: (id: string, resend: boolean) => void;
  onCancelEdit: () => void;
  onDelete: (id: string) => void;
  onRate?: (id: string, positive: boolean) => void;
  onRegenerate?: () => void;
};

const ChatMessageRow = memo(function ChatMessageRow({
  m,
  busy,
  streamStatus,
  streamingMessageId,
  findHit,
  isLastAssistant,
  onRegenerate,
  editingId,
  editDraft,
  copiedId,
  onJumpReply,
  onReply,
  onCopy,
  onStartEdit,
  onEditDraft,
  onSaveEdit,
  onCancelEdit,
  onDelete,
  onRate,
}: MessageRowProps) {
  // Only the active stream owner shows LIVE chrome
  const showStreaming =
    Boolean(m.streaming) &&
    m.role === "assistant" &&
    (busy || Boolean(streamStatus)) &&
    streamingMessageId != null &&
    m.id === streamingMessageId;
  return (
    <div
      id={`msg-${m.id}`}
      className={cn(
        "group/msg msg-enter flex",
        m.role === "user" ? "justify-end" : "justify-start",
        findHit && "ring-2 ring-[var(--color-info)] ring-offset-2 ring-offset-[var(--color-bg)] rounded-[var(--radius-lg)]",
      )}
    >
      <div
        className={cn(
          "chat-bubble relative rounded-[var(--radius-lg)] border px-3.5 py-2.5 text-sm leading-relaxed shadow-sm",
          m.role === "user"
            ? "border-[var(--color-border-strong)] bg-[var(--color-bubble-user)] text-[var(--color-fg)]"
            : m.role === "system"
              ? "border-[var(--color-border)] bg-[var(--color-panel)] text-[var(--color-muted)]"
              : "border-[var(--color-border)] bg-[var(--color-bubble-assistant)] text-[var(--color-fg)]",
          showStreaming &&
            "border-[color-mix(in_oklab,var(--color-info)_45%,var(--color-border))] shadow-[0_0_0_1px_color-mix(in_oklab,var(--color-info)_20%,transparent)]",
        )}
      >
        {m.replyToPreview && (
          <button
            type="button"
            className="mb-2 flex w-full items-start gap-2 rounded-[var(--radius-sm)] border-l-2 border-[var(--color-info)] bg-[color-mix(in_oklab,var(--color-info)_8%,var(--color-surface))] px-2 py-1.5 text-left text-xs text-[var(--color-muted)]"
            title="Jump to original"
            onClick={() => onJumpReply(m.replyToId)}
          >
            <Reply className="mt-0.5 h-3 w-3 shrink-0 text-[var(--color-info)]" />
            <span className="min-w-0">
              <span className="font-medium text-[var(--color-fg)]">
                {m.replyToRole || "message"}
              </span>
              <span className="mt-0.5 line-clamp-2 block normal-case tracking-normal">
                {m.replyToPreview}
              </span>
            </span>
          </button>
        )}
        <div
          id={`msg-${m.id}`}
          className="mb-1 flex flex-wrap items-center gap-2 text-[10px] uppercase tracking-wide text-[var(--color-subtle)]"
        >
          <span>
            {m.role} · <RelativeTime ts={m.ts} />
          </span>
          {(m.routeTier || m.mode) && m.role === "assistant" && (
            <span className="inline-flex max-w-full flex-col items-start gap-0.5 normal-case">
              <span
                className={cn(
                  "adaptive-pill inline-flex items-center gap-1 rounded-full border px-2.5 py-0.5 text-[11px] font-bold tracking-wide shadow-sm",
                  m.routeTier
                    ? tierMeta(m.routeTier).tone
                    : "border-[var(--color-border)] bg-[var(--color-elevated)] text-[var(--color-muted)]",
                )}
                title={
                  m.routeReason
                    ? m.routeReason
                    : m.mode === "auto"
                      ? "Adaptive router"
                      : getMode(m.mode || "auto").label
                }
              >
                {m.routeTier
                  ? tierMeta(m.routeTier).label
                  : m.mode === "auto"
                    ? "Adaptive"
                    : getMode(m.mode!).label}
              </span>
              {m.routeReason && (
                <span
                  className="max-w-[min(100%,20rem)] truncate text-[10px] font-normal normal-case tracking-normal text-[var(--color-subtle)]"
                  title={m.routeReason}
                >
                  {m.routeReason}
                </span>
              )}
            </span>
          )}
          {m.edited && (
            <span className="rounded border border-[var(--color-border)] px-1.5 py-px font-mono normal-case text-[var(--color-subtle)]">
              edited
            </span>
          )}
          {m.routeModel && m.role === "assistant" && (
            <span
              className="hidden max-w-[10rem] truncate rounded border border-[var(--color-border)] px-1.5 py-px font-mono normal-case text-[var(--color-subtle)] sm:inline"
              title={m.routeReason || m.routeModel}
            >
              {m.routeModel.replace(/^grok-/, "")}
            </span>
          )}
          {showStreaming && (
            <span
              className="inline-flex max-w-[min(100%,18rem)] items-center gap-1 rounded border border-[color-mix(in_oklab,var(--color-info)_40%,transparent)] px-1.5 py-px font-mono normal-case text-[var(--color-info)]"
              title={streamStatus || "Working…"}
            >
              <Loader2 className="h-2.5 w-2.5 shrink-0 animate-spin" />
              <span className="truncate">{streamStatusPill(streamStatus)}</span>
            </span>
          )}
          {m.stopped && (
            <span className="rounded border border-[var(--color-border)] px-1.5 py-px font-mono normal-case text-[var(--color-warn)]">
              stopped
            </span>
          )}
        </div>
        {m.content ? (
          m.role === "user" && editingId === m.id ? (
            <div className="space-y-2">
              <textarea
                value={editDraft}
                onChange={(e) => onEditDraft(e.target.value)}
                className="min-h-[4rem] w-full rounded border border-[var(--color-border-strong)] bg-[var(--color-surface)] p-2 text-sm"
                autoFocus
              />
              <div className="flex gap-1.5">
                <Button size="sm" onClick={() => onSaveEdit(m.id, true)}>
                  Save & resend
                </Button>
                <Button size="sm" variant="secondary" onClick={() => onSaveEdit(m.id, false)}>
                  Save
                </Button>
                <Button size="sm" variant="ghost" onClick={onCancelEdit}>
                  Cancel
                </Button>
              </div>
            </div>
          ) : m.role === "user" ? (
            <div className="whitespace-pre-wrap">
              <MarkdownBody content={m.content} />
            </div>
          ) : (
            <>
              <MarkdownBody content={m.content} streaming={showStreaming} />
              {m.streaming && streamStatus ? (
                <div
                  className="mt-2 flex items-center gap-2 border-t border-[var(--color-border)] pt-2 text-[11px] text-[var(--color-muted)]"
                  role="status"
                  aria-live="polite"
                >
                  <Loader2 className="h-3 w-3 shrink-0 animate-spin text-[var(--color-info)]" />
                  <span className="min-w-0 truncate font-mono">{streamStatus}</span>
                </div>
              ) : null}
            </>
          )
        ) : showStreaming ? (
          <span className="inline-flex items-center gap-2 text-sm text-[var(--color-muted)]" role="status" aria-live="polite">
            <span className="relative flex h-2 w-2">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-[var(--color-info)] opacity-60" />
              <span className="relative inline-flex h-2 w-2 rounded-full bg-[var(--color-info)]" />
            </span>
            {streamStatus || "Thinking…"}
          </span>
        ) : (
          ""
        )}

        {m.content && !showStreaming && (
          <div
            className={cn(
              "mt-2 flex flex-wrap items-center gap-0.5 border-t border-[var(--color-border)] pt-1.5",
              "opacity-100 sm:opacity-0 sm:transition-opacity sm:group-hover/msg:opacity-100 sm:focus-within:opacity-100",
            )}
          >
            <button
              type="button"
              className="focus-ring inline-flex items-center gap-1 rounded-[var(--radius-xs)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-muted)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]"

              title="Reply to this message"
              onClick={() => onReply({ id: m.id, content: m.content, role: m.role })}
            >
              <Reply className="h-3 w-3" />
              Reply
            </button>
            <button
              type="button"
              className="focus-ring inline-flex items-center gap-1 rounded-[var(--radius-xs)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-muted)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]"

              title="Copy message"
              onClick={() => void onCopy(m.id, m.content)}
            >
              {copiedId === m.id ? (
                <Check className="h-3 w-3 text-[var(--color-success)]" />
              ) : (
                <Copy className="h-3 w-3" />
              )}
              {copiedId === m.id ? "Copied" : "Copy"}
            </button>
            {m.role === "assistant" && !busy && onRate && (
              <>
                <button
                  type="button"
                  className="focus-ring inline-flex items-center gap-1 rounded-[var(--radius-xs)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-muted)] hover:bg-[color-mix(in_oklab,var(--color-success)_12%,transparent)] hover:text-[var(--color-success)]"
                  title="Helpful — teach GrokHub"
                  onClick={() => onRate(m.id, true)}
                >
                  <ThumbsUp className="h-3 w-3" />
                </button>
                <button
                  type="button"
                  className="focus-ring inline-flex items-center gap-1 rounded-[var(--radius-xs)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-muted)] hover:bg-[color-mix(in_oklab,var(--color-danger)_12%,transparent)] hover:text-[var(--color-danger)]"
                  title="Not helpful — teach GrokHub"
                  onClick={() => onRate(m.id, false)}
                >
                  <ThumbsDown className="h-3 w-3" />
                </button>
              </>
            )}

            {m.role === "assistant" && isLastAssistant && !busy && onRegenerate && (
              <button
                type="button"
                className="focus-ring inline-flex items-center gap-1 rounded-[var(--radius-xs)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-muted)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]"
                title="Regenerate response"
                onClick={() => onRegenerate()}
              >
                <RotateCcw className="h-3 w-3" />
                Regen
              </button>
            )}
            {m.role === "user" && !busy && (
              <button
                type="button"
                className="focus-ring inline-flex items-center gap-1 rounded-[var(--radius-xs)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-muted)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]"

                title="Edit message"
                onClick={() => onStartEdit(m.id, m.content)}
              >
                <Pencil className="h-3 w-3" />
                Edit
              </button>
            )}
            {!busy && (
              <button
                type="button"
                className="focus-ring inline-flex items-center gap-1 rounded-[var(--radius-xs)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-muted)] hover:bg-[color-mix(in_oklab,var(--color-danger)_12%,transparent)] hover:text-[var(--color-danger)]"
                title="Delete message"
                onClick={() => onDelete(m.id)}
              >
                <Trash2 className="h-3 w-3" />
                Delete
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
});


function ContextBudgetChip() {
  const chat = useGrokHub((s) => s.chat);
  const threads = useGrokHub((s) => s.threads);
  const activeThreadId = useGrokHub((s) => s.activeThreadId);
  const memoryNotes = useGrokHub((s) => s.agentPrefs.memoryNotes);
  const stats = useMemo(() => {
    const th = threads.find((x) => x.id === activeThreadId);
    return estimateThreadContextPercent(chat, th || null, memoryNotes);
  }, [chat, threads, activeThreadId, memoryNotes]);
  const th = threads.find((x) => x.id === activeThreadId);
  const tone =
    stats.percent >= 85
      ? "text-[var(--color-danger)] border-[color-mix(in_oklab,var(--color-danger)_40%,var(--color-border))]"
      : stats.percent >= 70
        ? "text-[var(--color-warn)] border-[color-mix(in_oklab,var(--color-warn)_40%,var(--color-border))]"
        : "text-[var(--color-muted)] border-[var(--color-border)]";
  return (
    <button
      type="button"
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[10px] font-medium tabular-nums",
        tone,
      )}
      title={`Context ~${stats.tokensEst.toLocaleString()} / ${stats.budget.toLocaleString()} tokens. Click for /context report.`}
      onClick={() => {
        void useGrokHub.getState().sendChat("/context");
      }}
    >
      <span
        className={cn(
          "inline-block h-1.5 w-1.5 rounded-full",
          stats.percent >= 85
            ? "bg-[var(--color-danger)]"
            : stats.percent >= 70
              ? "bg-[var(--color-warn)]"
              : "bg-[var(--color-success)]",
        )}
      />
      Context {stats.percent}%
      {th?.summary ? (
        <span className="rounded bg-[var(--color-elevated)] px-1 font-mono text-[9px] text-[var(--color-subtle)]">
          compact
        </span>
      ) : null}
    </button>
  );
}


function ProjectBar() {
  const project = useGrokHub((s) => s.projectWorkspace);
  const bindProjectWorkspace = useGrokHub((s) => s.bindProjectWorkspace);
  const clearProjectWorkspace = useGrokHub((s) => s.clearProjectWorkspace);
  const pushActivity = useGrokHub((s) => s.pushActivity);
  const [busy, setBusy] = useState(false);

  async function bind() {
    setBusy(true);
    try {
      let path = "";
      const desk = window.grokhubDesktop;
      if (desk?.pickFolder) {
        const r = await desk.pickFolder();
        if (r?.canceled) return;
        if (r?.ok && r.path) path = r.path;
      }
      if (!path) {
        path = window.prompt("Project folder path", project?.path || "") || "";
      }
      path = path.trim();
      if (!path) return;
      const res = await bindProjectWorkspace(path);
      pushActivity({
        kind: "desktop",
        title: res.ok ? "Project bound" : "Bind failed",
        detail: res.detail,
        status: res.ok ? "success" : "failed",
      });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--color-border)] bg-[var(--color-panel)]/80 px-3 py-1.5 text-[11px] md:px-4">
      <FolderOpen className="h-3.5 w-3.5 shrink-0 text-[var(--color-muted)]" />
      {project?.path ? (
        <>
          <span className="text-[var(--color-muted)]">Project</span>
          <span className="max-w-[min(28rem,50vw)] truncate font-medium text-[var(--color-fg)]" title={project.path}>
            {project.name}
          </span>
          <span className="hidden font-mono text-[10px] text-[var(--color-subtle)] sm:inline">
            {project.path}
          </span>
          <Button type="button" size="sm" variant="ghost" className="h-7 px-2 text-[11px]" disabled={busy} onClick={() => void bind()}>
            Change
          </Button>
          <Button type="button" size="sm" variant="ghost" className="h-7 px-2 text-[11px]" disabled={busy} onClick={() => clearProjectWorkspace()}>
            Unbind
          </Button>
        </>
      ) : (
        <>
          <span className="text-[var(--color-muted)]">No project bound</span>
          <Button type="button" size="sm" variant="secondary" className="h-7 px-2 text-[11px]" disabled={busy} onClick={() => void bind()}>
            Bind folder
          </Button>
          <span className="text-[10px] text-[var(--color-subtle)]">or <span className="font-mono">/project bind</span></span>
        </>
      )}
    </div>
  );
}

export function ChatView() {
  const chat = useGrokHub((s) => s.chat);
  const sendChat = useGrokHub((s) => s.sendChat);
  const proactiveNotice = useGrokHub((s) => s.proactiveNotice);
  const dismissProactiveNotice = useGrokHub((s) => s.dismissProactiveNotice);
  const stopChat = useGrokHub((s) => s.stopChat);
  const running = useGrokHub((s) => s.running);
  const streamStatus = useGrokHub((s) => s.streamStatus);
  const streamingMessageId = useGrokHub((s) => s.streamingMessageId);
  const mode = useGrokHub((s) => s.mode);
  const setMode = useGrokHub((s) => s.setMode);
  const setNav = useGrokHub((s) => s.setNav);
  const pushActivity = useGrokHub((s) => s.pushActivity);
  const recordUsage = useGrokHub((s) => s.recordUsage);
  const usage = useGrokHub((s) => s.usage);
  const grokConnected = useGrokHub((s) => s.grokConnected);
  const newThread = useGrokHub((s) => s.newThread);
  const activity = useGrokHub((s) => s.activity);
  const threads = useGrokHub((s) => s.threads);
  const activeThreadId = useGrokHub((s) => s.activeThreadId);
  const connectors = useGrokHub((s) => s.connectors);
  const pendingHostConfirm = useGrokHub((s) => s.pendingHostConfirm);
  const resolveHostConfirm = useGrokHub((s) => s.resolveHostConfirm);
  const quickAssistMemory = useGrokHub((s) => s.quickAssistMemory);
  const recordQuickAssistChip = useGrokHub((s) => s.recordQuickAssistChip);
  const recordQuickAssistTyped = useGrokHub((s) => s.recordQuickAssistTyped);
  const quickAssistDismissed = useGrokHub((s) => s.quickAssistDismissed);
  const quickAssistRotation = useGrokHub((s) => s.quickAssistRotation);
  const dismissQuickAssistChip = useGrokHub((s) => s.dismissQuickAssistChip);
  const rotateQuickAssist = useGrokHub((s) => s.rotateQuickAssist);
  const refreshQuickAssistLlm = useGrokHub((s) => s.refreshQuickAssistLlm);
  const quickAssistLlmChips = useGrokHub((s) => s.quickAssistLlmChips);
  const quickAssistLlmBusy = useGrokHub((s) => s.quickAssistLlmBusy);
  const sessionResume = useGrokHub((s) => s.sessionResume);
  const resumeLastSession = useGrokHub((s) => s.resumeLastSession);
  const continueInterruptedSession = useGrokHub((s) => s.continueInterruptedSession);
  const keepGoingChat = useGrokHub((s) => s.keepGoingChat);
  const dismissSessionResume = useGrokHub((s) => s.dismissSessionResume);
  const exportThreadMarkdown = useGrokHub((s) => s.exportThreadMarkdown);
  const editChatMessage = useGrokHub((s) => s.editChatMessage);
  const deleteChatMessages = useGrokHub((s) => s.deleteChatMessages);
  const rateMessage = useGrokHub((s) => s.rateMessage);
  const welcomeMessage = useGrokHub((s) => s.welcomeMessage);
  const welcomeBusy = useGrokHub((s) => s.welcomeBusy);
  const refreshWelcomeMessage = useGrokHub((s) => s.refreshWelcomeMessage);
  const replyTo = useGrokHub((s) => s.replyTo);
  const setReplyTo = useGrokHub((s) => s.setReplyTo);
  const composerDrafts = useGrokHub((s) => s.composerDrafts);
  const setComposerDraft = useGrokHub((s) => s.setComposerDraft);
  const shellHistory = useGrokHub((s) => s.shellHistory);
  const pushShellHistory = useGrokHub((s) => s.pushShellHistory);
  const addHostAllow = useGrokHub((s) => s.addHostAllow);
  const regenerateLast = useGrokHub((s) => s.regenerateLast);
  const [text, setText] = useState("");
  const [slashOpen, setSlashOpen] = useState(false);
  useEffect(() => {
    const focus = () => {
      requestAnimationFrame(() => {
        inputRef.current?.focus();
        resizeComposer();
      });
    };
    window.addEventListener("grokhub:focus-composer", focus);
    window.addEventListener("grokhub:new-chat", focus);
    return () => {
      window.removeEventListener("grokhub:focus-composer", focus);
      window.removeEventListener("grokhub:new-chat", focus);
    };
  }, []);

  // Auto-hide proactive banner
  useEffect(() => {
    if (!proactiveNotice) return;
    const t = window.setTimeout(() => dismissProactiveNotice(), 8000);
    return () => window.clearTimeout(t);
  }, [proactiveNotice, dismissProactiveNotice]);

  const [findOpen, setFindOpen] = useState(false);
  const [findQ, setFindQ] = useState("");
  const [findIdx, setFindIdx] = useState(0);
  const [shellHistIdx, setShellHistIdx] = useState(-1);
  const [alwaysAllowHost, setAlwaysAllowHost] = useState(false);
  const [attachError, setAttachError] = useState<string | null>(null);
  const [localRunning, setLocalRunning] = useState(false);
  const localHostJobRef = useRef<string | null>(null);
  const [pendingBusy, setPendingBusy] = useState(false);
  const [hostOnline, setHostOnline] = useState<boolean | undefined>(undefined);
  const [historyExtra, setHistoryExtra] = useState(0);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState("");
  const [listening, setListening] = useState(false);
  const recognitionRef = useRef<{ stop: () => void } | null>(null);
  const voiceRef = useRef<import("@/lib/voice-input").VoiceSession | null>(null);
  const [voiceStatus, setVoiceStatus] = useState<string | null>(null);
  const [chipsOpen, setChipsOpen] = useState(false);
  const [emojiOpen, setEmojiOpen] = useState(false);
  const [attachments, setAttachments] = useState<Array<{ name: string; dataUrl: string; kind: string }>>([]);
  const endRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);
  const [showJumpLatest, setShowJumpLatest] = useState(false);
  const pendingJumpId = useRef<string | null>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const busy = running || localRunning || pendingBusy;

  // Load draft when thread changes
  useEffect(() => {
    const d = (activeThreadId && composerDrafts?.[activeThreadId]) || "";
    setText(d);
    setShellHistIdx(-1);
    setSlashOpen(false);
    requestAnimationFrame(() => resizeComposer());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeThreadId]);

  // Persist draft (debounced lightly via rAF)
  useEffect(() => {
    if (!activeThreadId) return;
    const id = window.setTimeout(() => {
      setComposerDraft(activeThreadId, text);
    }, 200);
    return () => window.clearTimeout(id);
  }, [text, activeThreadId, setComposerDraft]);



  const lastAssistantId = useMemo(() => {
    for (let i = chat.length - 1; i >= 0; i--) {
      if (chat[i]?.role === "assistant") return chat[i]!.id;
    }
    return null;
  }, [chat]);

  const WINDOW = 50;
  const visibleChat = useMemo(() => {
    const take = WINDOW + historyExtra;
    if (chat.length <= take) return chat;
    return chat.slice(chat.length - take);
  }, [chat, historyExtra]);
  const hiddenCount = Math.max(0, chat.length - visibleChat.length);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const { hostInfo } = await import("@/lib/host-client");
        const i = await hostInfo();
        if (!cancelled) {
          setHostOnline(i.bridge !== "none" && Boolean(i.unsandboxed));
        }
      } catch {
        if (!cancelled) setHostOnline(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (running) setPendingBusy(false);
  }, [running]);

  const chips = useMemo(
    () =>
      buildQuickChips({
        chat,
        activity,
        threads,
        connectors,
        mode,
        grokConnected,
        usage,
        draft: text,
        hostOnline,
        memory: quickAssistMemory,
        dismissed: quickAssistDismissed,
        rotation: quickAssistRotation,
        max: 5,
        threadTitle: threads.find((th) => th.id === activeThreadId)?.title || null,
        llmChips: quickAssistLlmChips,
        contextTag: contextFingerprint(chat, activity),
      }),
    [
      chat,
      activity,
      threads,
      connectors,
      mode,
      grokConnected,
      usage,
      text,
      hostOnline,
      quickAssistMemory,
      quickAssistDismissed,
      quickAssistRotation,
      activeThreadId,
      quickAssistLlmChips,
    ],
  );

  // Fast-mode chip refresh only while suggestions are expanded
  useEffect(() => {
    if (!chipsOpen) return;
    if (busy) return;
    if (chat.length === 0) return;
    const t = window.setTimeout(() => {
      void refreshQuickAssistLlm();
    }, 900);
    return () => window.clearTimeout(t);
  }, [chipsOpen, chat.length, activeThreadId, busy, refreshQuickAssistLlm]);


  // Open suggestions for empty chats so the primary chip is obvious
  useEffect(() => {
    if (chat.length === 0) setChipsOpen(true);
  }, [chat.length, activeThreadId]);

  // Adaptive welcome for empty chat pages
  useEffect(() => {
    if (chat.length !== 0) return;
    void refreshWelcomeMessage();
  }, [chat.length, activeThreadId, refreshWelcomeMessage]);

  const findMatches = useMemo(() => {
    const q = findQ.trim().toLowerCase();
    if (!q) return [] as string[];
    return chat
      .filter((m) => (m.content || "").toLowerCase().includes(q))
      .map((m) => m.id);
  }, [chat, findQ]);

  useEffect(() => {
    setFindIdx(0);
  }, [findQ, activeThreadId]);

  useEffect(() => {
    if (!findOpen || !findMatches.length) return;
    const id = findMatches[Math.min(findIdx, findMatches.length - 1)];
    if (!id) return;
    document.getElementById(`msg-${id}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [findOpen, findIdx, findMatches]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === "f") {
        const tag = (e.target as HTMLElement)?.tagName;
        // always scope find in chat view
        e.preventDefault();
        setFindOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const onListScroll = useCallback(() => {
    const el = listRef.current;
    if (!el) return;
    const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
    const nearBottom = dist < 96;
    stickToBottomRef.current = nearBottom;
    setShowJumpLatest(!nearBottom && chat.length > 0);
  }, [chat.length]);

  useEffect(() => {
    if (!stickToBottomRef.current) {
      setShowJumpLatest(true);
      return;
    }
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [chat, busy, streamStatus]);

  // Heal stale streaming flags left after aborted/interrupted turns
  useEffect(() => {
    if (busy || streamStatus) return;
    const stuck = chat.some((m) => m.role === "assistant" && m.streaming);
    if (!stuck) return;
    useGrokHub.setState((s) => {
      const nextChat = s.chat.map((m) =>
        m.streaming ? { ...m, streaming: false } : m,
      );
      const threads = s.threads.map((th) =>
        th.id === s.activeThreadId
          ? { ...th, messages: nextChat, updatedAt: Date.now() }
          : {
              ...th,
              messages: (th.messages || []).map((m) =>
                m.streaming ? { ...m, streaming: false } : m,
              ),
            },
      );
      return {
        chat: nextChat,
        threads,
        streamingMessageId: null,
        streamStatus: null,
        running: false,
      };
    });
  }, [chat, busy, streamStatus]);

  useEffect(() => {
    const id = pendingJumpId.current;
    if (!id) return;
    const el = document.getElementById(`msg-${id}`);
    if (el) {
      pendingJumpId.current = null;
      requestAnimationFrame(() => {
        el.scrollIntoView({ behavior: "smooth", block: "center" });
      });
    }
  }, [visibleChat, historyExtra]);

  useEffect(() => {
    if (!pendingHostConfirm) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        resolveHostConfirm(false);
      } else if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        resolveHostConfirm(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pendingHostConfirm, resolveHostConfirm]);

  useEffect(() => {
    const onResume = (ev: Event) => {
      const detail = (ev as CustomEvent).detail as {
        pendingPrompt?: string;
        focusOnly?: boolean;
      } | null;
      requestAnimationFrame(() => {
        stickToBottomRef.current = true;
        endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
        if (detail?.pendingPrompt) {
          setText(detail.pendingPrompt);
        }
        inputRef.current?.focus();
        resizeComposer();
      });
    };
    window.addEventListener("grokhub:resume-session", onResume as EventListener);
    return () => window.removeEventListener("grokhub:resume-session", onResume as EventListener);
  }, []);

  useEffect(() => {
    const focus = () => {
      inputRef.current?.focus();
      resizeComposer();
    };
    window.addEventListener("grokhub:focus-chat-input", focus);
    return () => window.removeEventListener("grokhub:focus-chat-input", focus);
  }, []);

  function resizeComposer(el?: HTMLTextAreaElement | null) {
    const ta = el ?? inputRef.current;
    if (!ta) return;
    ta.style.height = "0px";
    const min = 40;
    const max = 160;
    const next = Math.min(max, Math.max(min, ta.scrollHeight));
    ta.style.height = `${next}px`;
    ta.style.overflowY = ta.scrollHeight > max ? "auto" : "hidden";
  }

  function insertEmoji(emoji: string) {
    const ta = inputRef.current;
    const start = ta?.selectionStart ?? text.length;
    const end = ta?.selectionEnd ?? text.length;
    const next = text.slice(0, start) + emoji + text.slice(end);
    setText(next);
    requestAnimationFrame(() => {
      const el = inputRef.current;
      if (!el) return;
      const pos = start + emoji.length;
      el.focus();
      try {
        el.setSelectionRange(pos, pos);
      } catch {
        /* ignore */
      }
      resizeComposer(el);
    });
  }

  useEffect(() => {
    resizeComposer();
  }, [text, busy]);

  async function addFiles(files: FileList | File[]) {
    const list = Array.from(files).slice(0, 4);
    const next: typeof attachments = [];
    for (const f of list) {
      if (f.size > MAX_ATTACH_BYTES) {
        setAttachError(`${f.name} is too large (max ~1.2 MB)`);
        pushActivity({
          kind: "system",
          title: "Attachment too large",
          detail: `${f.name} — keep under ~1.2MB`,
          status: "failed",
        });
        continue;
      }
      try {
        const dataUrl = await fileToDataUrl(f);
        if (dataUrl.length > MAX_ATTACH_BYTES * 1.4) {
          pushActivity({
            kind: "system",
            title: "Attachment too large",
            detail: f.name,
            status: "failed",
          });
          continue;
        }
        next.push({
          name: f.name,
          dataUrl,
          kind: f.type || "application/octet-stream",
        });
      } catch {
        /* skip */
      }
    }
    if (next.length) setAttachments((a) => [...a, ...next].slice(0, 6));
  }

  async function runShell(command: string) {
    setLocalRunning(true);
    const jobId = `local-host-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
    localHostJobRef.current = jobId;
    const userLine = command.startsWith("$") ? command : `$ ${command}`;
    useGrokHub.setState((s) => {
      const nextChat = [
        ...s.chat,
        {
          id: `u_${Date.now()}`,
          role: "user" as const,
          content: userLine,
          ts: Date.now(),
          mode,
        },
      ];
      const nextThreads = s.threads.map((t) =>
        t.id === s.activeThreadId
          ? { ...t, messages: nextChat, updatedAt: Date.now() }
          : t,
      );
      return { chat: nextChat, threads: nextThreads, running: true, streamStatus: "Host running…" };
    });
    try {
      const bill = recordUsage("host");
      if (!bill.ok) {
        useGrokHub.setState((s) => ({
          running: false,
          streamStatus: null,
          chat: [
            ...s.chat,
            {
              id: `a_${Date.now()}`,
              role: "system" as const,
              content: "Host command blocked by local limits.",
              ts: Date.now(),
            },
          ],
        }));
        return;
      }
      const { hostExec } = await import("@/lib/host-client");
      const r = await hostExec(
        command.replace(/^\$\s*/, "").replace(/^\/sh\s+/, ""),
        undefined,
        undefined,
        { jobId },
      );
      if (localHostJobRef.current !== jobId) return;
      const out = [
        r.ok ? "```" : "```text",
        `$ ${r.command || command}`,
        r.stdout || "",
        r.stderr ? `\n[stderr]\n${r.stderr}` : "",
        "```",
        `exit ${r.code} · ${r.ms}ms`,
      ]
        .filter(Boolean)
        .join("\n");
      useGrokHub.setState((s) => {
        const nextChat = [
          ...s.chat,
          {
            id: `a_${Date.now()}`,
            role: "assistant" as const,
            content: out,
            ts: Date.now(),
            mode,
          },
        ];
        const nextThreads = s.threads.map((t) =>
          t.id === s.activeThreadId ? { ...t, messages: nextChat, updatedAt: Date.now() } : t,
        );
        return { chat: nextChat, threads: nextThreads, running: false, streamStatus: null };
      });
    } catch (e) {
      if (localHostJobRef.current !== jobId) return;
      useGrokHub.setState((s) => ({
        chat: [
          ...s.chat,
          {
            id: `a_${Date.now()}`,
            role: "system" as const,
            content: e instanceof Error ? e.message : "host failed",
            ts: Date.now(),
          },
        ],
        running: false,
        streamStatus: null,
      }));
    } finally {
      if (localHostJobRef.current === jobId) localHostJobRef.current = null;
      setLocalRunning(false);
    }
  }

  async function onChip(chip: QuickChip) {
    recordQuickAssistChip(chip);
    if (chip.kind === "nav" && chip.value.startsWith("__nav:")) {
      let dest = chip.value.slice("__nav:".length);
      // Desktop tab removed — host controls live in Settings
      if (dest === "desktop" || dest === "connectors" || dest === "queue" || dest === "agents") {
        dest = "settings";
      }
      setNav(dest as "settings" | "imagine" | "history" | "command" | "chat");
      return;
    }
    if (chip.kind === "mode" && chip.value.startsWith("__mode:")) {
      const m = chip.value.slice("__mode:".length) as "auto" | "fast" | "balanced" | "expert" | "heavy" | "max" | "build";
      setMode(m);
      return;
    }
    if (chip.kind === "shell" || chip.value.startsWith("$") || chip.value.startsWith("/sh ")) {
      setText("");
      await runShell(chip.value);
      return;
    }
    setText("");
    setPendingBusy(true);
    await sendChat(chip.value);
    setPendingBusy(false);
  }

  function stopVoice() {
    try {
      recognitionRef.current?.stop();
    } catch {
      /* ignore */
    }
    recognitionRef.current = null;
    if (voiceRef.current) {
      void voiceRef.current.cancel();
      voiceRef.current = null;
    }
    setListening(false);
    setVoiceStatus(null);
  }

  async function toggleVoice() {
    const { toggleVoiceSession } = await import("@/lib/voice-input");
    const oauth = useGrokHub.getState().oauth;
    const apiKey = useGrokHub.getState().apiKey;
    if (listening && voiceRef.current) {
      setVoiceStatus("Transcribing…");
      const spoken = await voiceRef.current.stopAndTranscribe();
      voiceRef.current = null;
      setListening(false);
      if (spoken) {
        setText((prev) => {
          const base = prev.trim();
          return base ? `${base} ${spoken}` : spoken;
        });
        setVoiceStatus(null);
        requestAnimationFrame(() => {
          resizeComposer();
          inputRef.current?.focus();
        });
        pushActivity({
          kind: "system",
          title: "Voice transcribed",
          detail: spoken.slice(0, 120),
          status: "success",
        });
      } else {
        setVoiceStatus(null);
      }
      return;
    }

    const result = await toggleVoiceSession(
      voiceRef,
      {
        onListeningChange: (v) => setListening(v),
        onStatus: (s) => setVoiceStatus(s),
        onError: (message) => {
          setVoiceStatus(null);
          setListening(false);
          pushActivity({
            kind: "system",
            title: "Voice failed",
            detail: message,
            status: "failed",
          });
        },
        onFinal: () => setVoiceStatus(null),
      },
      {
        apiKey: apiKey || undefined,
        accessToken: oauth?.accessToken,
        tokens: oauth,
      },
    );

    if (result === "started") {
      pushActivity({
        kind: "system",
        title: "Listening",
        detail: "Speak, then click the mic again to stop and transcribe with Grok.",
        status: "running",
      });
      return;
    }

    if (result === "error" && !voiceRef.current?.isListening()) {
      type Rec = {
        continuous: boolean;
        interimResults: boolean;
        lang: string;
        onresult: ((ev: {
          resultIndex: number;
          results: ArrayLike<ArrayLike<{ transcript: string }> & { isFinal?: boolean }>;
        }) => void) | null;
        onerror: ((ev?: { error?: string }) => void) | null;
        onend: (() => void) | null;
        start: () => void;
        stop: () => void;
      };
      const w = window as unknown as {
        SpeechRecognition?: new () => Rec;
        webkitSpeechRecognition?: new () => Rec;
      };
      const SR = w.SpeechRecognition || w.webkitSpeechRecognition;
      if (!SR) return;
      try {
        const rec = new SR();
        rec.continuous = true;
        rec.interimResults = false;
        rec.lang = navigator.language || "en-US";
        rec.onresult = (ev) => {
          let chunk = "";
          for (let i = ev.resultIndex; i < ev.results.length; i++) {
            const r = ev.results[i];
            if (r && r[0]) chunk += r[0].transcript;
          }
          if (chunk.trim()) {
            setText((prev) => {
              const base = prev.trim();
              return base ? `${base} ${chunk.trim()}` : chunk.trim();
            });
            requestAnimationFrame(() => resizeComposer());
          }
        };
        rec.onerror = (ev) => {
          stopVoice();
          pushActivity({
            kind: "system",
            title: "Voice error",
            detail: ev?.error || "Speech recognition error",
            status: "failed",
          });
        };
        rec.onend = () => setListening(false);
        recognitionRef.current = rec;
        rec.start();
        setListening(true);
        setVoiceStatus("Listening (browser speech)…");
      } catch (e) {
        pushActivity({
          kind: "system",
          title: "Mic failed",
          detail: e instanceof Error ? e.message : "Could not start voice",
          status: "failed",
        });
      }
    }
  }

  const copyMessage = useCallback(async (id: string, content: string) => {
    try {
      await navigator.clipboard.writeText(content);
      setCopiedId(id);
      window.setTimeout(() => setCopiedId((c) => (c === id ? null : c)), 1600);
    } catch {
      try {
        const ta = document.createElement("textarea");
        ta.value = content;
        ta.style.position = "fixed";
        ta.style.left = "-9999px";
        document.body.appendChild(ta);
        ta.select();
        document.execCommand("copy");
        document.body.removeChild(ta);
        setCopiedId(id);
        window.setTimeout(() => setCopiedId((c) => (c === id ? null : c)), 1600);
      } catch {
        /* ignore */
      }
    }
  }, []);

  const replyToMessage = useCallback(
    (m: { id: string; content: string; role: ChatRole }) => {
      setReplyTo({ id: m.id, content: m.content, role: m.role });
      window.setTimeout(() => {
        inputRef.current?.focus();
      }, 50);
    },
    [setReplyTo],
  );

  const deleteMessage = useCallback(
    (id: string) => {
      if (typeof window !== "undefined" && !window.confirm("Delete this message?")) return;
      deleteChatMessages(id);
      if (editingId === id) setEditingId(null);
    },
    [deleteChatMessages, editingId],
  );

  const jumpToReply = useCallback(
    (id: string | undefined) => {
      if (!id) return;
      const el = document.getElementById(`msg-${id}`);
      if (el) {
        el.scrollIntoView({ behavior: "smooth", block: "center" });
        return;
      }
      const idx = chat.findIndex((m) => m.id === id);
      if (idx < 0) return;
      const fromEnd = chat.length - idx;
      const need = Math.max(0, fromEnd - WINDOW);
      pendingJumpId.current = id;
      setHistoryExtra((n) => Math.max(n, need + WINDOW));
    },
    [chat],
  );

  const onStartEdit = useCallback((id: string, content: string) => {
    setEditingId(id);
    setEditDraft(content);
  }, []);

  const onSaveEdit = useCallback(
    (id: string, resend: boolean) => {
      void editChatMessage(id, editDraft, resend);
      setEditingId(null);
    },
    [editChatMessage, editDraft],
  );

  async function onSend(value?: string) {
    if (busy) return;
    stickToBottomRef.current = true;
    setShowJumpLatest(false);
    let payload = (value ?? text).trim();
    if (attachments.length) {
      const blocks = attachments.map((a) => {
        if (a.kind.startsWith("image/") || a.dataUrl.startsWith("data:image/")) {
          return `![${a.name}](${a.dataUrl})`;
        }
        const body = a.dataUrl.startsWith("data:")
          ? "_binary attachment (preview omitted)_"
          : a.dataUrl;
        const fence = a.name.match(/\.(\w+)$/)?.[1] || "";
        return [
          `Attached file: **${a.name}** (\`${a.kind}\`)`,
          "",
          "```" + fence,
          body.slice(0, 120_000),
          "```",
        ].join("\n");
      });
      payload = [payload, ...blocks].filter(Boolean).join("\n\n");
    }
    if (!payload) return;
    recordQuickAssistTyped(payload);
    if (
      payload.toLowerCase().includes("imagine") &&
      !payload.startsWith("/") &&
      !payload.startsWith("$")
    ) {
      setNav("imagine");
    }
    setText("");
    setAttachments([]);
    setComposerDraft(activeThreadId, "");
    if (payload.startsWith("$") || payload.startsWith("/sh")) {
      pushShellHistory(payload);
    }
    if (payload.startsWith("$") || payload.startsWith("/sh ")) {
      await runShell(payload);
      return;
    }
    setPendingBusy(true);
    try {
      await sendChat(payload);
    } finally {
      setPendingBusy(false);
    }
  }

  function onStop() {
    setPendingBusy(false);
    if (localRunning || localHostJobRef.current) {
      const jid = localHostJobRef.current;
      localHostJobRef.current = null;
      setLocalRunning(false);
      if (jid) {
        void import("@/lib/host-client").then(({ hostKillExec }) => hostKillExec(jid)).catch(() => {});
      }
      useGrokHub.setState({ running: false, streamStatus: null });
      return;
    }
    stopChat();
  }

  return (
    <div className="chat-stage mx-auto flex h-full min-h-0 w-full flex-col">
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden bg-[var(--color-bg)]">
        {/* Project workspace bar */}
        <ProjectBar />
        {/* Slim toolbar — context budget + export */}
        {findOpen && (
          <div className="flex shrink-0 items-center gap-2 border-b border-[var(--color-border)] bg-[var(--color-panel)] px-4 py-1.5 md:px-6">
            <Search className="h-3.5 w-3.5 text-[var(--color-subtle)]" />
            <input
              autoFocus
              value={findQ}
              onChange={(e) => setFindQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  setFindOpen(false);
                  setFindQ("");
                }
                if (e.key === "Enter") {
                  e.preventDefault();
                  if (!findMatches.length) return;
                  setFindIdx((i) =>
                    e.shiftKey
                      ? (i - 1 + findMatches.length) % findMatches.length
                      : (i + 1) % findMatches.length,
                  );
                }
              }}
              placeholder="Find in chat…"
              className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-xs outline-none focus:ring-1 focus:ring-[var(--color-ring)]"
              aria-label="Find in chat"
            />
            <span className="shrink-0 text-[10px] tabular text-[var(--color-subtle)]">
              {findQ.trim()
                ? findMatches.length
                  ? `${findIdx + 1}/${findMatches.length}`
                  : "0"
                : "—"}
            </span>
            <Button size="sm" variant="ghost" onClick={() => { setFindOpen(false); setFindQ(""); }}>
              Close
            </Button>
          </div>
        )}
        <div className="chat-meta-bar flex shrink-0 items-center justify-between gap-2 px-4 py-1 md:px-6">
          <ContextBudgetChip />
          <div className="flex items-center gap-1">
            <Button
              size="sm"
              variant="ghost"
              disabled={busy || chat.length < 12}
              title="Compact older turns into a summary (frees API window)"
              onClick={() => {
                const r = useGrokHub.getState().compactThread();
                useGrokHub.getState().pushActivity({
                  kind: "chat",
                  title: r.ok ? "Compacted" : "Compact skipped",
                  detail: r.detail.slice(0, 160),
                  status: r.ok ? "success" : "failed",
                });
              }}
            >
              Compact
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={!chat.length}
              title="Export chat as Markdown"
              onClick={() => {
                const md = exportThreadMarkdown();
                const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
                const a = document.createElement("a");
                a.href = URL.createObjectURL(blob);
                a.download = `grokhub-chat-${Date.now()}.md`;
                a.click();
                URL.revokeObjectURL(a.href);
              }}
            >
              <Download className="h-3.5 w-3.5" />
              Export
            </Button>
          </div>
        </div>
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div className="shrink-0 space-y-2 px-4 pt-3 md:px-6 3xl:px-8">
            {proactiveNotice ? (
              <div className="flex items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-success)_35%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-success)_10%,var(--color-elevated))] px-3 py-2 text-[12px] text-[var(--color-fg)]">
                <span className="min-w-0">
                  <span className="font-medium text-[var(--color-success)]">Self-fixed · </span>
                  <span className="text-[var(--color-muted)]">{proactiveNotice.message}</span>
                </span>
                <button
                  type="button"
                  className="shrink-0 text-[11px] text-[var(--color-muted)] underline-offset-2 hover:underline"
                  onClick={() => dismissProactiveNotice()}
                >
                  Dismiss
                </button>
              </div>
            ) : null}
            <HostGatewayBanner variant="compact" />
            {sessionResume?.kind === "interrupted" && !busy && (
              <div className="flex flex-wrap items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-warn)_40%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-warn)_10%,var(--color-elevated))] px-3 py-2">
                <div className="min-w-0">
                  <div className="text-xs font-semibold text-[var(--color-warn)]">
                    Stream interrupted
                  </div>
                  <div className="truncate text-[11px] text-[var(--color-muted)]">
                    {sessionResume.title}
                    {sessionResume.preview ? ` — ${sessionResume.preview}` : ""}
                  </div>
                </div>
                <div className="flex shrink-0 flex-wrap gap-1.5">
                  <Button
                    size="sm"
                    disabled={busy}
                    onClick={() => {
                      void (async () => {
                        setPendingBusy(true);
                        try {
                          await continueInterruptedSession();
                        } finally {
                          setPendingBusy(false);
                        }
                        requestAnimationFrame(() => {
                          stickToBottomRef.current = true;
                          endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
                          inputRef.current?.focus();
                        });
                      })();
                    }}
                  >
                    Continue
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => {
                      resumeLastSession();
                      requestAnimationFrame(() => {
                        stickToBottomRef.current = true;
                        endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
                        if (sessionResume.pendingPrompt) setText(sessionResume.pendingPrompt);
                        inputRef.current?.focus();
                        resizeComposer();
                      });
                    }}
                  >
                    Open chat
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => dismissSessionResume()}>
                    Dismiss
                  </Button>
                </div>
              </div>
            )}
            {!busy &&
              sessionResume?.kind !== "interrupted" &&
              (() => {
                const lastAsst = [...chat]
                  .reverse()
                  .find((m) => m.role === "assistant" && !m.streaming);
                const lastUser = [...chat].reverse().find((m) => m.role === "user");
                if (!lastAsst) return null;
                if (
                  !looksLikeIncompleteAgentTurn(lastAsst.content || "", {
                    userPrompt: lastUser?.content,
                  })
                ) {
                  return null;
                }
                return (
                  <div className="flex flex-wrap items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-info)_40%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-info)_10%,var(--color-elevated))] px-3 py-2">
                    <div className="min-w-0">
                      <div className="text-xs font-semibold text-[var(--color-info)]">
                        Agent may have stopped early
                      </div>
                      <div className="truncate text-[11px] text-[var(--color-muted)]">
                        Planning-only or unfinished reply — keep going until the goal is done
                      </div>
                    </div>
                    <div className="flex shrink-0 flex-wrap gap-1.5">
                      <Button
                        size="sm"
                        disabled={busy}
                        onClick={() => {
                          void (async () => {
                            setPendingBusy(true);
                            try {
                              await keepGoingChat();
                            } finally {
                              setPendingBusy(false);
                            }
                            requestAnimationFrame(() => {
                              stickToBottomRef.current = true;
                              endRef.current?.scrollIntoView({
                                behavior: "smooth",
                                block: "end",
                              });
                            });
                          })();
                        }}
                      >
                        Keep going
                      </Button>
                    </div>
                  </div>
                );
              })()}
          </div>
          <div
            ref={listRef}
            onScroll={onListScroll}
            className="scroll-panel min-h-0 flex-1 space-y-3 px-4 py-4 md:px-6 3xl:px-10 uw:px-16"
          >
            {chat.length === 0 && !busy && (
              <div className="mx-auto flex max-w-md flex-col items-center justify-center gap-3 py-20 text-center">
                <div className="flex h-12 w-12 items-center justify-center rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-panel)] shadow-[var(--shadow-hairline)]">
                  <span className="text-sm font-semibold tracking-tight text-[var(--color-fg)]">
                    G
                  </span>
                </div>
                <div className="space-y-1.5">
                  <p className="text-base font-semibold tracking-tight text-[var(--color-fg)]">
                    {welcomeBusy && !welcomeMessage
                      ? "Getting ready…"
                      : welcomeMessage?.headline || "What's next?"}
                  </p>
                  <p className="text-sm leading-relaxed text-[var(--color-muted)]">
                    {welcomeMessage?.body || (
                      <>
                        Ask anything, run{" "}
                        <span className="font-mono text-[var(--color-fg)]">$ shell</span> commands,
                        or try{" "}
                        <span className="font-mono text-[var(--color-fg)]">/help</span> for slash
                        commands.
                      </>
                    )}
                  </p>
                  {welcomeMessage?.source === "llm" && (
                    <p className="text-[10px] text-[var(--color-subtle)]">Personal welcome · Fast</p>
                  )}
                </div>
              </div>
            )}
            {hiddenCount > 0 && (
              <div className="flex justify-center">
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => setHistoryExtra((n) => n + WINDOW)}
                >
                  Show {Math.min(WINDOW, hiddenCount)} earlier · {hiddenCount} hidden
                </Button>
              </div>
            )}
            {visibleChat.map((m) => (
              <ChatMessageRow
                key={m.id}
                m={m}
                busy={busy}
                streamStatus={streamStatus}
                streamingMessageId={streamingMessageId}
                findHit={findOpen && findMatches.includes(m.id)}
                isLastAssistant={m.id === lastAssistantId}
                onRegenerate={() => void regenerateLast()}
                editingId={editingId}
                editDraft={editDraft}
                copiedId={copiedId}
                onJumpReply={jumpToReply}
                onReply={replyToMessage}
                onCopy={copyMessage}
                onStartEdit={onStartEdit}
                onEditDraft={setEditDraft}
                onSaveEdit={onSaveEdit}
                onCancelEdit={() => setEditingId(null)}
                onDelete={deleteMessage}
                onRate={(id, positive) => rateMessage(id, positive)}
              />
            ))}
            {showJumpLatest && (
              <div className="sticky bottom-2 z-10 flex justify-center">
                <button
                  type="button"
                  className="rounded-full border border-[var(--color-border)] bg-[var(--color-panel)] px-3 py-1 text-xs font-medium shadow-sm hover:border-[var(--color-border-strong)]"
                  onClick={() => {
                    stickToBottomRef.current = true;
                    setShowJumpLatest(false);
                    endRef.current?.scrollIntoView({ behavior: "smooth" });
                  }}
                >
                  Jump to latest
                </button>
              </div>
            )}
            <div ref={endRef} />
          </div>

          <div className="composer-dock shrink-0 space-y-2 p-3 md:p-4 3xl:px-8 uw:px-12">
            {replyTo && (
              <div className="mx-auto mb-2 flex w-full max-w-[min(56rem,100%)] items-start gap-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-info)_40%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-info)_8%,var(--color-surface))] px-3 py-2 3xl:max-w-[min(64rem,100%)] uw:max-w-[min(72rem,100%)]">
                <Reply className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--color-info)]" />
                <div className="min-w-0 flex-1">
                  <div className="text-[10px] font-semibold uppercase tracking-wide text-[var(--color-info)]">
                    Replying to {replyTo.role}
                  </div>
                  <p className="line-clamp-2 text-xs text-[var(--color-muted)]">{replyTo.preview}</p>
                </div>
                <button
                  type="button"
                  className="rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]"
                  title="Cancel reply"
                  onClick={() => setReplyTo(null)}
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              </div>
            )}

            {!busy && (
              <div className="mx-auto w-full max-w-[min(56rem,100%)] 3xl:max-w-[min(64rem,100%)] uw:max-w-[min(72rem,100%)]">
                <div className="mb-1 flex flex-wrap items-center justify-center gap-2">
                  {/* Live predictive primary — updates as you type */}
                  {!chipsOpen && chips[0] ? (
                    <button
                      type="button"
                      disabled={busy}
                      title={
                        chips[0].hint
                          ? `${chips[0].hint}\n\n${chips[0].value.slice(0, 180)}`
                          : chips[0].value.slice(0, 180)
                      }
                      onClick={() => void onChip(chips[0]!)}
                      className="inline-flex max-w-[min(100%,30rem)] items-center gap-1.5 rounded-full border border-[color-mix(in_oklab,var(--color-accent)_40%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-accent)_8%,var(--color-surface))] px-3 py-1 text-[11px] font-medium text-[var(--color-fg)] shadow-sm hover:border-[var(--color-accent)] disabled:opacity-50"
                    >
                      <Sparkles className="h-3 w-3 shrink-0 text-[var(--color-accent)]" />
                      <span className="truncate">{chips[0].label}</span>
                      {text.trim().length >= 2 ? (
                        <span className="shrink-0 rounded-full bg-[var(--color-elevated)] px-1.5 text-[9px] font-normal text-[var(--color-subtle)]">
                          as you type
                        </span>
                      ) : chips[0].hint?.toLowerCase().includes("predict") ? (
                        <span className="shrink-0 rounded-full bg-[var(--color-elevated)] px-1.5 text-[9px] font-normal text-[var(--color-subtle)]">
                          predicted
                        </span>
                      ) : null}
                    </button>
                  ) : null}
                  <button
                    type="button"
                    onClick={() => setChipsOpen((v) => !v)}
                    className="inline-flex items-center gap-1.5 rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]"
                  >
                    <Sparkles className="h-3 w-3" />
                    {chipsOpen ? "Hide" : chips[0] ? "More" : "Suggestions"}
                    {chips.length > 0 ? (
                      <span className="rounded-full bg-[var(--color-elevated)] px-1.5 font-mono text-[10px]">
                        {chips.length}
                      </span>
                    ) : null}
                  </button>
                  {chipsOpen && (
                    <button
                      type="button"
                      onClick={() => {
                        rotateQuickAssist();
                        setChipsOpen(true);
                      }}
                      disabled={quickAssistLlmBusy}
                      className="inline-flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2 py-1 text-[10px] text-[var(--color-muted)] hover:text-[var(--color-fg)] disabled:opacity-50"
                      title="Generate new suggestions (Fast mode + habits)"
                    >
                      <RefreshCw
                        className={cn("h-2.5 w-2.5", quickAssistLlmBusy && "animate-spin")}
                      />
                      {quickAssistLlmBusy ? "Thinking…" : "Refresh"}
                    </button>
                  )}
                </div>
                {chipsOpen && chips.length > 0 && (
                  <div
                    className="flex flex-wrap items-stretch justify-center gap-2"
                    role="listbox"
                    aria-label="Quick assistant suggestions"
                  >
                    {chips.map((c, idx) => {
                      const Icon = chipIcon(c.kind);
                      const isPrimary = Boolean(c.primary) || idx === 0;
                      const tip = c.hint
                        ? `${c.hint}${c.value.startsWith("__") ? "" : `\n\nSends: ${c.value.slice(0, 180)}`}`
                        : c.value.startsWith("__")
                          ? c.label
                          : c.value;
                      return (
                        <div
                          key={c.id + String(quickAssistRotation)}
                          className={cn(
                            "group relative inline-flex max-w-[min(100%,22rem)] items-start gap-1 rounded-2xl border pl-3 pr-1 py-1 text-left text-xs transition-colors",
                            isPrimary
                              ? "border-[color-mix(in_oklab,var(--color-accent)_45%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-accent)_10%,var(--color-surface))] text-[var(--color-fg)] shadow-sm"
                              : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]",
                            c.kind === "shell" && "font-mono",
                          )}
                        >
                          <button
                            type="button"
                            role="option"
                            disabled={busy}
                            title={tip}
                            onClick={() => void onChip(c)}
                            className="flex min-w-0 flex-1 items-start gap-1.5 py-0.5 text-left disabled:opacity-50"
                          >
                            <Icon
                              className={cn(
                                "mt-0.5 h-3 w-3 shrink-0",
                                isPrimary ? "text-[var(--color-accent)] opacity-100" : "opacity-70",
                              )}
                            />
                            <span className="flex min-w-0 flex-col gap-0.5">
                              <span className="whitespace-normal break-words leading-snug font-medium">
                                {c.label}
                              </span>
                              {isPrimary && c.hint ? (
                                <span className="line-clamp-1 text-[10px] font-normal text-[var(--color-subtle)]">
                                  {c.hint}
                                </span>
                              ) : null}
                            </span>
                          </button>
                          <button
                            type="button"
                            className="mt-0.5 shrink-0 rounded p-0.5 text-[var(--color-subtle)] opacity-60 hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)] hover:opacity-100"
                            title="Dismiss"
                            aria-label={`Dismiss ${c.label}`}
                            onClick={(e) => {
                              e.preventDefault();
                              e.stopPropagation();
                              dismissQuickAssistChip(c);
                            }}
                          >
                            <X className="h-3 w-3" />
                          </button>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            )}

            {pendingHostConfirm && (
              <div
                role="alertdialog"
                aria-modal="true"
                aria-labelledby="host-confirm-title"
                aria-live="assertive"
                className="mx-auto w-full max-w-[min(56rem,100%)] rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-warn)_45%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-warn)_10%,var(--color-surface))] p-3 shadow-md 3xl:max-w-[min(64rem,100%)]"
              >
                <div
                  id="host-confirm-title"
                  className="mb-2 flex items-center gap-2 text-sm font-medium text-[var(--color-fg)]"
                >
                  <ShieldAlert className="h-4 w-4 text-[var(--color-warn)]" />
                  Allow host commands?
                </div>
                <ul className="mb-3 max-h-28 space-y-1 overflow-y-auto font-mono text-xs text-[var(--color-muted)]">
                  {pendingHostConfirm.cmds.map((c, i) => (
                    <li key={c + i} className="break-all">
                      <span className="text-[var(--color-subtle)]">
                        [{pendingHostConfirm.risks[i] || "run"}]
                      </span>{" "}
                      $ {c}
                    </li>
                  ))}
                </ul>
                <label className="mb-2 flex cursor-pointer items-center gap-2 text-xs text-[var(--color-muted)]">
                  <input
                    type="checkbox"
                    checked={alwaysAllowHost}
                    onChange={(e) => setAlwaysAllowHost(e.target.checked)}
                    className="rounded border-[var(--color-border)]"
                  />
                  Always allow these command prefixes
                </label>
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    size="sm"
                    autoFocus
                    onClick={() => {
                      if (alwaysAllowHost && pendingHostConfirm) {
                        for (const c of pendingHostConfirm.cmds) {
                          const pref = c.trim().split(/\s+/)[0];
                          if (pref) addHostAllow(pref);
                        }
                      }
                      resolveHostConfirm(true);
                      setAlwaysAllowHost(false);
                    }}
                  >
                    Run on this machine
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => {
                      resolveHostConfirm(false);
                      setAlwaysAllowHost(false);
                    }}
                  >
                    Cancel
                  </Button>
                  <span className="text-[10px] text-[var(--color-subtle)]">
                    Enter to run · Esc to cancel
                  </span>
                </div>
              </div>
            )}

            {attachError && (
              <div className="mx-auto w-full max-w-[min(56rem,100%)] text-xs text-[var(--color-danger)]">
                {attachError} · images & text files under ~4 MB · paste or drop
              </div>
            )}
            {attachments.length > 0 && (
              <div className="mx-auto flex w-full max-w-[min(56rem,100%)] flex-wrap gap-2">
                {attachments.map((a, i) => (
                  <div
                    key={a.name + i}
                    className="relative flex items-center gap-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1.5"
                  >
                    {a.kind.startsWith("image/") ? (
                      // eslint-disable-next-line @next/next/no-img-element
                      <img src={a.dataUrl} alt="" className="h-10 w-10 rounded object-cover" />
                    ) : (
                      <span className="font-mono text-[10px]">{a.kind}</span>
                    )}
                    <span className="max-w-[8rem] truncate text-[11px]">{a.name}</span>
                    <button
                      type="button"
                      className="rounded p-0.5 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                      onClick={() => setAttachments((list) => list.filter((_, j) => j !== i))}
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </div>
                ))}
              </div>
            )}

            {voiceStatus && (
              <div className="mx-auto w-full max-w-[min(56rem,100%)] px-1 text-[10px] text-[var(--color-info)] 3xl:max-w-[min(64rem,100%)]">
                {voiceStatus}
              </div>
            )}

            <form
              className="mx-auto flex w-full max-w-[min(56rem,100%)] gap-2 3xl:max-w-[min(64rem,100%)] uw:max-w-[min(72rem,100%)]"
              onDragOver={(e) => {
                e.preventDefault();
                e.stopPropagation();
              }}
              onDrop={(e) => {
                e.preventDefault();
                e.stopPropagation();
                if (e.dataTransfer?.files?.length) void addFiles(e.dataTransfer.files);
              }}
              onSubmit={(e) => {
                e.preventDefault();
                if (pendingHostConfirm) return;
                if (busy) {
                  onStop();
                  return;
                }
                void onSend();
              }}
            >
              <input
                ref={fileRef}
                type="file"
                className="hidden"
                multiple
                accept="image/*,.png,.jpg,.jpeg,.webp,.gif,.txt,.md,.json,.csv,.log,.ts,.tsx,.js,.jsx,.py,.rs,.go,.pdf,.zip"
                onChange={(e) => {
                  if (e.target.files?.length) void addFiles(e.target.files);
                  e.target.value = "";
                }}
              />
              <div className="flex shrink-0 gap-1">
                <Button
                  type="button"
                  size="icon"
                  variant="secondary"
                  disabled={busy || Boolean(pendingHostConfirm)}
                  title="Attach image or file"
                  aria-label="Attach file"
                  className="h-9 w-9 sm:h-10 sm:w-10"
                  onClick={() => fileRef.current?.click()}
                >
                  <Paperclip className="h-4 w-4" />
                </Button>
                <EmojiPicker
                  open={emojiOpen}
                  onOpenChange={setEmojiOpen}
                  disabled={busy || Boolean(pendingHostConfirm)}
                  onPick={insertEmoji}
                />
                <Button
                  type="button"
                  size="icon"
                  variant={listening ? "default" : "secondary"}
                  disabled={busy || Boolean(pendingHostConfirm)}
                  title={
                    listening
                      ? "Stop & transcribe with Grok"
                      : "Voice input — click to talk, click again to stop"
                  }
                  aria-label="Voice mode"
                  onClick={() => void toggleVoice()}
                  className={
                    listening
                      ? "h-9 w-9 recording-pulse border border-[color-mix(in_oklab,var(--color-danger)_45%,transparent)] text-[var(--color-danger)] sm:h-10 sm:w-10"
                      : "h-9 w-9 sm:h-10 sm:w-10"
                  }
                >
                  {listening ? <MicOff className="h-4 w-4" /> : <Mic className="h-4 w-4" />}
                </Button>
              </div>
              <div className="relative min-w-0 flex-1">
              <SlashAutocomplete
                draft={text}
                open={slashOpen && !busy}
                onClose={() => setSlashOpen(false)}
                onPick={(s: SlashDef) => {
                  const ins = s.insert ?? (s.runOnPick ? s.cmd : s.cmd + (s.cmd.endsWith(" ") ? "" : " "));
                  setText(ins);
                  setSlashOpen(false);
                  requestAnimationFrame(() => {
                    inputRef.current?.focus();
                    resizeComposer();
                    if (s.runOnPick) {
                      void sendChat(ins.trim());
                      setText("");
                    }
                  });
                }}
              />
              <Textarea
                ref={inputRef}
                data-composer
                value={text}
                onChange={(e) => {
                  const v = e.target.value;
                  setText(v);
                  setSlashOpen(v.trimStart().startsWith("/") || v.trimStart() === "$");
                  setShellHistIdx(-1);
                  resizeComposer(e.target);
                }}
                placeholder={
                  busy
                    ? "Agent running — press Stop to interrupt…"
                    : "Message Grok…  type / for commands · Enter send · $ shell"
                }
                rows={1}
                className="max-h-40 min-h-[2.5rem] flex-1 resize-none overflow-hidden leading-5"
                style={{ height: 40 }}
                onPaste={(e) => {
                  const items = e.clipboardData?.items;
                  if (!items) return;
                  const files: File[] = [];
                  for (const it of items) {
                    if (it.kind === "file") {
                      const f = it.getAsFile();
                      if (f) files.push(f);
                    }
                  }
                  if (files.length) {
                    e.preventDefault();
                    void addFiles(files);
                  }
                  requestAnimationFrame(() => resizeComposer());
                }}
                onInput={(e) => resizeComposer(e.currentTarget)}
                onKeyDown={(e) => {
                  if (e.key === "Escape" && busy) {
                    e.preventDefault();
                    onStop();
                    return;
                  }
                  if (e.key === "Escape" && slashOpen) {
                    e.preventDefault();
                    setSlashOpen(false);
                    return;
                  }
                  // Shell history when empty or starts with $
                  if (
                    (e.key === "ArrowUp" || e.key === "ArrowDown") &&
                    shellHistory.length &&
                    (text === "" || text.startsWith("$") || text.startsWith("/sh"))
                  ) {
                    e.preventDefault();
                    const next =
                      e.key === "ArrowUp"
                        ? Math.min(shellHistIdx + 1, shellHistory.length - 1)
                        : Math.max(shellHistIdx - 1, -1);
                    setShellHistIdx(next);
                    if (next < 0) setText("");
                    else {
                      const cmd = shellHistory[next] || "";
                      setText(cmd.startsWith("$") || cmd.startsWith("/") ? cmd : `$ ${cmd}`);
                    }
                    return;
                  }
                  if (e.key === "Tab" && slashOpen) {
                    // handled by SlashAutocomplete capture
                    return;
                  }
                  if (e.key === "Enter" && !e.shiftKey && !busy) {
                    if (slashOpen) {
                      // let Tab accept; Enter still sends unless autocomplete steals Tab only
                    }
                    e.preventDefault();
                    void onSend();
                  }
                }}
              />
              </div>
              {busy ? (
                <Button
                  type="button"
                  variant="secondary"
                  size="icon"
                  onClick={onStop}
                  aria-label="Stop"
                  title="Stop (Esc)"
                  className="h-9 w-9 border border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] text-[var(--color-danger)] sm:h-10 sm:w-10"
                >
                  <Square className="h-3.5 w-3.5 fill-current" />
                </Button>
              ) : (
                <Button
                  type="submit"
                  disabled={!text.trim() && attachments.length === 0}
                  size="icon"
                  aria-label="Send"
                  className="h-9 w-9 sm:h-10 sm:w-10"
                >
                  <Send className="h-4 w-4" />
                </Button>
              )}
            </form>
            {!busy && (
              <div className="mx-auto w-full max-w-[min(56rem,100%)] text-center text-[10px] text-[var(--color-subtle)]">
                <span className="font-mono">/help</span> · <span className="font-mono">Ctrl+K</span> palette ·{" "}
                <span className="font-mono">Ctrl+N</span> new · <span className="font-mono">Ctrl+L</span> focus · paste
                images
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
