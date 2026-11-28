import { describe, expect, it } from "vitest";
import {
  isInsideSafeTriangle,
  nearestEdgeCorners,
  pointInTriangle,
  safeTriangle,
  triangleArea2,
  type Point,
  type Rect,
} from "./safeTriangle";

const rightSubmenu: Rect = { left: 200, top: 100, right: 360, bottom: 280 };
const leftSubmenu: Rect = { left: 20, top: 100, right: 180, bottom: 280 };

describe("safeTriangle geometry", () => {
  it("returns true for a point inside a right-opening triangle", () => {
    const anchor: Point = { x: 100, y: 150 };
    // Midway between the anchor and the left edge of the right-opening submenu.
    const inside: Point = { x: 150, y: 160 };
    expect(isInsideSafeTriangle(inside, anchor, rightSubmenu, "right")).toBe(true);
    const t = safeTriangle(anchor, rightSubmenu, "right");
    expect(pointInTriangle(inside, t.a, t.b, t.c)).toBe(true);
  });

  it("returns true for a point inside a left-flipped triangle", () => {
    const anchor: Point = { x: 240, y: 150 };
    const inside: Point = { x: 210, y: 160 };
    expect(isInsideSafeTriangle(inside, anchor, leftSubmenu, "left")).toBe(true);
  });

  it("returns false for a point outside the corridor", () => {
    const anchor: Point = { x: 100, y: 150 };
    // Far above the triangle, over a sibling row.
    const outside: Point = { x: 120, y: 40 };
    expect(isInsideSafeTriangle(outside, anchor, rightSubmenu, "right")).toBe(false);
  });

  it("returns false for a degenerate zero-area triangle", () => {
    const a: Point = { x: 10, y: 10 };
    const b: Point = { x: 20, y: 20 };
    const c: Point = { x: 30, y: 30 }; // collinear with a,b
    expect(triangleArea2(a, b, c)).toBe(0);
    expect(pointInTriangle({ x: 15, y: 15 }, a, b, c)).toBe(false);
    // Flat submenu (top === bottom) yields a zero-area corridor.
    const flat: Rect = { left: 200, top: 100, right: 360, bottom: 100 };
    expect(isInsideSafeTriangle({ x: 150, y: 100 }, { x: 100, y: 100 }, flat, "right")).toBe(false);
  });

  it("pads the nearest edge so grazing the border stays inside", () => {
    const anchor: Point = { x: 100, y: 190 };
    const edge = nearestEdgeCorners(rightSubmenu, "right", 4);
    expect(edge.top).toEqual({ x: 196, y: 96 });
    expect(edge.bottom).toEqual({ x: 196, y: 284 });
    // Just above the unpadded top corner — outside without pad, inside with pad.
    const graze: Point = { x: 196, y: 97 };
    expect(isInsideSafeTriangle(graze, anchor, rightSubmenu, "right", 0)).toBe(false);
    expect(isInsideSafeTriangle(graze, anchor, rightSubmenu, "right", 4)).toBe(true);
  });
});
