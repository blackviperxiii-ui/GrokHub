#!/usr/bin/env python3
"""Generate or check packaging/aur/.SRCINFO from packaging/aur/PKGBUILD.

Without makepkg (CI is Ubuntu), keep the committed .SRCINFO in lockstep by
parsing the AUR PKGBUILD's scalars and arrays and emitting the same fields
makepkg --printsrcinfo would. Expand ${pkgver} / ${pkgname} in source=.

Usage:
  python3 scripts/aur-srcinfo.py              # print .SRCINFO to stdout
  python3 scripts/aur-srcinfo.py --check      # exit 1 when packaging/aur/.SRCINFO drifts
  python3 scripts/aur-srcinfo.py --write      # rewrite packaging/aur/.SRCINFO
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PKGBUILD = ROOT / "packaging" / "aur" / "PKGBUILD"
SRCINFO = ROOT / "packaging" / "aur" / ".SRCINFO"

SCALAR_KEYS = ("pkgname", "pkgver", "pkgrel", "pkgdesc", "url", "install")
ARRAY_KEYS = (
    "arch",
    "license",
    "depends",
    "optdepends",
    "makedepends",
    "options",
    "source",
    "sha256sums",
)


def _strip_quotes(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "'\"":
        return value[1:-1]
    return value


def _split_array_body(body: str) -> list[str]:
    items: list[str] = []
    token: list[str] = []
    quote: str | None = None
    i = 0
    while i < len(body):
        ch = body[i]
        if quote:
            if ch == "\\" and i + 1 < len(body):
                token.append(body[i + 1])
                i += 2
                continue
            if ch == quote:
                quote = None
            else:
                token.append(ch)
            i += 1
            continue
        if ch in "'\"":
            quote = ch
            i += 1
            continue
        if ch in " \t\n\r":
            if token:
                items.append("".join(token))
                token = []
            i += 1
            continue
        if ch == "#":
            while i < len(body) and body[i] != "\n":
                i += 1
            continue
        token.append(ch)
        i += 1
    if token:
        items.append("".join(token))
    return items


def _array_body(text: str, key: str) -> str:
    """Return the inside of `key=(...)`, respecting quotes so `)` in values stays."""
    m = re.search(rf"^{key}=\(", text, flags=re.M)
    if not m:
        raise SystemExit(f"aur-srcinfo: missing {key}=(...) in PKGBUILD")
    i = m.end()
    quote: str | None = None
    depth = 1
    start = i
    while i < len(text):
        ch = text[i]
        if quote:
            if ch == "\\" and i + 1 < len(text):
                i += 2
                continue
            if ch == quote:
                quote = None
            i += 1
            continue
        if ch in "'\"":
            quote = ch
            i += 1
            continue
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return text[start:i]
        i += 1
    raise SystemExit(f"aur-srcinfo: unclosed {key}=(...) in PKGBUILD")


def parse_pkgbuild(text: str) -> dict[str, list[str]]:
    # Drop function bodies so array/scalar parses do not see their locals.
    cleaned = re.sub(
        r"^[a-zA-Z_][a-zA-Z0-9_]*\(\)\s*\{.*?\n\}\s*$",
        "",
        text,
        flags=re.M | re.S,
    )
    fields: dict[str, list[str]] = {k: [] for k in SCALAR_KEYS + ARRAY_KEYS}

    for key in SCALAR_KEYS:
        m = re.search(rf"^{key}=([^\n#]+)", cleaned, flags=re.M)
        if not m:
            raise SystemExit(f"aur-srcinfo: missing {key}= in PKGBUILD")
        fields[key] = [_strip_quotes(m.group(1))]

    for key in ARRAY_KEYS:
        fields[key] = _split_array_body(_array_body(cleaned, key))
        if not fields[key]:
            raise SystemExit(f"aur-srcinfo: empty {key}=(...) in PKGBUILD")

    pkgver = fields["pkgver"][0]
    pkgname = fields["pkgname"][0]
    fields["source"] = [
        s.replace("${pkgver}", pkgver)
        .replace("$pkgver", pkgver)
        .replace("${pkgname}", pkgname)
        .replace("$pkgname", pkgname)
        for s in fields["source"]
    ]
    return fields


def render_srcinfo(fields: dict[str, list[str]]) -> str:
    pkgname = fields["pkgname"][0]
    lines = [
        f"pkgbase = {pkgname}",
        f"\tpkgdesc = {fields['pkgdesc'][0]}",
        f"\tpkgver = {fields['pkgver'][0]}",
        f"\tpkgrel = {fields['pkgrel'][0]}",
        f"\turl = {fields['url'][0]}",
        f"\tinstall = {fields['install'][0]}",
    ]
    for key in ("arch", "license", "makedepends", "depends", "optdepends", "options"):
        for item in fields[key]:
            lines.append(f"\t{key} = {item}")
    for item in fields["source"]:
        lines.append(f"\tsource = {item}")
    for item in fields["sha256sums"]:
        lines.append(f"\tsha256sums = {item}")
    lines.append("")
    lines.append(f"pkgname = {pkgname}")
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="fail when .SRCINFO drifts")
    mode.add_argument("--write", action="store_true", help="rewrite packaging/aur/.SRCINFO")
    args = parser.parse_args()

    fields = parse_pkgbuild(PKGBUILD.read_text(encoding="utf-8"))
    rendered = render_srcinfo(fields)

    if args.write:
        SRCINFO.write_text(rendered, encoding="utf-8")
        print(f"wrote {SRCINFO.relative_to(ROOT)}", file=sys.stderr)
        return 0

    if args.check:
        if not SRCINFO.is_file():
            print(f"aur-srcinfo: missing {SRCINFO.relative_to(ROOT)}", file=sys.stderr)
            return 1
        current = SRCINFO.read_text(encoding="utf-8")
        if current.replace("\r\n", "\n") != rendered:
            print(
                "aur-srcinfo: packaging/aur/.SRCINFO drifts from packaging/aur/PKGBUILD",
                file=sys.stderr,
            )
            print(
                "aur-srcinfo: run: python3 scripts/aur-srcinfo.py --write",
                file=sys.stderr,
            )
            return 1
        print("aur-srcinfo: packaging/aur/.SRCINFO matches PKGBUILD")
        return 0

    sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
