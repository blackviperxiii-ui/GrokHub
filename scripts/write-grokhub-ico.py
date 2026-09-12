"""Pack Linux hicolor PNGs into packaging/windows/grokhub.ico."""
from __future__ import annotations

import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "packaging" / "windows" / "grokhub.ico"
HICOLOR = ROOT / "packaging" / "icons" / "hicolor"
SIZES = (16, 32, 48, 256)
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


def png_for(size: int) -> bytes:
    path = HICOLOR / f"{size}x{size}" / "apps" / "grokhub.png"
    data = path.read_bytes()
    if data[:8] != PNG_MAGIC:
        raise SystemExit(f"{path} is not a PNG")
    w, h = struct.unpack(">II", data[16:24])
    if (w, h) != (size, size):
        raise SystemExit(f"{path} is {w}x{h}, expected {size}x{size}")
    return data


def write_ico(path: Path, pngs: list[bytes]) -> None:
    count = len(pngs)
    offset = 6 + 16 * count
    out = bytearray(struct.pack("<HHH", 0, 1, count))
    blobs = []
    for png in pngs:
        w, h = struct.unpack(">II", png[16:24])
        w_b = 0 if w >= 256 else w
        h_b = 0 if h >= 256 else h
        out += struct.pack("<BBBBHHII", w_b, h_b, 0, 0, 1, 32, len(png), offset)
        blobs.append(png)
        offset += len(png)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(bytes(out) + b"".join(blobs))


def main() -> None:
    write_ico(OUT, [png_for(n) for n in SIZES])
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
