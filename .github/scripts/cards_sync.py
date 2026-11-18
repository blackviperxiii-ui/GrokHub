#!/usr/bin/env python3
"""Notion Command Center <-> GitHub completion sync (stdlib only).

GitHub no longer picks up Ready cards, opens card issues, dispatches Claude,
or starts Jules sessions. Only completion sync runs:

Modes (argv[1]):
  sync  - Scheduled sweep: closed-as-completed card issues and open card issues
          whose PR already merged into beta are marked Completed in Notion.
  done  - issues/pull_request closed event: mark the linked card Completed.

Env: NOTION_TOKEN, GITHUB_TOKEN, GITHUB_REPOSITORY.
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
MARK_RE = re.compile(r"<!--\s*notion-card:\s*([0-9a-fA-F-]{32,36})\s*-->")
# GitHub's closing keywords, with or without a colon ("Fixes #12", "Fixes: #12").
CLOSES_RE = re.compile(r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s*:?\s+#(\d+)\b", re.I)
STATUS_DOING = "Working on it now"
STATUS_DONE = "Completed"
DONE_STATUSES = (STATUS_DONE, "Done", "Notes")


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


def set_card(page_id, status, result=None):
    props = {"Status": {"select": {"name": status}}}
    now = datetime.now(timezone.utc).isoformat(timespec="seconds")
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


def sync():
    """Completion sweep only: never pick up Ready cards or dispatch agents."""
    open_issues = card_issues("open")
    closed_issues = card_issues("closed")
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
    event = json.load(open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8"))
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
