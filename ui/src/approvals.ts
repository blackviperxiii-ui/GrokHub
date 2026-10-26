// Settings' approvals, shared with the composer's chips.
import type { Approvals } from "./api";

export const APPROVALS: Record<Approvals, { label: string; note: string }> = {
  ask: {
    label: "Ask every time",
    note: "It asks before it changes a file or runs a command.",
  },
  auto: {
    label: "Auto",
    note: "A quick check approves routine steps and asks you about the rest.",
  },
  always: {
    label: "Always approve",
    note: "Files change and commands run without asking. Deleting, sending, paying and credentials still ask.",
  },
};
