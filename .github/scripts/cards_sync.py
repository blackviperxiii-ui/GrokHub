#!/usr/bin/env python3
"""Notion Command Center cards <-> GitHub issues (stdlib only).

Modes (argv[1]):
  sync  - Ready cards for this repo's Project become issues (capped per coder),
          card -> Doing with the issue URL in Result; closed card issues -> Done.
  done  - issues/pull_request closed event: mark the linked card Done.

Env: NOTION_TOKEN, NOTION_DATA_SOURCE_ID (Notion API 2025-09-03 data source id),
GITHUB_TOKEN, GITHUB_REPOSITORY, CARDS_PROJECT (board Project option, default Loadout),
COPILOT_ASSIGN_TOKEN (optional user token; GITHUB_TOKEN may not assign Copilot),
JULES_API_KEY (optional; Jules sessions are started through the Jules API,
the 'jules' label alone does not start Jules).
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


JULES = "https://jules.googleapis.com/v1alpha"
JULES_MARK = "<!-- jules-session: "
JULES_DONE = {"COMPLETED", "FAILED", "CANCELLED", "STATE_UNSPECIFIED"}


def jules(path, method="GET", body=None):
    return req(JULES + path, method, body, {"X-Goog-Api-Key": os.environ["JULES_API_KEY"]})


def jules_active():
    source = f"sources/github/{REPO}"
    active, token = 0, ""
    for _ in range(10):
        res = jules("/sessions?pageSize=100" + (f"&pageToken={token}" if token else ""))
        for s in res.get("sessions", []):
            if (s.get("sourceContext") or {}).get("source") == source and s.get("state") not in JULES_DONE:
                active += 1
        token = res.get("nextPageToken")
        if not token:
            break
    return active


def start_jules_sessions(open_prs):
    """Start one Jules session per open 'jules' issue that has none, cap 2."""
    if not os.environ.get("JULES_API_KEY"):
        print("JULES_API_KEY is not set; Jules sessions skipped.")
        return
    if REPO.split("/")[-1].lower() == "genesis":
        return
    try:
        busy = max(jules_active(), coder_load(open_prs, {})["jules"])
    except RuntimeError as e:
        print(f"jules list failed: {e}")
        return
    issues = gh_all(f"/repos/{REPO}/issues?state=open&labels=jules&sort=created&direction=asc")
    for issue in issues:
        if "pull_request" in issue:
            continue
        comments = gh_all(f"/repos/{REPO}/issues/{issue['number']}/comments")
        if any(JULES_MARK in (c.get("body") or "") for c in comments):
            continue
        if busy >= CAP:
            print(f"jules at cap {CAP}; #{issue['number']} left queued")
            return
        body = issue.get("body") or ""
        if "## Rules" not in body:
            body += "\n\n" + RULES
        prompt = (f"Implement GitHub issue #{issue['number']} in {REPO}: {issue['title']}\n\n{body}\n\n"
                  f"Open one PR against beta whose body says Closes #{issue['number']}.")
        try:
            sess = jules("/sessions", "POST", {
                "prompt": prompt[:30000],
                "title": f"#{issue['number']} {issue['title']}"[:200],
                "sourceContext": {"source": f"sources/github/{REPO}",
                                  "githubRepoContext": {"startingBranch": "beta"}},
                "automationMode": "AUTO_CREATE_PR",
            })
        except RuntimeError as e:
            print(f"jules session failed for #{issue['number']}: {e}")
            return
        sid = sess.get("id") or sess.get("name", "").split("/")[-1]
        gh(f"/repos/{REPO}/issues/{issue['number']}/comments", "POST",
           {"body": f"Started Jules session `{sid}` (base beta).\n\n{JULES_MARK}{sid} -->"})
        print(f"#{issue['number']} -> jules session {sid}")
        busy += 1


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
    start_jules_sessions(open_prs)
    # Sweep: closed-as-completed card issues whose card is not Done yet.
    for pid, issue in closed_issues.items():
        if issue.get("state_reason") == "completed":
            mark_done(pid, issue["html_url"])
    # Sweep: open card issues whose PR already merged into beta. beta is not the
    # default branch, so "Fixes #N" never closes them, and a missed done event
    # would leave the card at Doing.
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
    if prop(page, "Status") not in ("Done", "Notes"):
        print(f"card {pid} -> Done ({url})")
        set_card(pid, "Done", f"Done: {url}")
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
            if m:
                mark_done(m.group(1).replace("-", ""), pr["html_url"],
                          issue["number"] if issue.get("state") == "open" else None)
    elif "issue" in event:
        issue = event["issue"]
        m = MARK_RE.search(issue.get("body") or "")
        if m and issue.get("state_reason") == "completed":
            mark_done(m.group(1).replace("-", ""), issue["html_url"])


if __name__ == "__main__":
    if not os.environ.get("NOTION_TOKEN"):
        print("NOTION_TOKEN is not set; Notion steps skipped.")
        if (sys.argv[1:] or ["sync"])[0] == "sync" and os.environ.get("JULES_API_KEY"):
            start_jules_sessions(gh_all(f"/repos/{REPO}/pulls?state=open"))
        sys.exit(0)
    {"sync": sync, "done": done}[sys.argv[1] if len(sys.argv) > 1 else "sync"]()
