#!/usr/bin/env python3
"""Fill in the derived profile fields of the on-device catalog (stdlib only).

Each chat row of crates/grokhub-core/data/local_catalog.toml states its
parameter counts (`params_b`, `active_b`) and trained context (`ctx_max`) by
hand. This script derives the rest, so a new row needs only those three:

    python3 .github/scripts/local_profiles.py write   # fill the fields in
    python3 .github/scripts/local_profiles.py check   # CI: fail if stale

Derived fields, written right after the row's `tok_s` line:

- `tok_s_tier`: generation speed estimated for each hardware tier's reference
  GPU (autotune/device.rs). Decoding reads the active weights once per token,
  so tok/s ~ EFFICIENCY x bandwidth / (file size x active/total params, plus
  MOE_OVERHEAD for a mixture of experts). A
  tier gets a figure only when the model fits in that tier's VRAM; otherwise
  part of it runs on the CPU and the picker's slow path applies.
- `gaming_safe`: small enough to run beside a game (at most GAMING_MB at the
  default context) and good enough to be worth it (quality at least
  GAMING_QUALITY), the gaming swap's pick.

`estimate` names every profile field nobody has measured; list a field in
`measured` once it has been, and it leaves `estimate`. The speed test still
replaces any tok/s figure at run time.
"""
import argparse
import os
import re
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
CATALOG = os.path.join(ROOT, "crates", "grokhub-core", "data", "local_catalog.toml")

# Reference GPU per tier: (VRAM MiB, memory bandwidth GB/s).
# T1 RTX 4050 laptop 6 GB, T2 RTX 4060 Ti 8 GB, T3 RX 9070 XT 16 GB, T4 RTX 4090 24 GB.
TIERS = {"T1": (6_144, 192), "T2": (8_192, 300), "T3": (16_384, 640), "T4": (24_576, 1_000)}
# Share of peak bandwidth llama.cpp decoding reaches on a Q4 model.
EFFICIENCY = 0.55
# Room the card keeps for the desktop and driver.
VRAM_HEADROOM = 0.92
GAMING_MB = 4_608
GAMING_QUALITY = 38
# A mixture-of-experts model also reads its attention, embeddings and router
# every token: this share of the file on top of the active experts.
MOE_OVERHEAD = 0.05
# Fields typed in by hand from the base model's card; estimates until measured.
HAND = ("params_b", "active_b", "ctx_max")


def tier_speeds(row):
    total, active = row.get("params_b"), row.get("active_b")
    if not total or not active:
        return {}
    share = active / total + (MOE_OVERHEAD if active < total else 0)
    gb_per_token = row["download_mb"] / 1_024 * min(1.0, share)
    out = {}
    for tier, (vram, bandwidth) in TIERS.items():
        if row["run_mb"] <= vram * VRAM_HEADROOM:
            out[tier] = max(1, round(EFFICIENCY * bandwidth / gb_per_token))
    return out


def gaming_safe(row):
    quality = (row.get("skills") or {}).get("quality", 0)
    return row["run_mb"] <= GAMING_MB and quality >= GAMING_QUALITY


def derived_lines(row):
    lines = []
    speeds = tier_speeds(row)
    if speeds:
        inner = ", ".join(f"{t} = {v}" for t, v in speeds.items())
        lines.append(f"tok_s_tier = {{ {inner} }}")
    lines.append(f"gaming_safe = {'true' if gaming_safe(row) else 'false'}")
    estimate = list(row.get("estimate", []))
    measured = row.get("measured", [])
    for field in (*HAND, "tok_s_tier") if speeds else HAND:
        if field not in estimate and field not in measured:
            estimate.append(field)
    order = (*HAND, "tok_s_tier")
    estimate.sort(key=lambda f: order.index(f) if f in order else len(order))
    if estimate:
        lines.append("estimate = [" + ", ".join(f'"{e}"' for e in estimate) + "]")
    return lines


def rewrite(text):
    """The catalog with every chat row's derived lines brought up to date."""
    rows = {r["id"]: r for r in tomllib.loads(text)["model"]}
    out, current = [], None
    for line in text.split("\n"):
        if line == "[[model]]":
            current = None
        m = re.match(r'^id = "(.+)"$', line)
        if m:
            current = rows[m.group(1)]
        if current is not None and current.get("kind", "chat") == "chat":
            if re.match(r"^(tok_s_tier|gaming_safe|estimate) = ", line):
                continue
            out.append(line)
            if line.startswith("tok_s = "):
                out.extend(derived_lines(current))
            continue
        out.append(line)
    return "\n".join(out)


def missing(text):
    """Chat rows that lack a hand-entered profile field."""
    return [
        r["id"]
        for r in tomllib.loads(text)["model"]
        if r.get("kind", "chat") == "chat"
        and any(k not in r for k in ("params_b", "active_b", "ctx_max"))
    ]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("command", choices=["check", "write"])
    ap.add_argument("--catalog", default=CATALOG)
    args = ap.parse_args(argv)
    with open(args.catalog, encoding="utf-8") as fh:
        text = fh.read()
    gaps = missing(text)
    if gaps:
        print("Chat rows without params_b, active_b and ctx_max:", *gaps, sep="\n- ", file=sys.stderr)
        return 1
    new = rewrite(text)
    if args.command == "write":
        if new != text:
            with open(args.catalog, "w", encoding="utf-8") as fh:
                fh.write(new)
        return 0
    if new != text:
        print("local_catalog.toml profile fields are stale: run "
              ".github/scripts/local_profiles.py write", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
