#!/usr/bin/env python3
"""Notion Command Center cards <-> GitHub issues (stdlib only).

Modes (argv[1]):
  sync  - Ready cards for this repo's Project become issues (capped per coder),
          card -> Doing with the issue URL in Result; closed card issues -> Done.
  done  - issues/pull_request closed event: mark the linked card Done.

Env: NOTION_TOKEN, NOTION_DATA_SOURCE_ID (Notion API 2025-09-03 data source id),
GITHUB_TOKEN, GITHUB_REPOSITORY, CARDS_PROJECT (default GrokHub),
COPILOT_ASSIGN_TOKEN (optional user token; GITHUB_TOKEN may not assign Copilot).
"""
import json
import os
import re
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone

NOTION = "https://api.notion.com/v1"
NOTION_VERSION = "2025-09-03"
GH = "https://api.github.com"
REPO = os.environ.get("GITHUB_REPOSITORY", "")
PROJECT = os.environ.get("CARDS_PROJECT") or "GrokHub"
CAP = 2
MARK_RE = re.compile(r"<!--\s*notion-card:\s*([0-9a-fA-F-]{32,36})\s*-->")
CODERS = {
    "claude": {"label": "claude", "authors": set(), "branch": "claude/"},
    "copilot": {"label": "copilot", "authors": {"Copilot", "copilot-swe-agent", "app/copilot-swe-agent"}, "branch": "copilot/"},
    "jules": {"label": "jules", "authors": {"google-labs-jules", "google-labs-jules[bot]", "app/google-labs-jules"}, "branch": ""},
}
RULES = """## Rules
- Base branch is `beta`, never `main`. Branch from `origin/beta`.
- Keep the change small and inside this card only.
- Before pushing, run locally: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace`, `bash scripts/ci-guards.sh` and `python3 scripts/pii-guard/pii_guard.py origin/beta`. Then push once.
- No force-push, no rebase. To catch up, merge `origin/beta` normally.
- No personal info (names, emails, home paths) in files, commits or the PR; use `user` and `/home/user`.
- New UI registers through beta's single `GROKHUB_DECK_NAV` handler, never a separate `deck_nav()` handler.
- One PR per issue; reference this issue with `Fixes #<n>` in the PR body.
"""


def req(url, method="GET", body=None, headers=None):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(url, data=data, method=method, headers=headers or {})
    if data is not None:
        r.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(r, timeout=30) as resp:
            raw = resp.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"{method} {url} -> {e.code}: {e.read()[:400]!r}") from None


def notion(path, method="GET", body=None):
    return req(NOTION + path, method, body, {
        "Authorization": "Bearer " + os.environ["NOTION_TOKEN"],
        "Notion-Version": NOTION_VERSION,
    })


def gh(path, method="GET", body=None, token=None):
    return req(GH + path, method, body, {
        "Authorization": "Bearer " + (token or os.environ["GITHUB_TOKEN"]),
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "cards-sync",
    })


def gh_all(path):
    out, page = [], 1
    sep = "&" if "?" in path else "?"
    while True:
        batch = gh(f"{path}{sep}per_page=100&page={page}")
        out += batch
        if len(batch) < 100 or page >= 10:
            return out
        page += 1


def text_of(rich):
    return "".join(t.get("plain_text", "") for t in rich or [])


def prop(page, name):
    p = page.get("properties", {}).get(name) or {}
    t = p.get("type")
    if t == "select":
        return (p.get("select") or {}).get("name", "")
    if t == "status":
        return (p.get("status") or {}).get("name", "")
    if t in ("title", "rich_text"):
        return text_of(p.get(t))
    if t == "number":
        return p.get("number")
    return ""


def page_body(page_id):
    lines, cursor = [], None
    while True:
        q = f"/blocks/{page_id}/children?page_size=100" + (f"&start_cursor={cursor}" if cursor else "")
        res = notion(q)
        for b in res.get("results", []):
            t = b.get("type")
            txt = text_of((b.get(t) or {}).get("rich_text"))
            prefix = {"heading_1": "# ", "heading_2": "## ", "heading_3": "### ",
                      "bulleted_list_item": "- ", "numbered_list_item": "1. ",
                      "to_do": "- [ ] ", "quote": "> "}.get(t, "")
            if t == "code":
                txt = "```\n" + txt + "\n```"
            if txt or t == "paragraph":
                lines.append(prefix + txt)
        if not res.get("has_more"):
            return "\n".join(lines).strip()
        cursor = res.get("next_cursor")


def set_card(page_id, status, result=None):
    props = {"Status": {"select": {"name": status}}}
    now = datetime.now(timezone.utc).isoformat(timespec="seconds")
    if status == "Doing":
        props["Picked up"] = {"date": {"start": now}}
    if status == "Done":
        props["Done at"] = {"date": {"start": now}}
    if result is not None:
        props["Result"] = {"rich_text": [{"type": "text", "text": {"content": result[:1900]}}]}
    notion(f"/pages/{page_id}", "PATCH", {"properties": props})


