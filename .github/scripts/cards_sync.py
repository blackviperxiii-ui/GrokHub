#!/usr/bin/env python3
"""Notion Command Center <-> GitHub sync for Antigravity cards (stdlib only).

GitHub Actions never dispatches Claude or starts Jules sessions, and never
touches cards assigned to other coders.

Modes (argv[1]):
  sync  - Ready cards assigned to Antigravity for this repo's Project become
          'antigravity' GitHub issues (capped at CAP, leaving Notion at Ready
          for work until Antigravity starts them). Closed-as-completed card
          issues and open card issues whose PR already merged into beta are
          marked Completed in Notion.
          Optional workflow_dispatch inputs:
            START_ISSUE  - mark the linked Antigravity card(s) Working on it now.
            RESET_ISSUES - return card(s) to Ready for work and close not_planned.
  done  - issues/pull_request closed event: mark the linked card Completed.

Env: NOTION_TOKEN, NOTION_DATA_SOURCE_ID, GITHUB_TOKEN, GITHUB_REPOSITORY,
CARDS_PROJECT (board Project option, default Loadout).
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
PROJECT = os.environ.get("CARDS_PROJECT") or "Loadout"
CAP = 2
MARK_RE = re.compile(r"<!--\s*notion-card:\s*([0-9a-fA-F-]{32,36})\s*-->")
# GitHub's closing keywords, with or without a colon ("Fixes #12", "Fixes: #12").
CLOSES_RE = re.compile(r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s*:?\s+#(\d+)\b", re.I)
STATUS_READY = "Ready for work"
STATUS_DOING = "Working on it now"
STATUS_DONE = "Completed"
DONE_STATUSES = (STATUS_DONE, "Done", "Notes")
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
    if status == STATUS_DOING:
        props["Picked up"] = {"date": {"start": now}}
    if status in (STATUS_DONE, "Done"):
        props["Done at"] = {"date": {"start": now}}
    if result is not None:
        props["Result"] = {"rich_text": [{"type": "text", "text": {"content": result[:1900]}}]}
    notion(f"/pages/{page_id}", "PATCH", {"properties": props})


def trusted(issue):
    """Anyone can open an issue and paste a card marker into it, so a marker
    only counts on an issue the sync opened itself (the workflow's own login;
    its issues report no association) or one from the project's own people."""
    return ((issue.get("user") or {}).get("login") == "github-actions[bot]"
            or issue.get("author_association") in ("OWNER", "MEMBER", "COLLABORATOR"))


def card_issues(state):
    issues = gh_all(f"/repos/{REPO}/issues?state={state}&sort=updated&direction=desc")
    out = {}
    for i in issues:
        if "pull_request" in i or not trusted(i):
            continue
        m = MARK_RE.search(i.get("body") or "")
        if m:
            out.setdefault(m.group(1).replace("-", ""), i)
    return out


def antigravity_load(open_prs, open_card_issues):
    prs = sum(
        1 for p in open_prs
        if (p.get("head") or {}).get("ref", "").startswith("antigravity/")
        or "antigravity" in {lb["name"].lower() for lb in p.get("labels", [])}
    )
    iss = sum(
        1 for i in open_card_issues.values()
        if "antigravity" in {lb["name"].lower() for lb in i.get("labels", [])}
    )
    return max(prs, iss)


def query_ready_antigravity():
    ds = os.environ.get("NOTION_DATA_SOURCE_ID", "").strip()
    if not ds:
        return []
    body = {
        "filter": {"and": [
            {"property": "Status", "select": {"equals": STATUS_READY}},
            {"property": "Project", "select": {"equals": PROJECT}},
            {"property": "Assigned", "select": {"equals": "Antigravity"}},
        ]},
        "sorts": [{"property": "Order", "direction": "ascending"}],
        "page_size": 100,
    }
    return notion(f"/data_sources/{ds}/query", "POST", body).get("results", [])


