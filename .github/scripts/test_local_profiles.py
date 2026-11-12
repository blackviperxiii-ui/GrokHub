#!/usr/bin/env python3
"""Tests for the local model profile filler (stdlib only).

    python3 .github/scripts/test_local_profiles.py
"""
import os
import sys
import tomllib
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import local_profiles as lp  # noqa: E402

ROW = """[[model]]
id = "{id}"
kind = "chat"
download_mb = {dl}
run_mb = {run}
params_b = {total}
active_b = {active}
ctx_max = 131072
skills = {{ quality = {q} }}
tok_s = {{ gpu = 50, unified = 30, cpu = 10 }}
"""


def row(id="m", dl=2_000, run=3_000, total=4, active=4, q=40):
    return ROW.format(id=id, dl=dl, run=run, total=total, active=active, q=q)


class Profiles(unittest.TestCase):
    def test_the_shipped_catalog_is_up_to_date(self):
        with open(lp.CATALOG, encoding="utf-8") as fh:
            text = fh.read()
        self.assertEqual(lp.rewrite(text), text, "run: local_profiles.py write")
        self.assertEqual(lp.missing(text), [])

    def test_speed_rises_with_the_card_and_skips_a_card_it_overflows(self):
        small = lp.tier_speeds(tomllib.loads(row())["model"][0])
        self.assertLess(small["T1"], small["T4"])
        big = lp.tier_speeds(tomllib.loads(row(dl=12_000, run=14_000))["model"][0])
        self.assertEqual(sorted(big), ["T3", "T4"])

    def test_a_mixture_of_experts_reads_only_its_active_share(self):
        dense = lp.tier_speeds(tomllib.loads(row(dl=10_000, run=12_000, total=20, active=20))["model"][0])
        moe = lp.tier_speeds(tomllib.loads(row(dl=10_000, run=12_000, total=20, active=2))["model"][0])
        self.assertGreater(moe["T4"], 5 * dense["T4"])

    def test_gaming_safe_needs_both_light_and_capable(self):
        self.assertTrue(lp.gaming_safe(tomllib.loads(row())["model"][0]))
        self.assertFalse(lp.gaming_safe(tomllib.loads(row(run=6_000))["model"][0]))
        self.assertFalse(lp.gaming_safe(tomllib.loads(row(q=20))["model"][0]))

    def test_write_is_idempotent_and_keeps_a_measured_field_out_of_estimate(self):
        text = row().replace("ctx_max = 131072\n", 'ctx_max = 131072\nmeasured = ["ctx_max"]\n')
        once = lp.rewrite(text)
        self.assertEqual(lp.rewrite(once), once)
        parsed = tomllib.loads(once)["model"][0]
        self.assertEqual(parsed["estimate"], ["params_b", "active_b", "tok_s_tier"])
        self.assertTrue(parsed["gaming_safe"])


if __name__ == "__main__":
    unittest.main()
