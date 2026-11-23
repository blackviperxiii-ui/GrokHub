#!/usr/bin/env python3
"""Tests for the cards sync (stdlib only): who may move a card.

No network: cards_sync.req is replaced by a recorder that answers from a
table of canned replies.

    python3 .github/scripts/test_cards_sync.py
"""
import json
import os
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cards_sync  # noqa: E402

REPO = "user/my-app"
CARD = "0123456789abcdef0123456789abcdef"
MARK = f"<!-- notion-card: {CARD} -->"
PAGE = f"{cards_sync.NOTION}/pages/{CARD}"
ISSUES = f"{cards_sync.GH}/repos/{REPO}/issues"
DOING = {"properties": {"Status": {"type": "select", "select": {"name": cards_sync.STATUS_DOING}}}}


def issue(login, association, number=7, state="closed", reason="completed", body=MARK):
    return {
        "number": number,
        "state": state,
        "state_reason": reason,
        "body": body,
        "html_url": f"https://github.com/{REPO}/issues/{number}",
        "user": {"login": login},
        "author_association": association,
        "labels": [],
    }


BOT = ("github-actions[bot]", "NONE")
OUTSIDER = ("someone-else", "NONE")


class Recorder:
    """Stands in for cards_sync.req: records every call, answers from `replies`."""

    def __init__(self, replies):
        self.replies = replies
        self.calls = []

    def __call__(self, url, method="GET", body=None, headers=None):
        self.calls.append((method, url, body))
        for (want_method, prefix), reply in self.replies.items():
            if method == want_method and url.startswith(prefix):
                return reply
        return [] if method == "GET" else {}

    def writes(self):
        return [c for c in self.calls if c[0] != "GET" and "/query" not in c[1]]


