#!/usr/bin/env python3
"""Tests for the vstack skill sync (stdlib and git only, no network).

    python3 .github/scripts/test_vstack_sync.py
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import vstack_sync as vs  # noqa: E402

COMMIT = "c296c0f714d0934353f99879e6f8b00ffdc7f789"


def manifest(**over):
    doc = {
        "about": "test",
        "repo": "https://example.invalid/vstack",
        "tag": "v1.0.0",
        "commit": COMMIT,
        "always_on": ["qa"],
        "skills": ["qa", "tdd"],
        "exclude": {"setup": "installs into an agent"},
    }
    doc.update(over)
    return doc


def run(*args, cwd):
    subprocess.run(args, cwd=cwd, check=True, capture_output=True)


class Shipped(unittest.TestCase):
    def test_the_shipped_bundle_matches_its_manifest(self):
        vs.check()

    def test_the_shipped_manifest_is_in_canonical_form(self):
        with open(vs.MANIFEST, encoding="utf-8") as fh:
            self.assertEqual(vs.emit_manifest(vs.load()), fh.read())


class Validate(unittest.TestCase):
    def refused(self, doc, words):
        with self.assertRaises(vs.SyncError) as err:
            vs.validate(doc)
        self.assertIn(words, str(err.exception))

    def test_a_good_manifest_passes(self):
        self.assertEqual(vs.validate(manifest())["skills"], ["qa", "tdd"])

    def test_bad_manifests_are_refused(self):
        self.refused(manifest(tag="1.0"), "is not a vX.Y.Z tag")
        self.refused(manifest(commit="c296c0f"), "full 40-character commit")
        self.refused(manifest(skills=["tdd", "qa"]), "sorted with no repeats")
        self.refused(manifest(always_on=["why"]), "always-on skills not in `skills`: why")
        self.refused(manifest(exclude={"tdd": "no"}), "both bundled and excluded: tdd")
        self.refused(manifest(exclude={"setup": " "}), "map each skill to a reason")
        doc = manifest()
        doc["extra"] = 1
        self.refused(doc, "keys must be, in order")


class Plan(unittest.TestCase):
    def upstream(self, *names):
        return {n: {"SKILL.md": b"x"} for n in names}

    def test_new_upstream_skills_join_and_gone_ones_leave(self):
        doc = manifest(skills=["old", "qa", "tdd"])
        nxt, added, removed = vs.plan(doc, self.upstream("qa", "tdd", "elevate", "setup"))
        self.assertEqual(nxt["skills"], ["elevate", "qa", "tdd"])
        self.assertEqual(added, ["elevate"])
        self.assertEqual(removed, ["old"])
        self.assertEqual(nxt["exclude"], {"setup": "installs into an agent"})

    def test_an_excluded_skill_gone_upstream_drops_from_the_list(self):
        nxt, _, _ = vs.plan(manifest(), self.upstream("qa", "tdd"))
        self.assertEqual(nxt["exclude"], {})

    def test_an_always_on_skill_gone_upstream_stops_the_sync(self):
        with self.assertRaises(vs.SyncError) as err:
            vs.plan(manifest(), self.upstream("tdd"))
        self.assertIn("always-on skills left vstack: qa", str(err.exception))

    def test_an_excluded_folder_may_have_any_name(self):
        doc = manifest(exclude={"_shared": "helpers", "setup": "installs into an agent"})
        nxt, added, _ = vs.plan(doc, self.upstream("qa", "tdd", "_shared"))
        self.assertEqual(added, [])
        self.assertEqual(nxt["exclude"], {"_shared": "helpers"})
        with self.assertRaises(vs.SyncError) as err:
            vs.plan(manifest(), self.upstream("qa", "tdd", "_shared"))
        self.assertIn("`_shared` is not a plain name; exclude it", str(err.exception))

    def test_a_skill_without_skill_md_stops_the_sync(self):
        up = self.upstream("qa")
        up["tdd"] = {"GUIDE.md": b"x"}
        with self.assertRaises(vs.SyncError) as err:
            vs.plan(manifest(), up)
        self.assertIn("`tdd` has no SKILL.md", str(err.exception))


class Files(unittest.TestCase):
    def refused(self, rel, data, words):
        with self.assertRaises(vs.SyncError) as err:
            vs.check_file("qa", rel, data)
        self.assertIn(words, str(err.exception))

    def test_text_files_pass(self):
        vs.check_file("qa", "scripts/closeout.py", b"print('ok')\n")

    def test_files_the_skill_tool_cannot_serve_are_refused(self):
        self.refused("logo.png", b"\x89PNG\0", "is binary")
        self.refused("a.md", b"\xff\xfe", "is not UTF-8")
        self.refused("big.py", b"x" * (48 * 1024 + 1), "over the skill tool's 49152")
        self.refused("crlf.md", b"a\r\nb\r\n", "CR line endings")
        self.refused("../up.md", b"x", "unexpected characters")
        self.refused("GUIDE.md", None, "is a symlink or submodule")


class Rust(unittest.TestCase):
    def test_short_tuples_stay_on_one_line(self):
        self.assertEqual(
            vs.tuple_lines(['"how"', 'include_str!("../assets/skills/how/SKILL.md")'], 4),
            ['    ("how", include_str!("../assets/skills/how/SKILL.md")),'],
        )

    def test_long_tuples_wrap_one_item_per_line(self):
        self.assertEqual(
            vs.tuple_lines(['"architect"', 'include_str!("../assets/skills/architect/SKILL.md")'], 4),
            [
                "    (",
                '        "architect",',
                '        include_str!("../assets/skills/architect/SKILL.md"),',
                "    ),",
            ],
        )

    def test_an_include_past_max_width_splits_its_path(self):
        path = "../assets/skills/principle-separate-before-serializing-shared-state/SKILL.md"
        self.assertEqual(
            vs.tuple_lines(['"x"', f'include_str!("{path}")'], 4),
            ["    (", '        "x",', "        include_str!(", f'            "{path}"', "        ),", "    ),"],
        )


class EndToEnd(unittest.TestCase):
    """A fake vstack repo and a fake assets folder, synced twice."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="vstack-test-")
        self.src = os.path.join(self.tmp, "vstack")
        os.makedirs(self.src)
        run("git", "init", "-q", cwd=self.src)
        files = {
            "LICENSE": "MIT License\n",
            "skills/qa/SKILL.md": "---\nname: qa\ndescription: Ship check.\n---\n# QA\n",
            "skills/tdd/SKILL.md": "---\nname: tdd\ndescription: Test first.\n---\n# TDD\n",
            "skills/elevate/SKILL.md": "---\nname: elevate\ndescription: UI craft.\n---\nRead GUIDE.md.\n",
            "skills/elevate/GUIDE.md": "# Guide\n",
            "skills/setup/SKILL.md": "---\nname: setup\ndescription: Install.\n---\n",
        }
        for rel, text in files.items():
            path = os.path.join(self.src, rel)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(text)
        run("git", "add", "-A", cwd=self.src)
        run("git", "-c", "user.name=t", "-c", "user.email=t@example.invalid",
            "commit", "-q", "-m", "v2", cwd=self.src)
        run("git", "tag", "v2.0.0", cwd=self.src)
        self.commit = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=self.src, check=True, capture_output=True, text=True
        ).stdout.strip()

        agent = os.path.join(self.tmp, "agent")
        assets = os.path.join(agent, "assets", "skills")
        os.makedirs(os.path.join(assets, "qa"))
        os.makedirs(os.path.join(agent, "src"))
        with open(os.path.join(assets, "qa", "SKILL.md"), "w") as fh:
            fh.write("stale\n")
        os.makedirs(os.path.join(assets, "retired"))
        with open(os.path.join(assets, "retired", "SKILL.md"), "w") as fh:
            fh.write("gone\n")
        self.saved = {k: getattr(vs, k) for k in ("ROOT", "ASSETS", "MANIFEST", "CREDITS", "LICENSE", "TABLE")}
        vs.ROOT = self.tmp
        vs.ASSETS = assets
        vs.MANIFEST = os.path.join(assets, "vstack.json")
        vs.CREDITS = os.path.join(assets, "CREDITS.md")
        vs.LICENSE = os.path.join(assets, "LICENSE-upstream")
        vs.TABLE = os.path.join(agent, "src", "builtin_skills.rs")
        with open(vs.MANIFEST, "w") as fh:
            fh.write(vs.emit_manifest(manifest(skills=["qa", "retired", "tdd"])))

    def tearDown(self):
        for k, v in self.saved.items():
            setattr(vs, k, v)
        shutil.rmtree(self.tmp, ignore_errors=True)

    def snapshot(self):
        out = {}
        for base, _, files in os.walk(self.tmp):
            if base.startswith(self.src):
                continue
            for f in files:
                path = os.path.join(base, f)
                with open(path, "rb") as fh:
                    out[os.path.relpath(path, self.tmp)] = fh.read()
        return out

    def test_sync_writes_the_tag_and_gives_the_same_files_twice(self):
        summary = os.path.join(self.tmp, "summary.md")
        vs.sync("v2.0.0", self.src, summary)
        with open(summary, encoding="utf-8") as fh:
            note = fh.read()
        os.remove(summary)
        first = self.snapshot()
        vs.sync("v2.0.0", self.src, None)
        self.assertEqual(self.snapshot(), first)
        vs.check()

        doc = vs.load()
        self.assertEqual(doc["tag"], "v2.0.0")
        self.assertEqual(doc["commit"], self.commit)
        self.assertEqual(doc["skills"], ["elevate", "qa", "tdd"])
        self.assertEqual(doc["always_on"], ["qa"])
        self.assertEqual(doc["exclude"], {"setup": "installs into an agent"})
        assets = "agent/assets/skills/"
        self.assertEqual(first[assets + "qa/SKILL.md"], b"---\nname: qa\ndescription: Ship check.\n---\n# QA\n")
        self.assertEqual(first[assets + "elevate/GUIDE.md"], b"# Guide\n")
        self.assertEqual(first[assets + "LICENSE-upstream"], b"MIT License\n")
        self.assertNotIn(assets + "retired/SKILL.md", first)
        self.assertNotIn(assets + "setup/SKILL.md", first)

        table = first["agent/src/builtin_skills.rs"].decode()
        # rustfmt, when present, owns the layout; check the tokens.
        flat = vs.squash(table)
        self.assertIn('("qa",include_str!("../assets/skills/qa/SKILL.md"))', flat)
        self.assertIn('("elevate","GUIDE.md",include_str!("../assets/skills/elevate/GUIDE.md"))', flat)
        self.assertIn('pubconstBUILTIN_ALWAYS_ON:&[&str]=&["qa"];', flat)
        credits = first[assets + "CREDITS.md"].decode()
        self.assertIn(f"release\n[`v2.0.0`](https://example.invalid/vstack/releases/tag/v2.0.0) (commit `{self.commit}`)", credits)
        self.assertIn("Skills shipped here (read-only built-ins inside Loadout / GrokHub):\n\n- elevate\n- qa\n- tdd\n", credits)
        self.assertIn("Left out:\n\n- setup: installs into an agent\n", credits)
        self.assertIn("from `v1.0.0` to `v2.0.0`", note)
        self.assertIn("- New upstream, now bundled: elevate.\n", note)
        self.assertIn("- Gone upstream, removed: retired.\n", note)

    def test_check_catches_a_hand_edit(self):
        vs.sync("v2.0.0", self.src, None)
        with open(os.path.join(vs.ASSETS, "tdd", "SKILL.md"), "a") as fh:
            fh.write("edited\n")
        with self.assertRaises(vs.SyncError) as err:
            vs.check()
        self.assertIn("CREDITS.md is stale", str(err.exception))

    def test_check_catches_a_folder_the_manifest_does_not_list(self):
        vs.sync("v2.0.0", self.src, None)
        os.makedirs(os.path.join(vs.ASSETS, "extra"))
        with open(os.path.join(vs.ASSETS, "extra", "SKILL.md"), "w") as fh:
            fh.write("x\n")
        with self.assertRaises(vs.SyncError) as err:
            vs.check()
        self.assertIn("extra ['extra']", str(err.exception))

    def test_a_symlink_in_a_bundled_skill_stops_the_sync(self):
        os.symlink("SKILL.md", os.path.join(self.src, "skills", "tdd", "LINK.md"))
        run("git", "add", "-A", cwd=self.src)
        run("git", "-c", "user.name=t", "-c", "user.email=t@example.invalid",
            "commit", "-q", "-m", "link", cwd=self.src)
        run("git", "tag", "v2.0.1", cwd=self.src)
        self.assertIsNone(vs.upstream_files(self.src, "v2.0.1")["tdd"]["LINK.md"])
        before = self.snapshot()
        with self.assertRaises(vs.SyncError) as err:
            vs.sync("v2.0.1", self.src, None)
        self.assertIn("tdd/LINK.md is a symlink or submodule", str(err.exception))
        self.assertEqual(self.snapshot(), before)

    def test_a_stray_file_stops_the_sync_before_anything_is_deleted(self):
        with open(os.path.join(vs.ASSETS, "README.md"), "w") as fh:
            fh.write("notes\n")
        before = self.snapshot()
        with self.assertRaises(vs.SyncError) as err:
            vs.sync("v2.0.0", self.src, None)
        self.assertIn("stray file assets/skills/README.md", str(err.exception))
        self.assertEqual(self.snapshot(), before)

    def test_check_ignores_layout_but_not_content_in_the_table(self):
        vs.sync("v2.0.0", self.src, None)
        with open(vs.TABLE) as fh:
            table = fh.read()
        with open(vs.TABLE, "w", newline="") as fh:
            fh.write(table.replace("\n", "\r\n").replace('("qa", ', '(\n    "qa",\n    '))
        vs.check()
        with open(vs.TABLE, "w") as fh:
            fh.write(table.replace("skills/tdd/SKILL.md", "skills/qa/SKILL.md"))
        with self.assertRaises(vs.SyncError) as err:
            vs.check()
        self.assertIn("builtin_skills.rs is stale", str(err.exception))

    def test_a_tag_older_than_the_pin_is_refused(self):
        vs.sync("v2.0.0", self.src, None)
        before = self.snapshot()
        run("git", "tag", "v1.9.0", cwd=self.src)
        with self.assertRaises(vs.SyncError) as err:
            vs.sync("v1.9.0", self.src, None)
        self.assertIn("v1.9.0 is older than the pinned v2.0.0", str(err.exception))
        self.assertEqual(self.snapshot(), before)

    def test_a_failed_sync_leaves_the_bundle_alone(self):
        with open(vs.MANIFEST, "w") as fh:
            fh.write(vs.emit_manifest(manifest(always_on=["qa", "gone"], skills=["gone", "qa", "tdd"])))
        before = self.snapshot()
        with self.assertRaises(vs.SyncError):
            vs.sync("v2.0.0", self.src, None)
        self.assertEqual(self.snapshot(), before)


if __name__ == "__main__":
    unittest.main(verbosity=1)
