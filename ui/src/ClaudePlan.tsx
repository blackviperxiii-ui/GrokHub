import { useCallback, useEffect, useState } from "react";
import {
  claudePlanChats,
  claudePlanStatus,
  setClaudePlanAllowed,
  type ClaudeChats,
  type ClaudePlan,
  type PlanHeadroom,
} from "./api";

export const CLAUDE_PLAN_NOTE =
  "Runs Claude Code on this computer, signed in with your own Claude account. This app never sees or keeps your Claude sign-in.";
export const CLAUDE_MISSING = "Claude Code isn't on this computer. Install it from code.claude.com, then check again.";
export const CLAUDE_NO_ANSWER =
  "Claude Code is on this computer but didn't answer claude --version. In a terminal, run claude doctor, then check again.";
export const CLAUDE_ALLOW =
  "A chat you switch to it (in its tab menu) runs Claude Code in the chat's folder. For now it can only read files there, and each message counts against your plan's usage.";
export const CLAUDE_ALLOWED = "Allowed. Switch a chat to it from the chat's tab menu.";

const BILLED_BY: Record<string, string> = {
  api_key: "an Anthropic API key",
  api_key_helper: "an API key helper",
  third_party: "a cloud provider",
};

/** The line for what this computer's Claude Code can do. */
export function claudePlanLine(plan: ClaudePlan): string {
  switch (plan.state) {
    case "missing":
      return CLAUDE_MISSING;
    case "noAnswer":
      return CLAUDE_NO_ANSWER;
    case "signedOut":
      return `Claude Code ${plan.version}, not signed in. In a terminal, run claude auth login, then check again.`;
    case "plan":
      return `Claude Code ${plan.version}, signed in with your Claude plan. Chats would count against your plan's usage.`;
    case "otherBilling":
      return `Claude Code ${plan.version}, signed in with ${BILLED_BY[plan.method] ?? plan.method}. Chats would be billed there, not to a Claude plan.`;
  }
}

const WINDOWS: Record<string, string> = {
  five_hour: "5-hour limit",
  seven_day: "weekly limit",
  seven_day_opus: "weekly Opus limit",
  seven_day_sonnet: "weekly Sonnet limit",
};

/** How the line names times: the user's own clock unless a test fixes them. */
export interface Clock {
  timeZone?: string;
  locale?: string;
}

/** A reading with no reset time is dropped after the shortest window. */
const UNTIMED_SECS = 5 * 60 * 60;

/** A time in the user's own clock. */
function at(secs: number, clock: Clock): string {
  return new Date(secs * 1000).toLocaleString(clock.locale, {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    timeZone: clock.timeZone,
  });
}

/**
 * The plan's limit as the last chat turn on it saw it, said as of then, since
 * use elsewhere (Claude Code itself, claude.ai) counts too. `null` once that
 * window has reset, since the reading no longer says anything.
 */
export function headroomLine(h: PlanHeadroom, nowSecs: number, clock: Clock = {}): string | null {
  const over = h.resetsAt ?? h.seenAt + UNTIMED_SECS;
  if (over <= nowSecs) return null;
  const window = WINDOWS[h.window] ?? "usage limit";
  const seen = `Your last chat on it (${at(h.seenAt, clock)})`;
  const when = h.resetsAt === null ? null : at(h.resetsAt, clock);
  if (h.limited) {
    const again = when === null ? "" : ` Chats on it can run again ${when}.`;
    return `${seen} found your plan's ${window} reached.${again}`;
  }
  if (h.usedPct === null) return when === null ? null : `Your plan's ${window} resets ${when}.`;
  const resets = when === null ? "" : ` It resets ${when}.`;
  return `${seen} saw ${h.usedPct}% of your plan's ${window} used.${resets}`;
}

/** Settings → AI models → Claude (your plan). */
export function ClaudePlanCard() {
  const [plan, setPlan] = useState<ClaudePlan | null>(null);
  const [chats, setChats] = useState<ClaudeChats | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Busy from the start: the first check runs on open, and Check again waits for it.
  const [busy, setBusy] = useState(true);
  const [saving, setSaving] = useState(false);
  const [asking, setAsking] = useState(false);
  // Ticks each minute, so an open card drops a reading once its window resets.
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const tick = window.setInterval(() => setNow(Date.now() / 1000), 60_000);
    return () => window.clearInterval(tick);
  }, []);

  const read = useCallback(
    () =>
      Promise.all([claudePlanStatus(), claudePlanChats()]).then(
        ([next, allowed]) => {
          setPlan(next);
          setChats(allowed);
          setError(null);
        },
        (err) => setError(String(err)),
      ),
    [],
  );

  useEffect(() => {
    void read().then(() => setBusy(false));
  }, [read]);

  async function check() {
    setBusy(true);
    await read();
    setBusy(false);
  }

  async function allow(on: boolean) {
    setAsking(false);
    setSaving(true);
    try {
      setChats(await setClaudePlanAllowed(on));
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  const allowed = chats?.allowed ?? false;
  const headroom = chats?.headroom ? headroomLine(chats.headroom, now) : null;
  return (
    <div className="card" aria-label="Claude (your plan)">
      <div className="card-title">Claude (your plan)</div>
      <p className="note">{CLAUDE_PLAN_NOTE}</p>
      <div className="card-status">{plan === null ? (error ? null : "Checking…") : claudePlanLine(plan)}</div>
      {error && (
        <p className="status status--error" role="alert">
          {error}
        </p>
      )}
      {allowed && <div className="card-detail-line">{CLAUDE_ALLOWED}</div>}
      {allowed && headroom && (
        <div className={chats?.headroom?.limited ? "card-detail-line status--error" : "card-detail-line"}>
          {headroom}
        </div>
      )}
      {asking && (
        <div className="card-buttons confirm-row" role="group" aria-label="Allow chats on your Claude plan">
          <span>Allow chats on your Claude plan? {CLAUDE_ALLOW}</span>
          <button type="button" disabled={busy || saving} onClick={() => void allow(true)}>
            Allow
          </button>
          <button type="button" className="quiet" onClick={() => setAsking(false)}>
            Not now
          </button>
        </div>
      )}
      <div className="card-buttons">
        {plan?.state === "plan" && !allowed && !asking && (
          <button type="button" disabled={busy || saving} onClick={() => setAsking(true)}>
            Allow for chats
          </button>
        )}
        {allowed && (
          <button type="button" className="quiet" disabled={busy || saving} onClick={() => void allow(false)}>
            Stop using for chats
          </button>
        )}
        <button type="button" className="quiet" disabled={busy || saving} onClick={() => void check()}>
          {busy ? "Checking…" : "Check again"}
        </button>
      </div>
    </div>
  );
}
