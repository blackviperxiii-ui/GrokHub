#!/usr/bin/env python3
"""Personal-info guard. Fails if tracked files or new commits contain the owner's
personal details. Blocked words are stored only as SHA-256 hashes so this file
never leaks them. Usage: pii_guard.py [BASE_REF]. Commit metadata is checked for
BASE_REF..HEAD; with no BASE_REF, for a push event's before..HEAD (or HEAD alone
for a new branch or a local run). Tests: test_pii_guard.py."""
import hashlib, json, os, re, subprocess, sys
HERE = os.path.dirname(os.path.abspath(__file__))
LOW, EXACT, HOMEU = set(), set(), set()
for line in open(os.path.join(HERE, "pii-hashes.txt")):
    p = line.split()
    if len(p) == 2 and p[0] in ("L", "E", "H"):
        {"L": LOW, "E": EXACT, "H": HOMEU}[p[0]].add(p[1])
ALLOW_PATHS = []
af = os.path.join(HERE, "pii-allow.txt")
if os.path.exists(af):
    ALLOW_PATHS = [l.strip() for l in open(af) if l.strip() and not l.startswith("#")]
TOK = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]+|[A-Za-z]+")
PERSONAL_MAIL = re.compile(r"[\w.+-]+@(?:gmail|googlemail|yahoo|hotmail|outlook|live|icloud|aol|proton|protonmail|pm)\.(?:com|me|net)\b", re.I)
# `/home/name`, `/Users/name`, and Windows `C:\Users\name` (escaped or not).
HOME = re.compile(r"[/\\](?:home|Users)[/\\]+([A-Za-z_][A-Za-z0-9_-]*)")
WORD = re.compile(r"[A-Za-z]+")
sha = lambda s: hashlib.sha256(s.encode()).hexdigest()
def blocked(text):
    # Whole email addresses, and every word, the ones inside an address too.
    for t in TOK.findall(text) + WORD.findall(text):
        if sha(t.lower()) in LOW or sha(t) in EXACT:
            return "blocked personal word"
    if PERSONAL_MAIL.search(text): return "personal email address"
    for u in HOME.findall(text):
        if sha(u.lower()) in HOMEU or sha(u.lower()) in LOW: return "real home-folder path"
    return None
def allowed(path):
    import fnmatch
    return any(fnmatch.fnmatch(path, g) for g in ALLOW_PATHS)
def git(*args):
    r = subprocess.run(["git", *args], capture_output=True, text=True)
    if r.returncode != 0:
        # Fail closed: a git error must not read as a clean run.
        print(f"pii-guard: couldn't read the commits or files (git {args[0]} failed)")
        sys.exit(2)
    return r.stdout
def commit_range():
    if len(sys.argv) > 1:
        return [f"{sys.argv[1]}..HEAD"]
    try:
        before = json.load(open(os.environ["GITHUB_EVENT_PATH"])).get("before") or ""
    except (KeyError, OSError, ValueError, AttributeError):
        before = ""
    known = re.fullmatch(r"[0-9a-f]{40}", before) and before.strip("0") and subprocess.run(
        ["git", "cat-file", "-e", f"{before}^{{commit}}"], capture_output=True).returncode == 0
    return [f"{before}..HEAD"] if known else ["-1", "HEAD"]
bad = []
files = git("ls-files", "-z").split("\0")
me = os.path.relpath(os.path.join(HERE), ".")
for f in filter(None, files):
    if allowed(f) or f.startswith(me + "/pii-") or not os.path.isfile(f): continue
    why = blocked(f)
    if why: bad.append(f"{f}: {why} in the file name")
    try: data = open(f, "rb").read()
    except OSError: continue
    if b"\0" in data[:8000]: continue
    for n, line in enumerate(data.decode("utf-8", "ignore").splitlines(), 1):
        why = blocked(line)
        if why: bad.append(f"{f}:{n}: {why}")
log = git("log", *commit_range(), "--format=%H%x00%an%x00%ae%x00%cn%x00%ce%x00%B%x01")
for rec in filter(str.strip, log.split("\x01")):
    h, an, ae, cn, ce, body = (rec.strip("\n").split("\x00") + [""] * 6)[:6]
    for label, val in (("author", f"{an} <{ae}>"), ("committer", f"{cn} <{ce}>"), ("message", body)):
        why = blocked(val)
        if why: bad.append(f"commit {h[:8]} {label}: {why}")
if bad:
    print("Personal info found (details hidden on purpose):"); print("\n".join(bad[:200]))
    print("Fix: use 'the user' instead of real names, the GitHub no-reply email, and /home/user in paths.")
    sys.exit(1)
print("pii-guard: clean")
