// Settings → Defaults: the one place the permission setting changes (plus
// Stop asking me on a permission card).
import type { Approvals } from "./api";

/** The setting's name in Settings → Defaults. */
export const APPROVALS_LABEL = "When the AI wants to change things";

export const APPROVALS: Record<Approvals, { label: string; note: string }> = {
  ask: {
    label: "Ask me first",
    note: "It asks before it changes a file or runs a command.",
  },
  auto: {
    label: "Only ask for risky things",
    note: "A quick check approves routine steps and asks you about the rest.",
  },
  always: {
    label: "Don't ask",
    note: "Files change and commands run without asking. Deleting, sending, paying and passwords still ask.",
  },
};
