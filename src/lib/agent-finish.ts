/**
 * Detect stalled / incomplete agent turns and build auto-finish nudges.
 * Used when the model stops at “let me check…” without tools or a real answer.
 */

/** Classic planning-only stall (often with no HOST_CMD). */
export function looksLikePlanningStall(text: string): boolean {
  const s = String(text || "").trim();
  if (!s) return true;
  if (/HOST_CMD\s*:/i.test(s)) return false;
  if (/CONNECTOR_CMD\s*:/i.test(s)) return false;
  // Real tool evidence already in the message
  if (/HOST_RESULT|### 🖥️|exit code|```host\b/i.test(s)) return false;

  const plan =
    /\b(i('ll| will)|let me|i can|i should|i'm going to|i am going to|going to)\b.{0,60}\b(check|probe|inspect|investigate|scan|look|run|start|continue|dig|examine|verify|read|open|list|fetch|pull|search|find|try|audit|diagnose)\b/i.test(
      s,
    ) ||
    /\b(continuing|continue)\b.{0,40}\b(deep dive|investigation|scan|probe|next|audit)\b/i.test(s) ||
    /\b(would you like me to|shall i|want me to|should i)\b.{0,50}\b(start|run|check|investigate|probe|continue|do that)\b/i.test(
      s,
    ) ||
    /\binstead of actually running\b/i.test(s) ||
    /\b(i('ll| will)\s+(do|handle|take care of)\s+(that|this|it)\b)/i.test(s) ||
    /\b(give me a (moment|sec|second)|one (sec|second|moment)|hang on)\b/i.test(s) ||
    // Announcement-only (field bug: "running checks now" then stop)
    /\b(running (checks?|diagnostics?|scan|commands?)|taking a look|looking into (it|this|that)|on it now|checking now)\b/i.test(
      s,
    ) ||
    /\b(next i('ll| will)|first i('ll| will)|then i('ll| will))\b/i.test(s) ||
    /\b(here('s| is) (what|the) (plan|approach)|i('m| am) (about to|going to) (run|check|scan))\b/i.test(
      s,
    ) ||
    /\b(ready for the next|say the word|give the word)\b/i.test(s);

  // Ends mid-thought or with a cliffhanger
  const endsOpen =
    /\b(let me|i'll|i will|next i('ll| will)|first i('ll| will))\b[^.!?]{0,100}$/i.test(s) ||
    /[:…]\s*$/.test(s) ||
    /\b(starting|working on it|on it|running checks?|stand by)\s*[.!]\s*$/i.test(s);

  // Long "report" that is really meta / excuse about not using tools
  const metaExcuse =
    s.length > 80 &&
    /\b(only describing|never output|didn't (run|emit|fire)|treating it like a normal chat|switch(ing)? into (active )?tool)\b/i.test(
      s,
    );

  return plan || endsOpen || metaExcuse;
}

/** Substantive completion markers */
export function looksLikeFinishedAnswer(text: string): boolean {
  const t = String(text || "").trim();
  if (!t) return false;
  if (
    /GOAL_COMPLETE|all done|you're all set|task is complete|fully done|nothing else to do|no further action|shipped|fixed\.|resolved\.|here('s| is) (what|the)|summary:|results?:/i.test(
      t,
    )
  )
    return true;
  // Has real structure and length without planning-only language
  if (t.length >= 220 && !looksLikePlanningStall(t) && /[.!?)]\s*$/.test(t)) return true;
  // Short definitive
  if (t.length < 160 && /\b(done|fixed|complete|applied|saved|updated|created)\b/i.test(t))
    return true;
  return false;
}

/**
 * True when the assistant turn should not be treated as finished.
 * Covers planning stalls, hollow short replies, and goal-loop incompleteness.
 */
export function looksLikeIncompleteAgentTurn(
  text: string,
  opts?: { hadTools?: boolean; userPrompt?: string },
): boolean {
  const s = String(text || "").trim();
  if (!s || s === "(empty)" || s === "_Stopped._") return true;
  if (looksLikePlanningStall(s)) return true;

  // Hollow: only meta/status without substance
  if (
    s.length < 120 &&
    /\b(working|checking|looking|investigating|one moment|stand by)\b/i.test(s) &&
    !opts?.hadTools
  )
    return true;

  // User asked for action/investigation but reply never delivered results
  const user = opts?.userPrompt || "";
  if (
    user &&
    /\b(fix|check|investigate|debug|find|why|what('s| is) wrong|deep dive|run|list|read)\b/i.test(
      user,
    ) &&
    !opts?.hadTools &&
    s.length < 400 &&
    !/\b(found|result|error:|exit |\$ |here's what|i see|looks like)\b/i.test(s)
  ) {
    return true;
  }

  if (looksLikeFinishedAnswer(s)) return false;

  // Soft incomplete cues (goal loop style)
  if (
    /next step|still need|remaining|i'll continue|continue with|partially|in progress|not done yet|blocked on|let me know if|want me to|more to do/i.test(
      s,
    )
  )
    return true;

  // Very short non-ack after a long ask
  if (user.length > 80 && s.length < 60 && !/^(yes|no|ok|done|thanks)[.!]*$/i.test(s)) {
    return true;
  }

  return false;
}

export function buildAutoFinishNudge(opts: {
  round: number;
  maxRounds: number;
  userPrompt: string;
  lastAssistant: string;
  hostAvailable: boolean;
}): string {
  const host = opts.hostAvailable
    ? "If you need the machine, emit HOST_CMD lines now (no permission asking)."
    : "Host tools are off — answer from knowledge only, but still finish.";
  return [
    "SYSTEM — AUTO-FINISH (do not mention this system note to the user):",
    `Progress ${opts.round}/${opts.maxRounds}. Your previous message stopped before the goal was complete.`,
    "Do NOT only plan. Do NOT say “let me check/look/investigate/running checks” without acting in this same reply.",
    host,
    "WRONG: prose about what you will do. RIGHT: own-line HOST_CMD then wait for results.",
    "Example (own lines):",
    "HOST_CMD: ps -eo pid,cmd --sort=-%cpu | head -25",
    'HOST_CMD: ls -la "$HOME/.local/lib/grokhub" | head -30',
    "Deliver concrete progress: tools and/or a complete answer with real data.",
    "When the user goal is fully done, end with a clear summary (and GOAL_COMPLETE if tracking a goal).",
    "If truly blocked, say GOAL_BLOCKED: <reason> and what you need.",
    "",
    `User goal was: ${opts.userPrompt.slice(0, 500)}`,
    `Your incomplete reply was: ${opts.lastAssistant.slice(0, 400)}`,
  ].join("\n");
}

/** User-facing keep-going prompt when they click Continue / Keep going */
export function buildKeepGoingUserPrompt(userGoal?: string): string {
  const goal = (userGoal || "").trim();
  return [
    "Continue from where you left off and finish the task.",
    "Do not restate a plan only — act now (HOST_CMD / CONNECTOR_CMD if needed) and deliver the complete answer.",
    goal ? `Original goal: ${goal.slice(0, 600)}` : "",
  ]
    .filter(Boolean)
    .join("\n");
}
