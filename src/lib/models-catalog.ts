/** Pick Max/flagship from live list — always grok-4.5 class, never multi-agent or 4.20. */
export function pickFlagshipModel(liveIds: string[]): string {
  const live = (liveIds || []).filter((id) => id && !SKIP_MODEL_RE.test(id) && !isMultiAgentModel(id));
  // Exact-ish 4.5 first
  const preferred = [
    "grok-4.5",
    "grok-4-5",
    "grok-4.5-latest",
    "grok-4-5-latest",
  ];
  for (const want of preferred) {
    const hit = live.find((id) => id.toLowerCase() === want.toLowerCase());
    if (hit) return hit;
  }
  const fourFive = live.find((id) => isFlagshipModel(id));
  if (fourFive) return fourFive;
  // Live partial: starts with grok-4.5 but not multi-agent
  const soft = live.find(
    (id) => /grok-4[.-]5/i.test(id) && !/4[.-]20|reasoning|multi/i.test(id),
  );
  if (soft) return soft;
  // Next-best live single-agent flagship-ish (4.3 / 4) — never 4.20 multi-agent
  for (const want of ["grok-4.3", "grok-4", "grok-3"]) {
    const hit = live.find(
      (id) =>
        id.toLowerCase() === want ||
        (id.toLowerCase().startsWith(want) &&
          !/[0-9]/.test(id[want.length] || "") &&
          !isMultiAgentModel(id) &&
          !/4[.-]?20/i.test(id)),
    );
    if (hit) return hit;
  }
  return "grok-4.5";
}
