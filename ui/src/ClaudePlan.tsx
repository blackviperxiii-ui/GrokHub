import { useCallback, useEffect, useState } from "react";
import { claudePlanStatus, type ClaudePlan } from "./api";

export const CLAUDE_PLAN_NOTE =
  "Runs Claude Code on this computer, signed in with your own Claude account. This app never sees or keeps your Claude sign-in.";
export const CLAUDE_MISSING = "Claude Code isn't on this computer. Install it from code.claude.com, then check again.";
export const CLAUDE_SIGNED_OUT = "Claude Code is installed but not signed in. In a terminal, run claude auth login, then check again.";
export const CLAUDE_SOON = "Chats on Claude come in a later update.";

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
    case "signedOut":
      return `Claude Code ${plan.version}, not signed in. In a terminal, run claude auth login, then check again.`;
    case "plan":
      return `Claude Code ${plan.version}, signed in with your Claude plan. Chats would count against your plan's usage.`;
    case "otherBilling":
      return `Claude Code ${plan.version}, signed in with ${BILLED_BY[plan.method] ?? plan.method}. Chats would be billed there, not to a Claude plan.`;
  }
}

/** Settings → AI models → Claude (your plan). */
export function ClaudePlanCard() {
  const [plan, setPlan] = useState<ClaudePlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const read = useCallback(
    () =>
      claudePlanStatus().then(
        (next) => {
          setPlan(next);
          setError(null);
        },
        (err) => setError(String(err)),
      ),
    [],
  );

  useEffect(() => {
    void read();
  }, [read]);

  async function check() {
    setBusy(true);
    await read();
    setBusy(false);
  }

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
      {plan?.state === "plan" && <div className="card-detail-line">{CLAUDE_SOON}</div>}
      <div className="card-buttons">
        <button type="button" className="quiet" disabled={busy} onClick={() => void check()}>
          {busy ? "Checking…" : "Check again"}
        </button>
      </div>
    </div>
  );
}
