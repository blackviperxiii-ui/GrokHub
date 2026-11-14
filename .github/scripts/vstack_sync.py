#!/usr/bin/env python3
"""Keep Loadout's built-in vstack skills in step with vstack releases (stdlib only).

    python3 .github/scripts/vstack_sync.py newer
    python3 .github/scripts/vstack_sync.py sync --tag v1.3.0 [--src PATH] [--summary FILE]
    python3 .github/scripts/vstack_sync.py check

`newer` prints the newest vX.Y.Z tag upstream when it is newer than the pinned
tag, and nothing otherwise. `sync` reads one tag from a vstack git repository
(`--src`, or a fresh shallow clone), rewrites crates/grokhub-agent/assets/skills/
from it, and regenerates CREDITS.md and crates/grokhub-agent/src/builtin_skills.rs.
The same tag always gives the same files. `check` needs no network: it fails
when the bundle, CREDITS.md or builtin_skills.rs no longer match vstack.json.

vstack.json is the one list. `skills` are bundled, `always_on` (a subset) ride
in full in every Code tab session, and `exclude` names the skills left out with
the reason. A new upstream skill that is in neither list joins `skills`; a skill
gone upstream leaves it.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
AGENT = os.path.join(ROOT, "crates", "grokhub-agent")
ASSETS = os.path.join(AGENT, "assets", "skills")
MANIFEST = os.path.join(ASSETS, "vstack.json")
CREDITS = os.path.join(ASSETS, "CREDITS.md")
LICENSE = os.path.join(ASSETS, "LICENSE-upstream")
TABLE = os.path.join(AGENT, "src", "builtin_skills.rs")
# Files in assets/skills/ that are not a skill folder.
OWN_FILES = {"vstack.json", "CREDITS.md", "LICENSE-upstream"}

KEYS = ["about", "repo", "tag", "commit", "always_on", "skills", "exclude"]
TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
NAME = re.compile(r"^[a-z0-9][a-z0-9-]*$")
PART = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
# The skill tool clips a file at 48 KiB (skills.rs BODY_CAP); a bundled file
# must arrive whole.
FILE_CAP = 48 * 1024
# rustfmt keeps a tuple on one line while its items fit this width
# (fn_call_width), and wraps a line past max_width.
CALL_WIDTH = 60
MAX_WIDTH = 100


class SyncError(Exception):
    pass


def tag_key(tag):
    m = TAG.match(tag)
    if not m:
        raise SyncError(f"`{tag}` is not a vX.Y.Z tag")
    return tuple(int(p) for p in m.groups())


def validate(doc):
    if not isinstance(doc, dict) or list(doc) != KEYS:
        raise SyncError(f"vstack.json keys must be, in order: {', '.join(KEYS)}")
    for key in ("about", "repo", "tag", "commit"):
        if not isinstance(doc[key], str) or not doc[key]:
            raise SyncError(f"vstack.json `{key}` must be a non-empty string")
    tag_key(doc["tag"])
    if not COMMIT.match(doc["commit"]):
        raise SyncError("vstack.json `commit` must be a full 40-character commit")
    skills, always, exclude = doc["skills"], doc["always_on"], doc["exclude"]
    if not isinstance(skills, list) or not all(isinstance(s, str) and NAME.match(s) for s in skills):
        raise SyncError("vstack.json `skills` must list skill folder names")
    if skills != sorted(set(skills)):
        raise SyncError("vstack.json `skills` must be sorted with no repeats")
    if not isinstance(always, list) or len(set(always)) != len(always):
        raise SyncError("vstack.json `always_on` must list each skill once")
    missing = [s for s in always if s not in skills]
    if missing:
        raise SyncError(f"always-on skills not in `skills`: {', '.join(missing)}")
    if not isinstance(exclude, dict) or not all(
        k and "/" not in k and isinstance(r, str) and r.strip() for k, r in exclude.items()
    ):
        raise SyncError("vstack.json `exclude` must map each skill to a reason")
    if list(exclude) != sorted(exclude):
        raise SyncError("vstack.json `exclude` must be sorted by name")
    both = sorted(set(skills) & set(exclude))
    if both:
        raise SyncError(f"skills both bundled and excluded: {', '.join(both)}")
    return doc


def load():
    with open(MANIFEST, encoding="utf-8") as fh:
        return validate(json.load(fh))


def emit_manifest(doc):
    return json.dumps({k: doc[k] for k in KEYS}, indent=2, ensure_ascii=False) + "\n"


def git(src, *args, text=True):
    out = subprocess.run(
        ["git", "-C", src, *args], check=True, capture_output=True, text=text
    )
    return out.stdout


def upstream_files(src, tag):
    """{skill: {relpath: bytes or None}} for every skill folder at `tag`. A
    symlink or submodule is None, so a bundled skill that has one is refused."""
    rows = git(src, "ls-tree", "-r", "-z", tag, "--", "skills/")
    out = {}
    for row in filter(None, rows.split("\0")):
        meta, path = row.split("\t", 1)
        mode, kind, _ = meta.split(" ")
        parts = path.split("/")
        if len(parts) < 3 or parts[0] != "skills":
            continue
        skill, rel = parts[1], "/".join(parts[2:])
        plain = kind == "blob" and mode in ("100644", "100755")
        data = git(src, "cat-file", "blob", f"{tag}:{path}", text=False) if plain else None
        out.setdefault(skill, {})[rel] = data
    return out


def check_file(skill, rel, data):
    if data is None:
        raise SyncError(f"{skill}/{rel} is a symlink or submodule; exclude {skill} in vstack.json")
    if not all(PART.match(p) for p in rel.split("/")):
        raise SyncError(f"{skill}/{rel}: unexpected characters in the path")
    if b"\0" in data:
        raise SyncError(f"{skill}/{rel} is binary; exclude {skill} in vstack.json")
    try:
        data.decode("utf-8")
    except UnicodeDecodeError:
        raise SyncError(f"{skill}/{rel} is not UTF-8; exclude {skill} in vstack.json")
    if len(data) > FILE_CAP:
        raise SyncError(
            f"{skill}/{rel} is {len(data)} bytes, over the skill tool's {FILE_CAP}; "
            f"exclude {skill} in vstack.json"
        )
    if b"\r" in data:
        raise SyncError(f"{skill}/{rel} has CR line endings")


def plan(doc, upstream):
    """The next manifest and the skill changes, for the upstream skill set."""
    have = set(upstream)
    for name in have - set(doc["exclude"]):
        if not NAME.match(name):
            raise SyncError(
                f"upstream skill folder `{name}` is not a plain name; exclude it in vstack.json"
            )
    gone_on = [s for s in doc["always_on"] if s not in have]
    if gone_on:
        raise SyncError(
            f"always-on skills left vstack: {', '.join(gone_on)}; update `always_on` in vstack.json"
        )
    added = sorted(have - set(doc["skills"]) - set(doc["exclude"]))
    removed = sorted(set(doc["skills"]) - have)
    nxt = dict(doc)
    nxt["skills"] = sorted((set(doc["skills"]) & have) | set(added))
    nxt["exclude"] = {k: v for k, v in sorted(doc["exclude"].items()) if k in have}
    for name in nxt["skills"]:
        if "SKILL.md" not in upstream[name]:
            raise SyncError(f"upstream skill `{name}` has no SKILL.md")
    return nxt, added, removed


def disk_files():
    """{skill: [relpath]} for the skill folders in assets/skills/, sorted."""
    out = {}
    for name in sorted(os.listdir(ASSETS)):
        path = os.path.join(ASSETS, name)
        if name in OWN_FILES:
            continue
        if not os.path.isdir(path):
            raise SyncError(f"stray file assets/skills/{name}")
        rels = []
        for base, dirs, files in os.walk(path):
            dirs.sort()
            for f in sorted(files):
                rels.append(os.path.relpath(os.path.join(base, f), path).replace(os.sep, "/"))
        out[name] = sorted(rels)
    return out


def rust_str(text):
    return json.dumps(text)


def tuple_lines(items, indent):
    """One rustfmt-shaped tuple element of an array, at `indent` spaces."""
    pad = " " * indent
    inner = ", ".join(items)
    if len(inner) <= CALL_WIDTH:
        return [pad + "(" + inner + "),"]
    out = [pad + "("]
    for item in items:
        line = pad + "    " + item + ","
        call = re.match(r"^(include_str!\()(.*)\)$", item)
        if len(line) > MAX_WIDTH and call:
            out += [pad + "    " + call[1], pad + "        " + call[2], pad + "    ),"]
        else:
            out.append(line)
    return out + [pad + "),"]


def emit_table(doc, files):
    lines = [
        "//! Generated by `.github/scripts/vstack_sync.py` from",
        "//! `assets/skills/vstack.json`. Do not edit by hand: run",
        "//! `python3 .github/scripts/vstack_sync.py sync --tag vX.Y.Z`.",
        "",
        "/// Bundled vstack skills (MIT; see `assets/skills/CREDITS.md`): folder and",
        "/// `SKILL.md`.",
        "pub const BUILTIN_SKILL_BODIES: &[(&str, &str)] = &[",
    ]
    for name in doc["skills"]:
        inc = f'include_str!("../assets/skills/{name}/SKILL.md")'
        lines += tuple_lines([rust_str(name), inc], 4)
    lines += [
        "];",
        "",
        "/// The other files in a bundled skill's folder: folder, path inside it, text.",
        "pub const BUILTIN_SKILL_FILES: &[(&str, &str, &str)] = &[",
    ]
    for name in doc["skills"]:
        for rel in files.get(name, []):
            if rel == "SKILL.md":
                continue
            inc = f'include_str!("../assets/skills/{name}/{rel}")'
            lines += tuple_lines([rust_str(name), rust_str(rel), inc], 4)
    lines += [
        "];",
        "",
        "/// Bundled skills every Code tab session carries in full, in this order.",
        "pub const BUILTIN_ALWAYS_ON: &[&str] = &[",
    ]
    lines += [f"    {rust_str(name)}," for name in doc["always_on"]]
    lines += ["];", ""]
    return "\n".join(lines)


def digest(files):
    """SHA-256 over every bundled file and LICENSE-upstream, so a hand edit shows."""
    h = hashlib.sha256()
    for name in sorted(files):
        for rel in files[name]:
            with open(os.path.join(ASSETS, name, *rel.split("/")), "rb") as fh:
                h.update(f"{name}/{rel}".encode() + b"\0" + fh.read() + b"\0")
    if os.path.isfile(LICENSE):
        with open(LICENSE, "rb") as fh:
            h.update(b"LICENSE-upstream\0" + fh.read())
    return h.hexdigest()


def emit_credits(doc, files):
    url = doc["repo"].rstrip("/")
    skills = "\n".join(f"- {name}" for name in doc["skills"])
    left = "\n".join(f"- {name}: {why}" for name, why in doc["exclude"].items())
    return f"""# Built-in vstack skills: credit and license