def card_issues(state):
    issues = gh_all(f"/repos/{REPO}/issues?state={state}&sort=updated&direction=desc")
    out = {}
    for i in issues:
        if "pull_request" in i:
            continue
        m = MARK_RE.search(i.get("body") or "")
        if m:
            out.setdefault(m.group(1).replace("-", ""), i)
    return out


def coder_load(open_prs, open_card_issues):
    load = {}
    for key, c in CODERS.items():
        prs = sum(1 for p in open_prs if p["user"]["login"] in c["authors"]
                  or (c["branch"] and p["head"]["ref"].startswith(c["branch"]))
                  or c["label"] in {lb["name"] for lb in p.get("labels", [])})
        iss = sum(1 for i in open_card_issues.values()
                  if c["label"] in {lb["name"] for lb in i.get("labels", [])})
        load[key] = max(prs, iss)
    return load


def query_ready():
    ds = os.environ["NOTION_DATA_SOURCE_ID"]
    body = {"filter": {"and": [
        {"property": "Status", "select": {"equals": "Ready"}},
        {"property": "Project", "select": {"equals": PROJECT}},
    ]}, "sorts": [{"property": "Order", "direction": "ascending"}], "page_size": 100}
    return notion(f"/data_sources/{ds}/query", "POST", body).get("results", [])


def assign_copilot(number):
    token = os.environ.get("COPILOT_ASSIGN_TOKEN") or None
    try:
        gh(f"/repos/{REPO}/issues/{number}/assignees", "POST", {"assignees": ["Copilot"]}, token)
        return True
    except RuntimeError as e:
        print(f"copilot assign failed for #{number}: {e}")
        return False


def dispatch_claude(number):
    try:
        gh(f"/repos/{REPO}/actions/workflows/claude.yml/dispatches", "POST",
           {"ref": "beta", "inputs": {"issue_number": str(number)}})
    except RuntimeError as e:
        print(f"claude dispatch failed for #{number}: {e}")


def sync():
    open_issues = card_issues("open")
    closed_issues = card_issues("closed")
    open_prs = gh_all(f"/repos/{REPO}/pulls?state=open")
    load = coder_load(open_prs, open_issues)
    print(f"coder load: {load}")
    for card in query_ready():
        pid = card["id"].replace("-", "")
        title = prop(card, "Name") or "Untitled card"
        coder = (prop(card, "Assigned") or "").strip().lower()
        existing = open_issues.get(pid) or closed_issues.get(pid)
        if existing:
            print(f"card {pid} already linked to #{existing['number']}; marking Doing")
            set_card(card["id"], "Doing", existing["html_url"])
            continue
        if coder not in CODERS:
            print(f"card {pid} '{title}': Assigned '{coder}' is not a GitHub coder; skipped")
            continue
        if load[coder] >= CAP:
            print(f"card {pid} '{title}': {coder} is at cap {CAP}; left queued")
            continue
        body = page_body(card["id"]) or "_No card body._"
        issue_body = (f"{body}\n\nNotion card: {card.get('url', '')}\n\n{RULES}\n"
                      f"<!-- notion-card: {pid} -->\n")
        issue = gh(f"/repos/{REPO}/issues", "POST",
                   {"title": title, "body": issue_body, "labels": [CODERS[coder]["label"]]})
        print(f"card {pid} -> #{issue['number']} ({coder})")
        load[coder] += 1
        open_issues[pid] = issue
        if coder == "copilot":
            assign_copilot(issue["number"])
        if coder == "claude":
            dispatch_claude(issue["number"])
        set_card(card["id"], "Doing", issue["html_url"])
    # Sweep: closed-as-completed card issues whose card is not Done yet.
    for pid, issue in closed_issues.items():
        if issue.get("state_reason") == "completed":
            mark_done(pid, issue["html_url"])


def mark_done(pid, url):
    page = notion(f"/pages/{pid}")
    if prop(page, "Status") in ("Done", "Notes"):
        return
    print(f"card {pid} -> Done ({url})")
    set_card(pid, "Done", f"Done: {url}")


def done():
    event = json.load(open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8"))
    if "pull_request" in event:
        pr = event["pull_request"]
        if not pr.get("merged") or pr["base"]["ref"] != "beta":
            return
        m = MARK_RE.search(pr.get("body") or "")
        if m:
            mark_done(m.group(1).replace("-", ""), pr["html_url"])
        for n in set(re.findall(r"(?i)\b(?:fix(?:e[sd])?|close[sd]?|resolve[sd]?)\s+#(\d+)", pr.get("body") or "")):
            issue = gh(f"/repos/{REPO}/issues/{n}")
            m = MARK_RE.search(issue.get("body") or "")
            if m:
                mark_done(m.group(1).replace("-", ""), pr["html_url"])
    elif "issue" in event:
        issue = event["issue"]
        m = MARK_RE.search(issue.get("body") or "")
        if m and issue.get("state_reason") == "completed":
            mark_done(m.group(1).replace("-", ""), issue["html_url"])


if __name__ == "__main__":
    if not os.environ.get("NOTION_TOKEN"):
        print("NOTION_TOKEN is not set; nothing to do.")
        sys.exit(0)
    {"sync": sync, "done": done}[sys.argv[1] if len(sys.argv) > 1 else "sync"]()
