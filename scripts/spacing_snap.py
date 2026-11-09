#!/usr/bin/env python3
"""Snap spacing literals onto Loadout's one spacing scale.

Scale (px): 0, 1 (hairline), 2, 4, 8, 12, 16, 24, 32, 40, 48, 64, 80, 96.
Off-scale values go to the nearest step; ties round up (6 -> 8, 10 -> 12).
See docs/design/spacing.md.

Usage:
  scripts/spacing_snap.py rust <files...>   # add_space / Margin / item_spacing literals
  scripts/spacing_snap.py css  <files...>   # padding / margin / gap px values
  add --check to report without writing (exit 1 if anything is off-scale).
"""
import re
import sys

SCALE = [0, 1, 2, 4, 8, 12, 16, 24, 32, 40, 48, 64, 80, 96]


def snap(v: float) -> float:
    a = abs(v)
    if a in SCALE or a > SCALE[-1]:
        return v
    best = min(SCALE[2:], key=lambda s: (abs(s - a), -s))
    return best if v >= 0 else -best


def fmt_float(orig: str, v: float) -> str:
    if "." in orig or orig.endswith("f32"):
        return f"{v:.1f}"
    return str(int(v))


NUM = r"-?\d+(?:\.\d+)?"
RUST_PATTERNS = [
    re.compile(rf"(add_space\(\s*)({NUM})(\s*\))"),
    re.compile(rf"(Margin::same\(\s*)({NUM})(\s*\))"),
    re.compile(rf"(item_spacing\.[xy]\s*=\s*)({NUM})(\s*;)"),
]
RUST_PAIRS = [
    re.compile(rf"(Margin::symmetric\(\s*)({NUM})(\s*,\s*)({NUM})(\s*\))"),
    re.compile(rf"(item_spacing\s*=\s*egui::vec2\(\s*)({NUM})(\s*,\s*)({NUM})(\s*\))"),
]


def fix_rust(text: str, report):
    def one(m):
        new = snap(float(m.group(2)))
        if new != float(m.group(2)):
            report(m.group(0))
        return m.group(1) + fmt_float(m.group(2), new) + m.group(3)

    def two(m):
        a, b = float(m.group(2)), float(m.group(4))
        na, nb = snap(a), snap(b)
        if (na, nb) != (a, b):
            report(m.group(0))
        return (m.group(1) + fmt_float(m.group(2), na) + m.group(3)
                + fmt_float(m.group(4), nb) + m.group(5))

    for p in RUST_PATTERNS:
        text = p.sub(one, text)
    for p in RUST_PAIRS:
        text = p.sub(two, text)
    return text


CSS_DECL = re.compile(r"((?:padding|margin|gap|row-gap|column-gap)(?:-[a-z]+)*\s*:\s*)([^;{}]+)(;?)")
CSS_PX = re.compile(r"(-?\d+(?:\.\d+)?)px")


def fix_css(text: str, report):
    def decl(m):
        def px(n):
            v = float(n.group(1))
            new = snap(v)
            if new != v:
                report(m.group(0).strip())
            return f"{int(new) if new == int(new) else new}px"
        return m.group(1) + CSS_PX.sub(px, m.group(2)) + m.group(3)
    return CSS_DECL.sub(decl, text)


def main(argv):
    check = "--check" in argv
    argv = [a for a in argv if a != "--check"]
    mode, files = argv[0], argv[1:]
    off = 0
    for path in files:
        with open(path, encoding="utf-8", newline="") as f:
            src = f.read()
        hits = []
        out = (fix_rust if mode == "rust" else fix_css)(src, hits.append)
        off += len(hits)
        for h in hits:
            print(f"{path}: {h}")
        if not check and out != src:
            with open(path, "w", encoding="utf-8", newline="") as f:
                f.write(out)
    print(f"{off} off-scale value(s)", file=sys.stderr)
    return 1 if (check and off) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