def sync_ready_antigravity(open_issues, closed_issues):
    """Create 'antigravity' issues for Ready cards assigned to Antigravity (never touching other coders)."""
    if not os.environ.get("NOTION_DATA_SOURCE_ID", "").strip():
        return
    open_prs = gh_all(f"/repos/{REPO}/pulls?state=open")
    load = antigravity_load(open_prs, open_issues)
    print(f"antigravity load: {load}/{CAP}")
    for card in query_ready_antigravity():
        pid = card["id"].replace("-", "")
        title = prop(card, "Name") or "Untitled card"
        assigned = (prop(card, "Assigned") or "").strip().lower()
        if assigned != "antigravity":
            print(f"card {pid} '{title}': Assigned '{assigned}' is not Antigravity; skipped")
            continue
        if card.get("archived") or card.get("in_trash"):
            print(f"card {pid} is archived; skipped")
            continue
        if pid in open_issues:
            continue
        closed = closed_issues.get(pid)
        if closed and closed.get("state_reason") == "completed":
            continue
        if load >= CAP:
            print(f"card {pid} '{title}': antigravity is at cap {CAP}; left queued")
            break
        body = page_body(card["id"]) or "_No card body._"
        issue_body = (f"{body}\n\nNotion card: {card.get('url', '')}\n\n{RULES}\n"
                      f"<!-- notion-card: {pid} -->\n")
        issue = gh(f"/repos/{REPO}/issues", "POST",
                   {"title": title, "body": issue_body, "labels": ["antigravity"]})
        print(f"card {pid} -> #{issue['number']} (antigravity)")
        load += 1
        open_issues[pid] = issue


def start_antigravity(raw):
    """Mark picked-up antigravity card issues as Working on it now in Notion."""
    for token in re.split(r"[\s,]+", raw.strip()):
        if not token.isdigit():
            continue
        number = int(token)
        issue = gh(f"/repos/{REPO}/issues/{number}")
        if "pull_request" in issue or not trusted(issue):
            print(f"#{number}: untrusted or PR; skipped")
            continue
        labels = {lb["name"].lower() for lb in issue.get("labels", [])}
        if "antigravity" not in labels:
            print(f"#{number}: missing 'antigravity' label ({sorted(labels)}); refused")
            continue
        m = MARK_RE.search(issue.get("body") or "")
        if not m:
            print(f"#{number}: no notion-card marker; skipped")
            continue
        pid = m.group(1).replace("-", "")
        page = notion(f"/pages/{pid}")
        if page.get("archived") or page.get("in_trash"):
            print(f"card {pid} is archived; skipped")
            continue
        assigned = (prop(page, "Assigned") or "").strip().lower()
        if assigned and assigned != "antigravity":
            print(f"card {pid} (#{number}): Assigned is '{assigned}', not Antigravity; refused")
            continue
        status = prop(page, "Status")
        if status in DONE_STATUSES:
            print(f"card {pid} (#{number}) already {status}; skipped")
            continue
        print(f"card {pid} (#{number}) -> {STATUS_DOING}")
        set_card(pid, STATUS_DOING, issue["html_url"])


def reset_ready(raw):
    """Return unstarted card issues' Notion cards to Ready for work and close them not_planned."""
    for token in re.split(r"[\s,]+", raw.strip()):
        if not token.isdigit():
            continue
        number = int(token)
        issue = gh(f"/repos/{REPO}/issues/{number}")
        m = MARK_RE.search(issue.get("body") or "")
        if not m or not trusted(issue):
            continue
        pid = m.group(1).replace("-", "")
        page = notion(f"/pages/{pid}")
        if page.get("archived") or page.get("in_trash"):
            print(f"card {pid} is archived; skipped")
        elif prop(page, "Status") not in DONE_STATUSES:
            print(f"card {pid} (#{number}) -> {STATUS_READY}")
            notion(f"/pages/{pid}", "PATCH", {"properties": {
                "Status": {"select": {"name": STATUS_READY}},
                "Picked up": {"date": None},
                "Result": {"rich_text": []},
            }})
        if issue.get("state") == "open":
            gh(f"/repos/{REPO}/issues/{number}", "PATCH",
               {"state": "closed", "state_reason": "not_planned"})


