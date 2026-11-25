#!/usr/bin/env python3
"""Tests for affected_crates.py against a fixed fake workspace."""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from affected_crates import WORKSPACE, select  # noqa: E402

ROOT = "/w"


def pkg(name, rel, deps=()):
    return {
        "id": name + " 0.1.0",
        "name": name,
        "manifest_path": f"{ROOT}/{rel}/Cargo.toml",
        "dependencies": [{"name": d} for d in deps] + [{"name": "serde"}],
    }


META = {
    "workspace_root": ROOT,
    "workspace_members": [n + " 0.1.0" for n in ("proto", "core", "agent", "probe", "desktop", "hub", "code", "xtask")],
    "packages": [
        pkg("proto", "crates/proto"),
        pkg("core", "crates/core", ["proto"]),
        pkg("probe", "crates/probe"),
        pkg("agent", "crates/agent", ["core", "probe"]),
        pkg("desktop", "crates/desktop", ["agent", "core"]),
        pkg("hub", "crates/hub", ["agent", "core", "desktop"]),
        pkg("code", "crates/code", ["agent", "core"]),
        pkg("xtask", "xtask"),
    ],
}


class Select(unittest.TestCase):
    def test_leaf_crate_runs_only_itself(self):
        self.assertEqual(select(META, ["crates/code/src/lib.rs"]), ["-p", "code"])

    def test_xtask_alone(self):
        self.assertEqual(select(META, ["xtask/src/main.rs"]), ["-p", "xtask"])

    def test_agent_pulls_in_its_dependents(self):
        self.assertEqual(
            select(META, ["crates/agent/src/run.rs"]),
            ["-p", "agent", "-p", "code", "-p", "desktop", "-p", "hub"],
        )

    def test_probe_dependents_are_transitive(self):
        self.assertEqual(
            select(META, ["crates/probe/src/lib.rs"]),
            ["-p", "agent", "-p", "code", "-p", "desktop", "-p", "hub", "-p", "probe"],
        )

    def test_proto_skips_crates_that_do_not_use_it(self):
        # probe and xtask don't depend on proto, directly or otherwise.
        self.assertEqual(
            select(META, ["crates/proto/src/lib.rs"]),
            ["-p", "agent", "-p", "code", "-p", "core", "-p", "desktop", "-p", "hub", "-p", "proto"],
        )

    def test_markdown_inside_a_crate_counts_for_that_crate(self):
        self.assertEqual(select(META, ["crates/code/prompts/system.md"]), ["-p", "code"])

    def test_root_cargo_files_run_everything(self):
        self.assertEqual(select(META, ["crates/code/src/lib.rs", "Cargo.lock"]), [WORKSPACE])
        self.assertEqual(select(META, ["Cargo.toml"]), [WORKSPACE])

    def test_crate_manifest_runs_everything(self):
        self.assertEqual(select(META, ["crates/code/Cargo.toml"]), [WORKSPACE])

    def test_paths_outside_crates_run_everything(self):
        for p in ("assets/skills/x/SKILL.md", "data/model_profiles.toml", ".github/workflows/ci.yml",
                  "scripts/install.sh", "src-tauri/src/main.rs", "ui/src/App.tsx", ".config/nextest.toml",
                  "PRIVACY.md", "docs/compass/a.md", "docs/eval/b.json"):
            with self.subTest(p=p):
                self.assertEqual(select(META, [p]), [WORKSPACE])

    def test_docs_only_falls_back_to_everything(self):
        # The changes job skips these PRs; if tests run anyway, run them all.
        self.assertEqual(select(META, ["docs/guide.md", "README.md"]), [WORKSPACE])

    def test_ignored_paths_do_not_widen_a_crate_pick(self):
        self.assertEqual(
            select(META, ["crates/code/src/a.rs", "docs/guide.md", "README.md", "screenshots/a.png", "research/n.md"]),
            ["-p", "code"],
        )

    def test_empty_diff_runs_everything(self):
        self.assertEqual(select(META, []), [WORKSPACE])
        self.assertEqual(select(META, ["", "  "]), [WORKSPACE])

    def test_root_package_disables_narrowing(self):
        meta = dict(META, packages=META["packages"] + [pkg("rootpkg", ".")],
                    workspace_members=META["workspace_members"] + ["rootpkg 0.1.0"])
        meta["packages"][-1]["manifest_path"] = f"{ROOT}/Cargo.toml"
        self.assertEqual(select(meta, ["crates/code/src/lib.rs"]), [WORKSPACE])

    def test_non_member_packages_are_ignored(self):
        meta = dict(META, packages=META["packages"] + [pkg("vendored", "vendor/x", ["code"])])
        self.assertEqual(select(meta, ["crates/code/src/lib.rs"]), ["-p", "code"])


if __name__ == "__main__":
    unittest.main()
