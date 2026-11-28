/** Pure geometry for the Amazon-style safe-triangle hover corridor.
 *
 * When a submenu is open, the corridor is the triangle from the last pointer
 * position on the parent item to the two corners of the submenu's nearest
 * edge. Pointer motion inside that triangle keeps the open submenu; motion
 * outside switches immediately. */

export type Point = { readonly x: number; readonly y: number };

/** Axis-aligned rectangle in viewport coordinates (getBoundingClientRect shape). */
export type Rect = {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
};

export type SubmenuSide = "right" | "left";

export type SafeTriangle = {
  readonly a: Point;
  readonly b: Point;
  readonly c: Point;
};

/** Signed cross product of OA × OB. Positive = B is left of OA. */
function cross(o: Point, a: Point, b: Point): number {
  return (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
}

/** Twice the signed area of triangle ABC. Zero means a degenerate (flat) triangle. */
export function triangleArea2(a: Point, b: Point, c: Point): number {
  return Math.abs(cross(a, b, c));
}

/** True when `p` lies inside or on the boundary of triangle `abc`.
 * Degenerate (zero-area) triangles never contain a point. */
export function pointInTriangle(p: Point, a: Point, b: Point, c: Point): boolean {
  if (triangleArea2(a, b, c) === 0) return false;
  const d1 = cross(p, a, b);
  const d2 = cross(p, b, c);
  const d3 = cross(p, c, a);
  const hasNeg = d1 < 0 || d2 < 0 || d3 < 0;
  const hasPos = d1 > 0 || d2 > 0 || d3 > 0;
  return !(hasNeg && hasPos);
}

/** The two corners of the submenu edge closest to the parent, optionally padded
 * outward so grazing the border never flickers the corridor off. */
export function nearestEdgeCorners(
  submenu: Rect,
  side: SubmenuSide,
  padding = 0,
): { top: Point; bottom: Point } {
  const pad = Math.max(0, padding);
  if (side === "right") {
    return {
      top: { x: submenu.left - pad, y: submenu.top - pad },
      bottom: { x: submenu.left - pad, y: submenu.bottom + pad },
    };
  }
  return {
    top: { x: submenu.right + pad, y: submenu.top - pad },
    bottom: { x: submenu.right + pad, y: submenu.bottom + pad },
  };
}

/** Build the safe triangle: anchor pointer → nearest-edge top → nearest-edge bottom. */
export function safeTriangle(
  anchor: Point,
  submenu: Rect,
  side: SubmenuSide,
  padding = 0,
): SafeTriangle {
  const edge = nearestEdgeCorners(submenu, side, padding);
  return { a: anchor, b: edge.top, c: edge.bottom };
}

/** Whether `point` lies inside the safe corridor for the open submenu. */
export function isInsideSafeTriangle(
  point: Point,
  anchor: Point,
  submenu: Rect,
  side: SubmenuSide,
  padding = 0,
): boolean {
  const t = safeTriangle(anchor, submenu, side, padding);
  return pointInTriangle(point, t.a, t.b, t.c);
}
