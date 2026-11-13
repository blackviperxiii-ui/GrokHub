#!/usr/bin/env python3
"""Keep the built-in model profiles current (stdlib only).

The profiles live in crates/grokhub-core/data/model_profiles.toml and ship
with the app (see docs/model-profiles.md). This script compares them with
OpenRouter's public model list, which needs no key:

    python3 .github/scripts/model_profiles.py check            # print drift
    python3 .github/scripts/model_profiles.py write            # apply it
    python3 .github/scripts/model_profiles.py check --listing models.json

`write` updates the provider facts of every OpenRouter row (prices, window,
max output, modalities, tool and structured-output support, expiry) except
the fields a row pins, adds each new free model as a row on probation,
stamps `gone_since` on rows OpenRouter stopped listing, and rewrites the
file in its canonical form. Direct Claude and xAI rows have no public
price list, so drift against their OpenRouter mirror is only reported.
"""
import argparse
import datetime
import json
import os
import sys
import tomllib
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
PROFILES = os.path.join(ROOT, "crates", "grokhub-core", "data", "model_profiles.toml")
LISTING_URL = "https://openrouter.ai/api/v1/models"
SCHEMA = 1

# Canonical field order. Every field a row may carry is here; the Rust
# parser rejects any other, so a typo fails the build instead of vanishing.
FIELDS = [
    "key", "name", "provider", "mirror", "aliases", "tier", "intelligence",
    "roles", "caps", "lane",
    "prompt", "cached", "cache_write", "completion",
    "prompt_long", "completion_long", "long_threshold",
    "context", "max_output", "input", "output",
    "tools", "structured", "reasoning", "efforts", "default_effort", "cutoff",
    "trains", "retains", "moderated",
    "expires", "hosts", "probation", "speed_tps",
    "source", "verified", "estimate", "pin", "gone_since", "note",
]
CAP_KEYS = ["chat", "code", "tools", "reasoning", "long_context", "vision", "audio"]
LANE_KEYS = ["reasoning", "code", "long_context", "tools", "speed_floor"]
# The facts an OpenRouter listing states, which `write` keeps current.
LISTED = [
    "prompt", "cached", "cache_write", "completion", "context", "max_output",
    "input", "output", "tools", "structured", "reasoning", "expires", "moderated",
]


# ---- TOML out ---------------------------------------------------------------

def _value(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, int):
        return f"{v:_}" if abs(v) >= 1_000 else str(v)
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, list):
        return "[" + ", ".join(_value(x) for x in v) + "]"
    raise TypeError(f"no TOML form for {v!r}")


def _inline(table, order):
    parts = [f"{k} = {_value(table[k])}" for k in order if k in table]
    extra = sorted(set(table) - set(order))
    if extra:
        raise ValueError(f"unknown keys {extra}")
    return "{ " + ", ".join(parts) + " }"


def emit(doc):
    """The canonical text of a profiles document."""
    out = [
        "# Built-in model profiles: what Loadout's router knows about each cloud",
        "# model before it has measured anything. Prices are USD cents per 100M",
        "# tokens ($1 per 1M = 10_000). See docs/model-profiles.md. The weekly",
        "# updater (.github/scripts/model_profiles.py) rewrites this file in this",
        "# exact form, so keep notes in `note`, not in comments.",
        "",
        f"schema = {doc['schema']}",
        f"updated = {_value(doc['updated'])}",
    ]
    for row in doc["model"]:
        extra = sorted(set(row) - set(FIELDS))
        if extra:
            raise ValueError(f"{row.get('key')}: unknown fields {extra}")
        out += ["", "[[model]]"]
        for f in FIELDS:
            if f not in row:
                continue
            v = row[f]
            if f == "caps":
                out.append(f"caps = {_inline(v, CAP_KEYS)}")
            elif f == "lane":
                out.append(f"lane = {_inline(v, LANE_KEYS)}")
            else:
                out.append(f"{f} = {_value(v)}")
    return "\n".join(out) + "\n"


def load(path=PROFILES):
    with open(path, "rb") as fh:
        return tomllib.load(fh)


# ---- OpenRouter in ----------------------------------------------------------

def _cents(price):
    """USD per token (OpenRouter's string) to cents per 100M tokens."""
    if price in (None, ""):
        return None
    value = float(price)
    if value < 0:
        return None
    return int(round(value * 1e10))


def listing_facts(row):
    """The facts one OpenRouter /models row states, in profile field names."""
    pricing = row.get("pricing") or {}
    arch = row.get("architecture") or {}
    top = row.get("top_provider") or {}
    params = row.get("supported_parameters") or []
    facts = {
        "prompt": _cents(pricing.get("prompt")),
        "completion": _cents(pricing.get("completion")),
        "cached": _cents(pricing.get("input_cache_read")),
        "cache_write": _cents(pricing.get("input_cache_write")),
        "context": row.get("context_length") or top.get("context_length"),
        "max_output": top.get("max_completion_tokens"),
        "input": arch.get("input_modalities"),
        "output": arch.get("output_modalities"),
        "tools": ("tools" in params) if params else None,
        "structured": (("response_format" in params) or ("structured_outputs" in params))
        if params else None,
        "reasoning": ("reasoning" in params) if params else None,
        "expires": row.get("expiration_date"),
        "moderated": top.get("is_moderated"),
    }
    return {k: v for k, v in facts.items() if v is not None}


def index_listing(body):
    data = json.loads(body)
    rows = data.get("data", data) if isinstance(data, dict) else data
    return {r["id"]: r for r in rows if isinstance(r, dict) and r.get("id")}


