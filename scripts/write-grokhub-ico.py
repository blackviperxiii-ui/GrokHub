"""Rasterize packaging/grokhub.svg into packaging/windows/grokhub.ico."""
from __future__ import annotations

import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "packaging" / "windows" / "grokhub.ico"

BG = (10, 10, 11, 255)
STROKE = (39, 39, 42, 255)
FG = (244, 244, 245, 255)


def clamp(v: int, lo: int, hi: int) -> int:
    return max(lo, min(hi, v))


def rounded_mask(n: int, radius: float) -> list[list[float]]:
    out = [[0.0] * n for _ in range(n)]
    r = radius
    for y in range(n):
        for x in range(n):
            cx = x + 0.5
            cy = y + 0.5
            dx = 0.0
            dy = 0.0
            if cx < r:
                dx = r - cx
            elif cx > n - r:
                dx = cx - (n - r)
            if cy < r:
                dy = r - cy
            elif cy > n - r:
                dy = cy - (n - r)
            if dx == 0.0 or dy == 0.0:
                out[y][x] = 1.0
            else:
                out[y][x] = 1.0 if (dx * dx + dy * dy) <= r * r else 0.0
    return out


def blit(px: list[list[tuple[int, int, int, int]]], color, x0, y0, x1, y1, mask=None):
    h = len(px)
    w = len(px[0])
    for y in range(max(0, int(y0)), min(h, int(y1) + 1)):
        for x in range(max(0, int(x0)), min(w, int(x1) + 1)):
            if mask is not None and mask[y][x] < 0.5:
                continue
            px[y][x] = color


def paint(n: int) -> bytes:
    px = [[BG for _ in range(n)] for _ in range(n)]
    outer = rounded_mask(n, n * 0.22)
    inner_pad = max(2, n // 21)
    inner = [[0.0] * n for _ in range(n)]
    inner_r = n * 0.19
    for y in range(n):
        for x in range(n):
            if inner_pad <= x < n - inner_pad and inner_pad <= y < n - inner_pad:
                xx = x - inner_pad
                yy = y - inner_pad
                m = n - 2 * inner_pad
                r = inner_r * m / n
                cx, cy = xx + 0.5, yy + 0.5
                dx = dy = 0.0
                if cx < r:
                    dx = r - cx
                elif cx > m - r:
                    dx = cx - (m - r)
                if cy < r:
                    dy = r - cy
                elif cy > m - r:
                    dy = cy - (m - r)
                on_edge = False
                if dx == 0.0 or dy == 0.0:
                    dist_in = min(cx, cy, m - cx, m - cy)
                    on_edge = dist_in < max(1.2, n / 64)
                else:
                    d = (dx * dx + dy * dy) ** 0.5
                    on_edge = abs(d - r) < max(1.2, n / 64)
                inner[y][x] = 1.0 if on_edge else 0.0
    for y in range(n):
        for x in range(n):
            if outer[y][x] < 0.5:
                px[y][x] = (0, 0, 0, 0)
            elif inner[y][x] > 0.5:
                px[y][x] = STROKE
    # Grok-ish mark: two white strokes in the well
    s = n / 256.0
    def stroke_line(x0, y0, x1, y1, thick):
        steps = max(n, int(((x1 - x0) ** 2 + (y1 - y0) ** 2) ** 0.5) + 1)
        t = max(1, int(thick))
        for i in range(steps + 1):
            u = i / steps
            x = int(x0 + (x1 - x0) * u)
            y = int(y0 + (y1 - y0) * u)
            for dy in range(-t, t + 1):
                for dx in range(-t, t + 1):
                    if dx * dx + dy * dy <= t * t:
                        xx, yy = x + dx, y + dy
                        if 0 <= xx < n and 0 <= yy < n and outer[yy][xx] >= 0.5:
                            px[yy][xx] = FG

    stroke_line(72 * s, 168 * s, 148 * s, 88 * s, max(1, int(7 * s)))
    stroke_line(88 * s, 176 * s, 184 * s, 96 * s, max(1, int(6 * s)))
    stroke_line(64 * s, 152 * s, 108 * s, 200 * s, max(1, int(5 * s)))
    return rgba_to_png(px)


def rgba_to_png(px: list[list[tuple[int, int, int, int]]]) -> bytes:
    h = len(px)
    w = len(px[0])
    raw = b"".join(
        b"\x00" + b"".join(struct.pack("BBBB", *px[y][x]) for x in range(w)) for y in range(h)
    )
    def chunk(tag: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(tag + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def write_ico(path: Path, pngs: list[bytes]) -> None:
    count = len(pngs)
    offset = 6 + 16 * count
    out = bytearray(struct.pack("<HHH", 0, 1, count))
    blobs = []
    for png in pngs:
        # IHDR width/height
        w, h = struct.unpack(">II", png[16:24])
        w_b = 0 if w >= 256 else w
        h_b = 0 if h >= 256 else h
        out += struct.pack("<BBBBHHII", w_b, h_b, 0, 0, 1, 32, len(png), offset)
        blobs.append(png)
        offset += len(png)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(bytes(out) + b"".join(blobs))


def main() -> None:
    sizes = (16, 32, 48, 256)
    write_ico(OUT, [paint(n) for n in sizes])
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