class CardsSyncTest(unittest.TestCase):
    def setUp(self):
        env = {"NOTION_TOKEN": "fake-token", "GITHUB_TOKEN": "fake-token",
               "NOTION_DATA_SOURCE_ID": "ds1"}
        patcher = mock.patch.dict(os.environ, env)
        patcher.start()
        self.addCleanup(patcher.stop)
        os.environ.pop("JULES_API_KEY", None)
        repo = mock.patch.object(cards_sync, "REPO", REPO)
        repo.start()
        self.addCleanup(repo.stop)

    def run_with(self, replies, fn):
        rec = Recorder(replies)
        with mock.patch.object(cards_sync, "req", rec), mock.patch("builtins.print"):
            fn()
        return rec

    def run_done(self, event, replies):
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
            json.dump(event, f)
        self.addCleanup(os.unlink, f.name)
        with mock.patch.dict(os.environ, {"GITHUB_EVENT_PATH": f.name}):
            return self.run_with(replies, cards_sync.done)

    def assert_one_done_patch(self, rec, result):
        self.assertEqual(len(rec.writes()), 1, rec.writes())
        method, url, body = rec.writes()[0]
        self.assertEqual((method, url), ("PATCH", PAGE))
        self.assertEqual(body["properties"]["Status"], {"select": {"name": cards_sync.STATUS_DONE}})
        self.assertEqual(body["properties"]["Result"]["rich_text"][0]["text"]["content"], result)

    # --- a closed issue event -------------------------------------------------

    def test_closing_the_projects_own_card_issue_marks_the_card_done(self):
        rec = self.run_done({"issue": issue(*BOT)}, {("GET", PAGE): DOING})
        self.assert_one_done_patch(rec, f"Done: https://github.com/{REPO}/issues/7")

    def test_closing_an_outsiders_issue_with_a_card_marker_changes_nothing(self):
        rec = self.run_done({"issue": issue(*OUTSIDER)}, {("GET", PAGE): DOING})
        self.assertEqual(rec.calls, [])

    def test_an_owner_member_or_collaborator_issue_counts(self):
        for association in ("OWNER", "MEMBER", "COLLABORATOR"):
            with self.subTest(association=association):
                rec = self.run_done({"issue": issue("user", association)}, {("GET", PAGE): DOING})
                self.assert_one_done_patch(rec, f"Done: https://github.com/{REPO}/issues/7")

    def test_a_contributor_or_first_timer_issue_does_not_count(self):
        for association in ("CONTRIBUTOR", "FIRST_TIME_CONTRIBUTOR", "FIRST_TIMER", "NONE", None):
            with self.subTest(association=association):
                rec = self.run_done({"issue": issue("someone-else", association)}, {("GET", PAGE): DOING})
                self.assertEqual(rec.calls, [])

    # --- a merged pull request that closes an issue ---------------------------

    def merged_pr(self):
        return {"pull_request": {
            "merged": True, "base": {"ref": "beta"}, "body": "Fixes #7",
            "html_url": f"https://github.com/{REPO}/pull/9",
        }}

    def test_a_merged_pr_fixing_the_projects_own_card_issue_marks_the_card_done(self):
        replies = {("GET", f"{ISSUES}/7"): issue(*BOT, state="open", reason=None),
                   ("GET", PAGE): DOING}
        rec = self.run_done(self.merged_pr(), replies)
        self.assertEqual([(m, u) for m, u, _ in rec.writes()],
                         [("PATCH", PAGE), ("PATCH", f"{ISSUES}/7")])
        self.assertEqual(rec.writes()[0][2]["properties"]["Status"], {"select": {"name": cards_sync.STATUS_DONE}})
        self.assertEqual(rec.writes()[1][2], {"state": "closed", "state_reason": "completed"})

    def test_a_merged_pr_fixing_an_outsiders_issue_with_a_card_marker_changes_nothing(self):
        replies = {("GET", f"{ISSUES}/7"): issue(*OUTSIDER, state="open", reason=None),
                   ("GET", PAGE): DOING}
        rec = self.run_done(self.merged_pr(), replies)
        self.assertEqual(rec.writes(), [])
        self.assertNotIn(("GET", PAGE, None), rec.calls)

    # --- the scheduled sweep --------------------------------------------------

    def sweep_replies(self, closed):
        return {("GET", f"{ISSUES}?state=open"): [],
                ("GET", f"{ISSUES}?state=closed"): closed,
                ("GET", PAGE): DOING,
                ("POST", f"{cards_sync.NOTION}/data_sources/ds1/query"): {"results": []}}

    def test_the_sweep_marks_the_projects_own_closed_card_issue_done(self):
        rec = self.run_with(self.sweep_replies([issue(*BOT)]), cards_sync.sync)
        self.assert_one_done_patch(rec, f"Done: https://github.com/{REPO}/issues/7")

    def test_the_sweep_ignores_an_outsiders_closed_issue_with_a_card_marker(self):
        rec = self.run_with(self.sweep_replies([issue(*OUTSIDER)]), cards_sync.sync)
        self.assertEqual(rec.writes(), [])
        self.assertNotIn(("GET", PAGE, None), rec.calls)

    def test_an_outsiders_newer_issue_cannot_take_over_a_cards_link(self):
        # Newest first, as the listing is sorted: the outsider's copy comes first.
        listing = [issue(*OUTSIDER, number=8, state="open", reason=None),
                   issue(*BOT, number=7, state="open", reason=None)]
        rec = self.run_with({("GET", f"{ISSUES}?state=open"): listing},
                            lambda: self.assertEqual(
                                {k: v["number"] for k, v in cards_sync.card_issues("open").items()},
                                {CARD: 7}))
        self.assertEqual(rec.writes(), [])

    def test_sync_sweeps_completed_cards_and_never_dispatches_claude_or_jules(self):
        open_issue = issue(*BOT, number=42, state="open", reason=None)
        merged_timeline = [{
            "event": "cross-referenced",
            "source": {"issue": {
                "number": 99,
                "repository": {"full_name": REPO},
                "pull_request": {"merged_at": "2026-11-18T00:00:00Z"},
            }},
        }]
        merged_pr = {
            "number": 99,
            "merged": True,
            "base": {"ref": "beta"},
            "body": "Fixes #42",
            "html_url": f"https://github.com/{REPO}/pull/99",
        }
        replies = {
            ("GET", f"{ISSUES}?state=open"): [open_issue],
            ("GET", f"{ISSUES}?state=closed"): [],
            ("GET", f"{ISSUES}/42/timeline"): merged_timeline,
            ("GET", f"{cards_sync.GH}/repos/{REPO}/pulls/99"): merged_pr,
            ("POST", f"{cards_sync.NOTION}/data_sources/ds1/query"): {"results": []},
            ("GET", PAGE): DOING,
        }
        rec = self.run_with(replies, cards_sync.sync)
        self.assertEqual(
            [(m, u) for m, u, _ in rec.writes()],
            [("PATCH", PAGE), ("PATCH", f"{ISSUES}/42")],
        )
        self.assertEqual(
            rec.writes()[0][2]["properties"]["Status"],
            {"select": {"name": cards_sync.STATUS_DONE}},
        )
        self.assertFalse(any("/dispatches" in url or "jules" in url for _, url, _ in rec.calls))

    def ready_card(self, card_id, name, assigned):
        return {
            "id": card_id,
            "url": f"https://www.notion.so/{card_id}",
            "properties": {
                "Name": {"type": "title", "title": [{"plain_text": name}]},
                "Assigned": {"type": "select", "select": {"name": assigned}},
                "Status": {"type": "select", "select": {"name": cards_sync.STATUS_READY}},
            },
        }

    def test_sync_creates_issues_only_for_antigravity_cards_and_leaves_status_ready(self):
        c_claude = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        c_ag = CARD
        replies = {
            ("POST", f"{cards_sync.NOTION}/data_sources/ds1/query"): {
                "results": [
                    self.ready_card(c_claude, "Claude task", "Claude"),
                    self.ready_card(c_ag, "Antigravity task", "Antigravity"),
                ]
            },
            ("GET", f"{cards_sync.NOTION}/blocks/{c_ag}/children"): {
                "results": [{"type": "paragraph", "paragraph": {"rich_text": [{"plain_text": "Details"}]}}],
                "has_more": False,
            },
            ("POST", ISSUES): {
                "number": 501,
                "html_url": f"https://github.com/{REPO}/issues/501",
                "labels": [{"name": "antigravity"}],
            },
        }
        rec = self.run_with(replies, cards_sync.sync)
        query_calls = [body for m, u, body in rec.calls if m == "POST" and u.endswith("/data_sources/ds1/query")]
        self.assertEqual(len(query_calls), 1)
        self.assertIn({"property": "Assigned", "select": {"equals": "Antigravity"}}, query_calls[0]["filter"]["and"])
        self.assertEqual([(m, u) for m, u, _ in rec.writes()], [("POST", ISSUES)])
        created = rec.writes()[0][2]
        self.assertEqual(created["title"], "Antigravity task")
        self.assertEqual(created["labels"], ["antigravity"])
        self.assertIn(f"<!-- notion-card: {c_ag} -->", created["body"])
        self.assertFalse(any("/dispatches" in url or "jules" in url for _, url, _ in rec.calls))

    def test_sync_respects_cap_and_allows_requeued_not_planned_card(self):
        c_open = "11111111111111111111111111111111"
        c_requeued = "22222222222222222222222222222222"
        c_overflow = "33333333333333333333333333333333"
        open_ag = issue(*BOT, number=10, state="open", reason=None, body=f"<!-- notion-card: {c_open} -->")
        open_ag["labels"] = [{"name": "antigravity"}]
        closed_np = issue(*BOT, number=9, state="closed", reason="not_planned",
                          body=f"<!-- notion-card: {c_requeued} -->")
        replies = {
            ("GET", f"{ISSUES}?state=open"): [open_ag],
            ("GET", f"{ISSUES}?state=closed"): [closed_np],
            ("POST", f"{cards_sync.NOTION}/data_sources/ds1/query"): {
                "results": [
                    self.ready_card(c_open, "Already open", "Antigravity"),
                    self.ready_card(c_requeued, "Requeued task", "Antigravity"),
                    self.ready_card(c_overflow, "Third task", "Antigravity"),
                ]
            },
            ("GET", f"{cards_sync.NOTION}/blocks/{c_requeued}/children"): {"results": [], "has_more": False},
            ("POST", ISSUES): {
                "number": 502,
                "html_url": f"https://github.com/{REPO}/issues/502",
                "labels": [{"name": "antigravity"}],
            },
        }
        rec = self.run_with(replies, cards_sync.sync)
        self.assertEqual([(m, u) for m, u, _ in rec.writes()], [("POST", ISSUES)])
        self.assertEqual(rec.writes()[0][2]["title"], "Requeued task")

    def test_start_antigravity_marks_working_on_it_now(self):
        ag_issue = issue(*BOT, number=7, state="open", reason=None)
        ag_issue["labels"] = [{"name": "antigravity"}]
        ready_page = self.ready_card(CARD, "My card", "Antigravity")
        replies = {
            ("GET", f"{ISSUES}/7"): ag_issue,
            ("GET", PAGE): ready_page,
        }
        with mock.patch.dict(os.environ, {"START_ISSUE": "7"}):
            rec = self.run_with(replies, cards_sync.sync)
        self.assertEqual([(m, u) for m, u, _ in rec.writes()], [("PATCH", PAGE)])
        props = rec.writes()[0][2]["properties"]
        self.assertEqual(props["Status"], {"select": {"name": cards_sync.STATUS_DOING}})
        self.assertIn("start", props["Picked up"]["date"])
        self.assertEqual(props["Result"]["rich_text"][0]["text"]["content"],
                         f"https://github.com/{REPO}/issues/7")

    def test_start_antigravity_refuses_claude_cards(self):
        claude_issue = issue(*BOT, number=8, state="open", reason=None)
        claude_issue["labels"] = [{"name": "claude"}]
        mismatched_issue = issue(*BOT, number=9, state="open", reason=None)
        mismatched_issue["labels"] = [{"name": "antigravity"}]
        replies = {
            ("GET", f"{ISSUES}/8"): claude_issue,
            ("GET", f"{ISSUES}/9"): mismatched_issue,
            ("GET", PAGE): self.ready_card(CARD, "Actually Claude", "Claude"),
        }
        with mock.patch.dict(os.environ, {"START_ISSUE": "8,9"}):
            rec = self.run_with(replies, cards_sync.sync)
        self.assertEqual(rec.writes(), [])

    def test_reset_ready_puts_cards_back_to_ready_and_closes_not_planned(self):
        replies = {
            ("GET", f"{ISSUES}/2004"): issue(*BOT, number=2004, state="open", reason=None),
            ("GET", PAGE): DOING,
        }
        with mock.patch.dict(os.environ, {"RESET_ISSUES": "2004"}):
            rec = self.run_with(replies, cards_sync.sync)
        self.assertEqual(
            [(m, u) for m, u, _ in rec.writes()],
            [("PATCH", PAGE), ("PATCH", f"{ISSUES}/2004")],
        )
        props = rec.writes()[0][2]["properties"]
        self.assertEqual(props["Status"], {"select": {"name": cards_sync.STATUS_READY}})
        self.assertEqual(props["Picked up"], {"date": None})
        self.assertEqual(props["Result"], {"rich_text": []})
        self.assertEqual(rec.writes()[1][2], {"state": "closed", "state_reason": "not_planned"})

    def test_archived_notion_card_still_closes_open_issue_on_merge(self):
        replies = {
            ("GET", f"{ISSUES}/7"): issue(*BOT, state="open", reason=None),
            ("GET", PAGE): {"archived": True, **DOING},
        }
        rec = self.run_done(self.merged_pr(), replies)
        self.assertEqual([(m, u, b) for m, u, b in rec.writes()],
                         [("PATCH", f"{ISSUES}/7", {"state": "closed", "state_reason": "completed"})])


if __name__ == "__main__":
    unittest.main()