def fetch_listing(url=LISTING_URL):
    req = urllib.request.Request(url, headers={"User-Agent": "loadout-model-profiles"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return resp.read().decode("utf-8")


# ---- drift ------------------------------------------------------------------

def _is_free(model_id):
    return model_id.endswith(":free")


def _wants(model_id, row):
    """A text model the router could use: text out, a real price."""
    out = (row.get("architecture") or {}).get("output_modalities") or ["text"]
    return "text" in out and model_id != "openrouter/auto"


def drift(doc, listing, today):
    """What changed: per-row field changes, new models, and rows gone."""
    changes, mirror_drift, gone, back = [], [], [], []
    known = set()
    for row in doc["model"]:
        if row.get("mirror"):
            known.add(row["mirror"])
        if row.get("provider") != "openrouter":
            mirror = row.get("mirror")
            if mirror and mirror in listing:
                facts = listing_facts(listing[mirror])
                for f in ("prompt", "completion", "context"):
                    if f in facts and f in row and facts[f] != row[f]:
                        mirror_drift.append((row["key"], f, row[f], facts[f]))
            continue
        key = row["key"]
        known.add(key)
        known.update(row.get("aliases", []))
        if key not in listing:
            if not row.get("gone_since"):
                gone.append(key)
            continue
        if row.get("gone_since"):
            back.append(key)
        facts = listing_facts(listing[key])
        pinned = set(row.get("pin", []))
        for f in LISTED:
            if f in facts and f not in pinned and row.get(f) != facts[f]:
                changes.append((key, f, row.get(f), facts[f]))
    new_free = sorted(
        m for m, r in listing.items() if _is_free(m) and m not in known and _wants(m, r)
    )
    new_paid = sorted(
        m for m, r in listing.items() if not _is_free(m) and m not in known and _wants(m, r)
    )
    return {
        "changes": changes,
        "mirror_drift": mirror_drift,
        "gone": gone,
        "back": back,
        "new_free": new_free,
        "new_paid": new_paid,
        "today": today,
    }


def apply(doc, listing, report):
    """Apply a drift report to the document in place. Returns it."""
    today = report["today"]
    by_key = {r["key"]: r for r in doc["model"]}
    touched = set()
    for key, f, _old, new in report["changes"]:
        by_key[key][f] = new
        touched.add(key)
    for key in report["gone"]:
        by_key[key]["gone_since"] = today
    for key in report["back"]:
        by_key[key].pop("gone_since", None)
        touched.add(key)
    for key in touched:
        by_key[key]["verified"] = today
    for model_id in report["new_free"]:
        row = {
            "key": model_id,
            "name": listing[model_id].get("name") or model_id,
            "provider": "openrouter",
            "tier": "free",
            "roles": ["probation"],
        }
        row.update(listing_facts(listing[model_id]))
        row.update({
            "hosts": 1,
            "probation": True,
            "source": "openrouter.ai/api/v1/models",
            "verified": today,
            "note": "Added by the weekly updater; on a tryout until the router has measured it.",
        })
        doc["model"].append(row)
    if any(report[k] for k in ("changes", "gone", "back", "new_free")):
        doc["updated"] = today
    return doc


def looks_partial(doc, report):
    """A listing that drops most of the OpenRouter rows at once is a bad
    fetch, not a mass retirement: `write` refuses it."""
    rows = sum(1 for r in doc["model"] if r.get("provider") == "openrouter" and not r.get("gone_since"))
    return len(report["gone"]) > max(3, rows // 2)


def render(report):
    lines = [f"Model profile drift, {report['today']}", ""]
    if report["changes"]:
        lines.append("Changed on OpenRouter:")
        lines += [f"- {k} {f}: {o} -> {n}" for k, f, o, n in report["changes"]]
    if report["new_free"]:
        lines.append("New free models (added on probation by `write`):")
        lines += [f"- {m}" for m in report["new_free"]]
    if report["gone"]:
        lines.append("No longer listed:")
        lines += [f"- {m}" for m in report["gone"]]
    if report["back"]:
        lines.append("Listed again:")
        lines += [f"- {m}" for m in report["back"]]
    if report["mirror_drift"]:
        lines.append("Direct rows that differ from their OpenRouter mirror (check by hand):")
        lines += [f"- {k} {f}: {o} vs {n}" for k, f, o, n in report["mirror_drift"]]
    if report["new_paid"]:
        lines.append(f"New paid models not profiled: {len(report['new_paid'])}")
        lines += [f"- {m}" for m in report["new_paid"][:40]]
    if len(lines) == 2:
        lines.append("No drift.")
    return "\n".join(lines) + "\n"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("command", choices=["check", "write", "format"])
    ap.add_argument("--listing", help="read OpenRouter's list from this file")
    ap.add_argument("--profiles", default=PROFILES)
    ap.add_argument("--today", default=datetime.date.today().isoformat())
    args = ap.parse_args(argv)

    doc = load(args.profiles)
    if doc.get("schema") != SCHEMA:
        print(f"unsupported schema {doc.get('schema')}", file=sys.stderr)
        return 2
    if args.command == "format":
        with open(args.profiles, "w", encoding="utf-8") as fh:
            fh.write(emit(doc))
        return 0
    if args.listing:
        with open(args.listing, encoding="utf-8") as fh:
            body = fh.read()
    else:
        body = fetch_listing()
    listing = index_listing(body)
    report = drift(doc, listing, args.today)
    text = render(report)
    sys.stdout.write(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as fh:
            fh.write(text)
    if args.command == "write":
        if looks_partial(doc, report):
            print("Refusing to write: the listing is missing most profiled models "
                  "(a partial or failed fetch).", file=sys.stderr)
            return 3
        with open(args.profiles, "w", encoding="utf-8") as fh:
            fh.write(emit(apply(doc, listing, report)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
