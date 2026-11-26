#!/usr/bin/env python3
"""Tests for pii_guard.py. Each case copies the guard into a throwaway git repo
beside its own hash list of made-up test words (never real ones), so these
fixtures hold nothing personal. Run: python3 scripts/pii-guard/test_pii_guard.py"""
import hashlib, json, os, shutil, subprocess, sys, tempfile, unittest

GUARD = os.path.join(os.path.dirname(os.path.abspath(__file__)), "pii_guard.py")
WORD, HOME_USER = "zorblax", "quuxuser"
GIT_ENV = {
    "GIT_AUTHOR_NAME": "user", "GIT_AUTHOR_EMAIL": "user@users.noreply.github.com",
    "GIT_COMMITTER_NAME": "user", "GIT_COMMITTER_EMAIL": "user@users.noreply.github.com",
    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
}
sha = lambda s: hashlib.sha256(s.encode()).hexdigest()


class PiiGuard(unittest.TestCase):
    def setUp(self):
        self.repo = tempfile.mkdtemp(prefix="pii-guard-test-")
        self.addCleanup(shutil.rmtree, self.repo, True)
        here = os.path.join(self.repo, "scripts", "pii-guard")
        os.makedirs(here)
        shutil.copy(GUARD, here)
        with open(os.path.join(here, "pii-hashes.txt"), "w") as f:
            f.write(f"L {sha(WORD)}\nH {sha(HOME_USER)}\n")
        self.git("init", "-q", "-b", "beta")
        self.write("README.md", "A clean file.\n")
        self.commit("base")

    def git(self, *args, env=None):
        return subprocess.run(["git", *args], cwd=self.repo, check=True, capture_output=True,
                              text=True, env={**os.environ, **GIT_ENV, **(env or {})}).stdout

    def write(self, path, text):
        full = os.path.join(self.repo, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w") as f:
            f.write(text)

    def commit(self, msg, env=None):
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", msg, env=env)

    def guard(self, *args, env=None):
        clean = {k: v for k, v in os.environ.items() if not k.startswith("GITHUB_")}
        r = subprocess.run([sys.executable, "scripts/pii-guard/pii_guard.py", *args], cwd=self.repo,
                           capture_output=True, text=True, env={**clean, **(env or {})})
        return r.returncode, r.stdout + r.stderr

    def test_a_clean_repo_passes(self):
        self.assertEqual(self.guard(), (0, "pii-guard: clean\n"))

    def test_a_blocked_word_in_plain_text_fails(self):
        self.write("notes.md", f"Ask {WORD.title()} about it.\n")
        self.commit("notes")
        self.assertEqual(self.guard()[0], 1)

    def test_a_blocked_word_inside_an_email_address_fails(self):
        self.write("notes.md", f"Mail {WORD}.smith@example.org for access.\n")
        self.commit("notes")
        rc, out = self.guard()
        self.assertEqual(rc, 1)
        self.assertIn("notes.md:1: blocked personal word", out)

    def test_a_windows_home_path_fails(self):
        self.write("src/paths.rs", f'const P: &str = "C:\\\\Users\\\\{HOME_USER}\\\\AppData";\n')
        self.commit("paths")
        rc, out = self.guard()
        self.assertEqual(rc, 1)
        self.assertIn("src/paths.rs:1: real home-folder path", out)

    def test_a_blocked_word_in_a_file_name_fails(self):
        self.write(f"docs/{WORD}-plan.md", "Nothing personal in here.\n")
        self.commit("plan")
        rc, out = self.guard()
        self.assertEqual(rc, 1)
        self.assertIn(f"docs/{WORD}-plan.md: blocked personal word in the file name", out)

    def test_a_push_run_with_no_base_checks_the_pushed_commits(self):
        before = self.git("rev-parse", "HEAD").strip()
        self.write("a.md", "fine\n")
        self.commit("first", env={"GIT_AUTHOR_NAME": WORD.title()})
        self.write("b.md", "fine too\n")
        self.commit("second")
        self.assertEqual(self.guard("beta~2")[0], 1, "with a base it already fails")
        # No event payload (a local run): the newest commit alone, which is clean.
        self.assertEqual(self.guard(), (0, "pii-guard: clean\n"))
        # A push: every commit since the event's `before`.
        event = os.path.join(self.repo, "..", os.path.basename(self.repo) + "-event.json")
        self.addCleanup(lambda: os.path.exists(event) and os.remove(event))
        with open(event, "w") as f:
            json.dump({"before": before, "after": "HEAD"}, f)
        rc, out = self.guard(env={"GITHUB_EVENT_PATH": event})
        self.assertEqual(rc, 1)
        self.assertIn("author: blocked personal word", out)

    def test_a_push_of_a_new_branch_checks_its_head_commit(self):
        self.write("a.md", "fine\n")
        self.commit(f"thanks {WORD}")
        event = os.path.join(self.repo, "..", os.path.basename(self.repo) + "-event.json")
        self.addCleanup(lambda: os.path.exists(event) and os.remove(event))
        with open(event, "w") as f:
            json.dump({"before": "0" * 40}, f)
        rc, out = self.guard(env={"GITHUB_EVENT_PATH": event})
        self.assertEqual(rc, 1)
        self.assertIn("message: blocked personal word", out)

    def test_a_base_git_cannot_read_fails_closed(self):
        rc, out = self.guard("origin/no-such-branch")
        self.assertEqual(rc, 2)
        self.assertIn("pii-guard: couldn't read the commits", out)


if __name__ == "__main__":
    unittest.main()
