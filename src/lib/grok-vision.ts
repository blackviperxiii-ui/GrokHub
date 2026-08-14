/**
 * Cap in-memory tool-loop history so only the last N screenshots go to the model.
 * xAI image_url hydration lives in desktop/vision-messages.cjs (used by grok-bridge).
 */

export function capHistoryImages<T extends { images?: string[] }>(
  messages: T[],
  max = 2,
): T[] {
  let kept = 0;
  const out = messages.slice();
  for (let i = out.length - 1; i >= 0; i--) {
    const imgs = out[i]?.images;
    if (!imgs?.length) continue;
    if (kept >= max) {
      out[i] = { ...out[i], images: undefined };
    } else {
      kept += 1;
    }
  }
  return out;
}

/** Drop old screenshots on the live tool-loop array so the renderer cannot OOM. */
export function capHistoryImagesInPlace<T extends { images?: string[] }>(
  messages: T[],
  max = 2,
): T[] {
  const next = capHistoryImages(messages, max);
  messages.length = 0;
  messages.push(...next);
  return messages;
}