Bundled from [vstack]({url}) release
[`{doc['tag']}`]({url}/releases/tag/{doc['tag']}) (commit `{doc['commit']}`).
`.github/scripts/vstack_sync.py` writes this file, the skill folders and
`../../src/builtin_skills.rs` from `vstack.json`; edit that list, not these files.

The method and several playbooks are adapted from the pstack plugin
(https://github.com/cursor/plugins/tree/main/pstack). Branding and naming in
the upstream repo are Viper Stack / vstack.

License: MIT. The upstream copyright and permission notice ships unchanged in
[LICENSE-upstream](LICENSE-upstream) in this folder and must stay with these files.

Skills shipped here (read-only built-ins inside Loadout / GrokHub):

{skills}

Left out:

{left}

Do not install these as a Cursor plugin from this tree; they are app built-ins.

Bundle SHA-256: `{digest(files)}`
"""


def write(path, text):
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)


def squash(text):
    """Rust source without whitespace or trailing commas, which rustfmt adds or
    drops as a line wraps. No generated literal holds a space or `,]`."""
    return re.sub(r",(?=[\])])", "", re.sub(r"\s+", "", text))


def rustfmt(path):
    """Format the generated table the way the CI fmt job does, when rustfmt is here."""
    exe = shutil.which("rustfmt")
    if exe is None:
        return
    cmd = [exe, "--edition", "2021"]
    config = os.path.join(ROOT, "rustfmt.toml")
    if os.path.isfile(config):
        cmd += ["--config-path", config]
    subprocess.run(cmd + [path], check=True)


def regenerate(doc):
    """The generated files' expected text, from the manifest and the folders on disk."""
    files = disk_files()
    return {
        MANIFEST: emit_manifest(doc),
        CREDITS: emit_credits(doc, files),
        TABLE: emit_table(doc, files),
    }


def check():
    doc = load()
    files = disk_files()
    problems = []
    if sorted(files) != doc["skills"]:
        extra = sorted(set(files) - set(doc["skills"]))
        missing = sorted(set(doc["skills"]) - set(files))
        problems.append(f"skill folders differ from vstack.json (extra {extra}, missing {missing})")
    for name, rels in files.items():
        if "SKILL.md" not in rels:
            problems.append(f"{name} has no SKILL.md")
        for rel in rels:
            with open(os.path.join(ASSETS, name, rel), "rb") as fh:
                try:
                    check_file(name, rel, fh.read())
                except SyncError as err:
                    problems.append(str(err))
    if not os.path.isfile(LICENSE):
        problems.append("LICENSE-upstream is missing")
    for path, want in regenerate(doc).items():
        with open(path, encoding="utf-8", newline="") as fh:
            have = fh.read()
        if path == TABLE:
            # rustfmt (the CI fmt job) owns the table's layout; compare tokens.
            have, want = squash(have), squash(want)
        if have != want:
            problems.append(f"{os.path.relpath(path, ROOT)} is stale")
    if problems:
        raise SyncError("; ".join(problems) + ". Run: vstack_sync.py sync --tag " + doc["tag"])
    print(f"vstack-sync: {len(files)} skills match vstack.json ({doc['tag']})")


def sync(tag, src, summary):
    doc = load()
    if tag_key(tag) < tag_key(doc["tag"]):
        raise SyncError(f"{tag} is older than the pinned {doc['tag']}; edit vstack.json to go back")
    tmp = None
    if src is None:
        tmp = tempfile.mkdtemp(prefix="vstack-")
        src = os.path.join(tmp, "vstack")
        subprocess.run(
            ["git", "clone", "--quiet", "--depth", "1", "--branch", tag, doc["repo"], src],
            check=True,
        )
    try:
        commit = git(src, "rev-parse", f"{tag}^{{commit}}").strip()
        upstream = upstream_files(src, tag)
        license_text = git(src, "cat-file", "blob", f"{tag}:LICENSE", text=False)
    finally:
        if tmp:
            shutil.rmtree(tmp, ignore_errors=True)
    nxt, added, removed = plan(doc, upstream)
    nxt["tag"], nxt["commit"] = tag, commit
    validate(nxt)
    for name in nxt["skills"]:
        for rel, data in upstream[name].items():
            check_file(name, rel, data)
    for name in os.listdir(ASSETS):
        if name not in OWN_FILES and not os.path.isdir(os.path.join(ASSETS, name)):
            raise SyncError(f"stray file assets/skills/{name}; move it before syncing")
    for name in os.listdir(ASSETS):
        path = os.path.join(ASSETS, name)
        if name not in OWN_FILES and os.path.isdir(path):
            shutil.rmtree(path)
    for name in nxt["skills"]:
        for rel, data in sorted(upstream[name].items()):
            path = os.path.join(ASSETS, name, *rel.split("/"))
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "wb") as fh:
                fh.write(data)
    with open(LICENSE, "wb") as fh:
        fh.write(license_text)
    for path, text in regenerate(nxt).items():
        write(path, text)
    rustfmt(TABLE)
    note = [
        f"Syncs the built-in vstack skills from `{doc['tag']}` to `{tag}` (commit `{commit[:12]}`).",
        "",
        f"- Bundled: {len(nxt['skills'])} skills, {len(nxt['always_on'])} always on in the Code tab.",
        f"- New upstream, now bundled: {', '.join(added) or 'none'}.",
        f"- Gone upstream, removed: {', '.join(removed) or 'none'}.",
        f"- Left out (see `vstack.json`): {', '.join(nxt['exclude']) or 'none'}.",
    ]
    if summary:
        write(summary, "\n".join(note) + "\n")
    print("\n".join(note))


def newer():
    doc = load()
    refs = subprocess.run(
        ["git", "ls-remote", "--tags", "--refs", doc["repo"]],
        check=True, capture_output=True, text=True,
    ).stdout
    tags = [line.split("refs/tags/", 1)[1] for line in refs.splitlines() if "refs/tags/" in line]
    tags = [t for t in tags if TAG.match(t)]
    if not tags:
        raise SyncError("no vX.Y.Z tags upstream")
    best = max(tags, key=tag_key)
    if tag_key(best) > tag_key(doc["tag"]):
        print(best)


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("newer")
    sub.add_parser("check")
    s = sub.add_parser("sync")
    s.add_argument("--tag", required=True)
    s.add_argument("--src", help="a vstack git repository that has the tag")
    s.add_argument("--summary", help="write the PR summary here")
    args = ap.parse_args(argv)
    try:
        if args.cmd == "newer":
            newer()
        elif args.cmd == "check":
            check()
        else:
            sync(args.tag, args.src, args.summary)
    except (SyncError, subprocess.CalledProcessError) as err:
        print(f"vstack-sync: {err}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