def sync():
    """Sync Ready+Antigravity cards, sweep completions, or handle explicit start/reset."""
    reset_raw = os.environ.get("RESET_ISSUES", "").strip()
    if reset_raw:
        reset_ready(reset_raw)
        return
    start_raw = os.environ.get("START_ISSUE", "").strip()
    if start_raw:
        start_antigravity(start_raw)
        return
    open_issues = card_issues("open")
    closed_issues = card_issues("closed")
    sync_ready_antigravity(open_issues, closed_issues)
    for pid, issue in closed_issues.items():
        if issue.get("state_reason") == "completed":
            mark_done(pid, issue["html_url"])
    # Sweep: open card issues whose PR already merged into beta. beta is not the
    # default branch, so "Fixes #N" never closes them, and a missed done event
    # would leave the card open and not Completed.
    for pid, issue in open_issues.items():
        if "number" not in issue:
            continue
        pr_url = merged_beta_pr(issue["number"])
        if pr_url:
            mark_done(pid, pr_url, issue["number"])


def merged_beta_pr(number):
    """URL of a same-repo PR merged into beta whose body closes issue #number."""
    for ev in gh_all(f"/repos/{REPO}/issues/{number}/timeline"):
        src = (ev.get("source") or {}).get("issue") or {}
        if ev.get("event") != "cross-referenced" or not (src.get("pull_request") or {}).get("merged_at"):
            continue
        if (src.get("repository") or {}).get("full_name") != REPO:
            continue
        pr = gh(f"/repos/{REPO}/pulls/{src['number']}")
        # A bare mention ("see #12") is not a fix; only a closing keyword counts.
        if str(number) not in CLOSES_RE.findall(pr.get("body") or ""):
            continue
        if pr.get("merged") and pr["base"]["ref"] == "beta":
            return pr["html_url"]
    return None


def close_issue(number):
    try:
        gh(f"/repos/{REPO}/issues/{number}", "PATCH", {"state": "closed", "state_reason": "completed"})
    except RuntimeError as e:
        print(f"close #{number} failed: {e}")


def mark_done(pid, url, issue_number=None):
    page = notion(f"/pages/{pid}")
    if page.get("archived") or page.get("in_trash"):
        print(f"card {pid} is archived; skipped")
    elif prop(page, "Status") not in DONE_STATUSES:
        print(f"card {pid} -> {STATUS_DONE} ({url})")
        try:
            set_card(pid, STATUS_DONE, f"Done: {url}")
        except RuntimeError as e:
            if "archived" in str(e).lower():
                print(f"card {pid} is archived; skipped")
            else:
                raise
    if issue_number:
        close_issue(issue_number)


def done():
    with open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8") as f:
        event = json.load(f)
    if "pull_request" in event:
        pr = event["pull_request"]
        if not pr.get("merged") or pr["base"]["ref"] != "beta":
            return
        m = MARK_RE.search(pr.get("body") or "")
        if m:
            mark_done(m.group(1).replace("-", ""), pr["html_url"])
        for n in set(CLOSES_RE.findall(pr.get("body") or "")):
            issue = gh(f"/repos/{REPO}/issues/{n}")
            m = MARK_RE.search(issue.get("body") or "")
            if m and trusted(issue):
                mark_done(m.group(1).replace("-", ""), pr["html_url"],
                          issue["number"] if issue.get("state") == "open" else None)
    elif "issue" in event:
        issue = event["issue"]
        m = MARK_RE.search(issue.get("body") or "")
        if m and trusted(issue) and issue.get("state_reason") == "completed":
            mark_done(m.group(1).replace("-", ""), issue["html_url"])


if __name__ == "__main__":
    if not os.environ.get("NOTION_TOKEN"):
        print("NOTION_TOKEN is not set; Notion steps skipped.")
        sys.exit(0)
    {"sync": sync, "done": done}[sys.argv[1] if len(sys.argv) > 1 else "sync"]()
