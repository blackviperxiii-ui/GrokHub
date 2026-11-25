#!/usr/bin/env python3
"""Pick which workspace packages a pull request's tests must cover.

Prints cargo package args for nextest and doctests: `--workspace`, or one
`-p NAME` per package. A package is picked when a file under its directory
changed, and so is every workspace package that depends on it (directly or
through others, dev-dependencies included). Anything the script can't place
in one package runs the whole workspace: Cargo files, CI, scripts, assets,
data, packaging, ui, src-tauri, and any path it doesn't know. Only docs,
screenshots, research and top-level Markdown (except PRIVACY.md) are
ignored; the `changes` job already skips the build for those PRs.

Usage: affected_crates.py METADATA_JSON < changed-paths
"""
import json
import sys

WORKSPACE = "--workspace"
# Read from disk or compiled in by tests (see ci.yml `embedded`).
FULL_DOCS = ("docs/compass/", "docs/eval/")
IGNORED_PREFIXES = ("docs/", "screenshots/", "research/")


def packages(metadata):
    root = metadata["workspace_root"].rstrip("/") + "/"
    members = set(metadata["workspace_members"])
    pkgs = {}
    for p in metadata["packages"]:
        if p["id"] not in members:
            continue
        manifest = p["manifest_path"]
        if not manifest.startswith(root):
            return None
        rel_dir = manifest[len(root):].rsplit("/", 1)[0]
        if not rel_dir or rel_dir == manifest[len(root):]:
            # A package at the workspace root owns every path; no narrowing.
            return None
        deps = {d["name"] for d in p["dependencies"]}
        pkgs[p["name"]] = (rel_dir + "/", deps)
    return pkgs


def select(metadata, paths):
    pkgs = packages(metadata)
    if not pkgs:
        return [WORKSPACE]
    hit = set()
    for path in paths:
        path = path.strip()
        if not path:
            continue
        if path.startswith(FULL_DOCS):
            return [WORKSPACE]
        if path.startswith(IGNORED_PREFIXES):
            continue
        if "/" not in path and path.endswith(".md") and path != "PRIVACY.md":
            continue
        owner = [n for n, (d, _) in pkgs.items() if path.startswith(d)]
        if len(owner) != 1:
            return [WORKSPACE]
        name = owner[0]
        # A manifest change can change features or deps for its dependents.
        if path == pkgs[name][0] + "Cargo.toml":
            return [WORKSPACE]
        hit.add(name)
    if not hit:
        return [WORKSPACE]
    # Close over reverse dependencies inside the workspace.
    changed = True
    while changed:
        changed = False
        for name, (_, deps) in pkgs.items():
            if name not in hit and deps & hit:
                hit.add(name)
                changed = True
    if hit == set(pkgs):
        return [WORKSPACE]
    args = []
    for name in sorted(hit):
        args += ["-p", name]
    return args


def main():
    with open(sys.argv[1]) as f:
        metadata = json.load(f)
    print(" ".join(select(metadata, sys.stdin.read().splitlines())))


if __name__ == "__main__":
    main()
