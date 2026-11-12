#!/usr/bin/env python3
"""Tests for the model profiles updater (stdlib only, no network).

    python3 .github/scripts/test_model_profiles.py
"""
import copy
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model_profiles as mp  # noqa: E402

TODAY = "2026-11-20"


def listing_row(model_id, prompt="0", completion="0", **extra):
    row = {
        "id": model_id,
        "name": model_id,
        "context_length": extra.pop("context", 128000),
        "pricing": {"prompt": prompt, "completion": completion},
        "supported_parameters": extra.pop("params", ["tools", "max_tokens"]),
        "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]},
        "top_provider": {"context_length": None, "max_completion_tokens": None, "is_moderated": False},
    }
    row.update(extra)
    return row


class Format(unittest.TestCase):
    def test_the_shipped_file_is_in_canonical_form(self):
        with open(mp.PROFILES, encoding="utf-8") as fh:
            text = fh.read()
        self.assertEqual(mp.emit(mp.load()), text, "run: model_profiles.py format")

    def test_an_unknown_field_is_refused(self):
        doc = {"schema": 1, "updated": TODAY, "model": [{"key": "a", "prise": 1}]}
        with self.assertRaises(ValueError):
            mp.emit(doc)


class Facts(unittest.TestCase):
    def test_prices_convert_to_cents_per_100m(self):
        facts = mp.listing_facts(listing_row(
            "v/m", prompt="0.0000015", completion="0.000006",
            params=["tools", "response_format", "reasoning"],
        ))
        self.assertEqual((facts["prompt"], facts["completion"]), (15_000, 60_000))
        self.assertTrue(facts["tools"] and facts["structured"] and facts["reasoning"])
        self.assertEqual(mp._cents("-1"), None)

    def test_the_repo_fixture_parses(self):
        path = os.path.join(mp.ROOT, "crates", "grokhub-core", "tests", "fixtures", "openrouter_models.json")
        if not os.path.exists(path):
            self.skipTest("fixture moved")
        with open(path, encoding="utf-8") as fh:
            listing = mp.index_listing(fh.read())
        self.assertIn("x-ai/grok-test", listing)
        self.assertEqual(mp.listing_facts(listing["x-ai/grok-test"])["prompt"], 30_000)


class Drift(unittest.TestCase):
    def setUp(self):
        self.doc = {"schema": 1, "updated": "2026-11-12", "model": [
            {"key": "v/paid", "provider": "openrouter", "prompt": 10_000, "completion": 50_000,
             "verified": "2026-11-12"},
            {"key": "v/pinned", "provider": "openrouter", "prompt": 1_000, "pin": ["prompt"]},
            {"key": "v/old:free", "provider": "openrouter", "prompt": 0, "completion": 0},
            {"key": "claude-x-1.0", "provider": "anthropic", "mirror": "anthropic/claude-x-1.0",
             "prompt": 20_000},
        ]}
        rows = [
            listing_row("v/paid", prompt="0.0000012", completion="0.000005"),
            listing_row("v/pinned", prompt="0.0000009"),
            listing_row("v/new:free"),
            listing_row("v/image:free", architecture={"input_modalities": ["text"], "output_modalities": ["image"]}),
            listing_row("v/brand-new", prompt="0.000001"),
            listing_row("anthropic/claude-x-1.0", prompt="0.000003"),
        ]
        self.listing = {r["id"]: r for r in rows}

    def test_drift_names_changes_new_and_gone_models(self):
        r = mp.drift(self.doc, self.listing, TODAY)
        changed = {(k, f) for k, f, _, _ in r["changes"]}
        self.assertIn(("v/paid", "prompt"), changed)
        self.assertNotIn(("v/pinned", "prompt"), changed, "a pin beats the listing")
        self.assertEqual(r["new_free"], ["v/new:free"], "text models only")
        self.assertEqual(r["new_paid"], ["v/brand-new"])
        self.assertEqual(r["gone"], ["v/old:free"])
        self.assertEqual(r["mirror_drift"], [("claude-x-1.0", "prompt", 20_000, 30_000)])

    def test_write_applies_it_and_is_idempotent(self):
        doc = copy.deepcopy(self.doc)
        mp.apply(doc, self.listing, mp.drift(doc, self.listing, TODAY))
        rows = {r["key"]: r for r in doc["model"]}
        self.assertEqual(rows["v/paid"]["prompt"], 12_000)
        self.assertEqual(rows["v/paid"]["verified"], TODAY)
        self.assertEqual(rows["v/pinned"]["prompt"], 1_000)
        self.assertEqual(rows["v/old:free"]["gone_since"], TODAY)
        self.assertTrue(rows["v/new:free"]["probation"])
        self.assertEqual(rows["claude-x-1.0"]["prompt"], 20_000, "direct rows are only reported")
        self.assertEqual(doc["updated"], TODAY)
        once = mp.emit(doc)
        again = mp.apply(copy.deepcopy(doc), self.listing, mp.drift(doc, self.listing, "2026-11-27"))
        self.assertEqual(mp.emit(again), once, "a second run with no news changes nothing")

    def test_a_model_listed_again_loses_its_gone_mark(self):
        doc = copy.deepcopy(self.doc)
        doc["model"][2]["gone_since"] = "2026-11-12"
        self.listing["v/old:free"] = listing_row("v/old:free")
        mp.apply(doc, self.listing, mp.drift(doc, self.listing, TODAY))
        self.assertNotIn("gone_since", doc["model"][2])

    def test_a_listing_missing_most_rows_is_refused(self):
        doc = {"schema": 1, "updated": "2026-11-12", "model": [
            {"key": f"v/m{i}", "provider": "openrouter", "prompt": 1} for i in range(10)
        ]}
        self.assertTrue(mp.looks_partial(doc, mp.drift(doc, {}, TODAY)))
        self.assertFalse(mp.looks_partial(self.doc, mp.drift(self.doc, self.listing, TODAY)))

    def test_the_command_line_writes_the_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            prof = os.path.join(tmp, "p.toml")
            lst = os.path.join(tmp, "l.json")
            with open(prof, "w", encoding="utf-8") as fh:
                fh.write(mp.emit(self.doc))
            with open(lst, "w", encoding="utf-8") as fh:
                json.dump({"data": list(self.listing.values())}, fh)
            self.assertEqual(mp.main(["write", "--profiles", prof, "--listing", lst, "--today", TODAY]), 0)
            self.assertEqual(mp.load(prof)["updated"], TODAY)


if __name__ == "__main__":
    unittest.main()
