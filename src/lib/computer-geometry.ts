/**
 * Map a click on an object-contain image to source-image pixels.
 * Clicks on the letterbox bars return null.
 */
export function mapContainedImageClick(
  clientX: number,
  clientY: number,
  box: { left: number; top: number; width: number; height: number },
  imageW: number,
  imageH: number,
): { x: number; y: number } | null {
  if (!box.width || !box.height || !imageW || !imageH) return null;
  const scale = Math.min(box.width / imageW, box.height / imageH);
  const contentW = imageW * scale;
  const contentH = imageH * scale;
  const ox = box.left + (box.width - contentW) / 2;
  const oy = box.top + (box.height - contentH) / 2;
  if (clientX < ox || clientY < oy || clientX > ox + contentW || clientY > oy + contentH) {
    return null;
  }
  return {
    x: Math.round(((clientX - ox) / contentW) * imageW),
    y: Math.round(((clientY - oy) / contentH) * imageH),
  };
}
