#!/usr/bin/env python3
"""Personal-info guard. Fails if tracked files or new commits contain the owner's
personal details. Blocked words are stored only as SHA-256 hashes so this file
never leaks them. Usage: pii_guard.py [BASE_REF]  (BASE_REF also checks commit metadata)."""
import hashlib, os, re, subprocess, sys
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
HOME = re.compile(r"/(?:home|Users)/([A-Za-z_][A-Za-z0-9_-]*)")
sha = lambda s: hashlib.sha256(s.encode()).hexdigest()
def blocked(text):
    for t in TOK.findall(text):
        if sha(t.lower()) in LOW or sha(t) in EXACT:
            return "blocked personal word"
    if PERSONAL_MAIL.search(text): return "personal email address"
    for u in HOME.findall(text):
        if sha(u.lower()) in HOMEU or sha(u.lower()) in LOW: return "real home-folder path"
    return None
def allowed(path):
    import fnmatch
    return any(fnmatch.fnmatch(path, g) for g in ALLOW_PATHS)
bad = []
files = subprocess.run(["git", "ls-files", "-z"], capture_output=True, text=True).stdout.split("\0")
me = os.path.relpath(os.path.join(HERE), ".")
for f in filter(None, files):
    if allowed(f) or f.startswith(me + "/pii-") or not os.path.isfile(f): continue
    try: data = open(f, "rb").read()
    except OSError: continue
    if b"\0" in data[:8000]: continue
    for n, line in enumerate(data.decode("utf-8", "ignore").splitlines(), 1):
        why = blocked(line)
        if why: bad.append(f"{f}:{n}: {why}")
if len(sys.argv) > 1:
    log = subprocess.run(["git", "log", f"{sys.argv[1]}..HEAD", "--format=%H%x00%an%x00%ae%x00%cn%x00%ce%x00%B%x01"], capture_output=True, text=True).stdout
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
