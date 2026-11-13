#!/usr/bin/env python3
"""Closeout gate for follow-through: no "done" while a PR the run opened is open.

    python3 closeout.py owner/repo#12 https://github.com/owner/repo/pull/13 \
        --pending "owner/repo#13=waiting on the release review"

Each PR must be MERGED, or be named with --pending and a blocker. Exit 0 when
the run may close, 1 when it may not (the lines say which PR and why), 2 when
a PR can't be read. State comes from `gh` when it is on PATH, else the GitHub
REST API with GH_TOKEN or GITHUB_TOKEN.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
from typing import Callable

REF = re.compile(r"^(?:https://github\.com/)?([\w.-]+)/([\w.-]+)(?:#|/pull/)(\d+)/?$")


def parse_ref(text: str) -> tuple[str, int]:
    """`owner/repo#N` or a PR URL -> ("owner/repo", N)."""
    m = REF.match(text.strip())
    if not m:
        raise ValueError(f"not a PR: {text!r} (use owner/repo#N or its URL)")
    return f"{m.group(1)}/{m.group(2)}", int(m.group(3))


def key(repo: str, number: int) -> str:
    # GitHub names are case-insensitive: GrokHub#1 and grokhub#1 are one PR.
    return f"{repo.lower()}#{number}"


def pr_state(pr: object) -> str:
    """REST pull object -> MERGED, OPEN or CLOSED."""
    if not isinstance(pr, dict):
        raise ValueError(f"reply is not a PR object: {type(pr).__name__}")
    if pr.get("merged"):
        return "MERGED"
    state = str(pr.get("state", "")).upper()
    if state not in ("OPEN", "CLOSED"):
        raise ValueError(f"no PR state in the reply: {state!r}")
    return state


def state_via_gh(repo: str, number: int) -> str:
    # REST, not `gh pr view`: some hosts allow the REST API but block GraphQL.
    p = subprocess.run(
        ["gh", "api", "--hostname", "github.com", f"repos/{repo}/pulls/{number}"],
        capture_output=True, text=True, timeout=60,
    )
    if p.returncode:
        lines = (p.stderr or p.stdout).strip().splitlines()
        raise RuntimeError(f"gh api: {lines[-1] if lines else f'exit {p.returncode}'}")
    return pr_state(json.loads(p.stdout))


def state_via_api(repo: str, number: int) -> str:
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "vstack-closeout"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(f"https://api.github.com/repos/{repo}/pulls/{number}", headers=headers)
    with urllib.request.urlopen(req, timeout=30) as resp:
        return pr_state(json.load(resp))


def default_lookup() -> Callable[[str, int], str]:
    return state_via_gh if shutil.which("gh") else state_via_api


def verdict(
    prs: list[tuple[str, int]],
    pending: dict[str, str],
    lookup: Callable[[str, int], str],
) -> tuple[int, list[str]]:
    """(exit code, one line per PR). Merged passes; anything else needs a named blocker."""
    lines, code = [], 0
    seen = set()
    for repo, number in prs:
        if key(repo, number) in seen:
            continue
        seen.add(key(repo, number))
        name = f"{repo}#{number}"
        try:
            state = lookup(repo, number)
        except Exception as exc:  # any failure to read means the run can't close
            why = f"HTTP {exc.code}" if isinstance(exc, urllib.error.HTTPError) else str(exc) or type(exc).__name__
            lines.append(f"{name} UNREADABLE ({why}): can't close until it is checked")
            code = max(code, 2)
            continue
        blocker = pending.get(key(repo, number), "").strip()
        if state == "MERGED":
            lines.append(f"{name} MERGED")
        elif blocker:
            lines.append(f"{name} {state} pending: {blocker}")
        else:
            lines.append(f"{name} {state}: not merged and no blocker named (--pending \"{name}=why\")")
            code = max(code, 1)
    return code, lines


def main(argv: list[str] | None = None, lookup: Callable[[str, int], str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("prs", nargs="+", help="every PR the run opened: owner/repo#N or its URL")
    ap.add_argument("--pending", action="append", default=[], metavar="PR=BLOCKER",
                    help="a PR left open on purpose, with what blocks it")
    args = ap.parse_intermixed_args(argv)
    try:
        prs = [parse_ref(p) for p in args.prs]
        listed = {key(*pr) for pr in prs}
        pending = {}
        for item in args.pending:
            ref, sep, why = item.partition("=")
            if not sep:
                raise ValueError(f"--pending {item!r} has no blocker (use PR=BLOCKER)")
            name = key(*parse_ref(ref))
            if name not in listed:
                raise ValueError(f"--pending names {ref.strip()}, which is not in the PR list")
            pending[name] = why
    except ValueError as exc:
        ap.error(str(exc))
    code, lines = verdict(prs, pending, lookup or default_lookup())
    print("\n".join(lines))
    print("closeout: OK, the run may close" if code == 0 else "closeout: NOT DONE")
    return code


if __name__ == "__main__":
    sys.exit(main())
