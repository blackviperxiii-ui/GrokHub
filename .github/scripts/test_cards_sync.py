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
DOING = {"properties": {"Status": {"type": "select", "select": {"name": "Doing"}}}}


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
        self.assertEqual(body["properties"]["Status"], {"select": {"name": "Done"}})
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
        self.assertEqual(rec.writes()[0][2]["properties"]["Status"], {"select": {"name": "Done"}})
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


if __name__ == "__main__":
    unittest.main()
