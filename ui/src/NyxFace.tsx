import { memo } from "react";

/** Nyx's face: 32 px by Grok's replies, 96 px in the empty chat. It only
 * decorates; the reply itself carries the meaning. */
export const NyxFace = memo(function NyxFace({ url, size }: { url: string; size: 32 | 96 }) {
  return (
    <img
      className={`nyx-face nyx-face-${size}`}
      src={url}
      width={size}
      height={size}
      alt=""
      aria-hidden="true"
      draggable={false}
    />
  );
});
