#!/usr/bin/env python3
"""Snap text sizes onto Loadout's one type scale (docs/design/type.md).

Scale (px): 12 caption, 14 body, 16 prose/title, 20 section, 24 headline,
32 display. 10 is the badge step, reached only through `TYPE_BADGE`.
Off-scale sizes go to the nearest step; ties round up (13 -> 14, 18 -> 20).

Usage:
  scripts/type_snap.py rust <files...>  # literal .size(N) / FontId sizes -> theme::TYPE_*
  scripts/type_snap.py css  <files...>  # font-size / line-height / font-weight -> tokens
  add --check to report without writing (exit 1 if anything is off-scale).
"""
import re
import sys

STEPS = [12, 14, 16, 20, 24, 32]
RUST_CONST = {12: "TYPE_CAPTION", 14: "TYPE_BODY", 16: "TYPE_TITLE",
              20: "TYPE_SECTION", 24: "TYPE_HEADLINE", 32: "TYPE_DISPLAY"}
CSS_TOKEN = {12: "caption", 14: "body", 16: "prose", 20: "section",
             24: "headline", 32: "display"}


def snap(px: float) -> int:
    if px < 10:  # unread-counter digits; see TYPE_BADGE
        return 10
    return min(STEPS, key=lambda s: (abs(s - px), -s))


NUM = r"\d+(?:\.\d+)?"
RUST_PATTERNS = [
    re.compile(rf"(\.size\(\s*)({NUM})(?:_?f32)?(\s*\))"),
    re.compile(rf"(FontId::(?:proportional|monospace)\(\s*)({NUM})(?:_?f32)?(\s*\))"),
    re.compile(rf"(FontId::new\(\s*)({NUM})(?:_?f32)?(\s*,)"),
]


def rust_const(px: int) -> str:
    return "crate::theme::" + ("TYPE_BADGE" if px == 10 else RUST_CONST[px])


def fix_rust(text: str, report):
    def one(m):
        report(m.group(0))
        return m.group(1) + rust_const(snap(float(m.group(2)))) + m.group(3)

    for p in RUST_PATTERNS:
        text = p.sub(one, text)
    return text


def css_px(value: str):
    """px size of a font-size value, taking em/rem against a 16px base."""
    m = re.fullmatch(rf"({NUM})(px|rem|em)", value.strip())
    if not m:
        return None
    v = float(m.group(1))
    return v if m.group(2) == "px" else v * 16


WEIGHT = {"normal": "400", "bold": "600", "700": "600", "800": "600"}


def fix_css(text: str, report):
    def size(m):
        px = css_px(m.group(2))
        if px is None:
            return m.group(0)
        report(m.group(0))
        tok = CSS_TOKEN.get(snap(px), "caption")
        return f"{m.group(1)}var(--font-{tok})"

    text = re.sub(r"(font-size:\s*)([^;}\n]+)", size, text)

    def weight(m):
        new = WEIGHT.get(m.group(2).strip())
        if new is None:
            return m.group(0)
        report(m.group(0))
        return m.group(1) + new

    return re.sub(r"(font-weight:\s*)([^;}\n]+)", weight, text)


def main(argv):
    if len(argv) < 2 or argv[0] not in ("rust", "css"):
        print(__doc__)
        return 2
    mode, check = argv[0], "--check" in argv
    files = [a for a in argv[1:] if a != "--check"]
    fixer = fix_rust if mode == "rust" else fix_css
    hits = 0
    for f in files:
        text = open(f, encoding="utf-8").read()
        found = []
        new = fixer(text, found.append)
        hits += len(found)
        for h in found:
            print(f"{f}: {h}")
        if not check and new != text:
            open(f, "w", encoding="utf-8", newline="").write(new)
    return 1 if check and hits else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
