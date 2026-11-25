# Affected-packages picker: Cargo example

Goal: on a PR, print the `cargo` package arguments for everything the diff reaches, and fall back to `--workspace` whenever in doubt.

```python
#!/usr/bin/env python3
"""Print '--workspace' or '-p a -p b' for the packages a diff reaches.

argv[1]: path to `cargo metadata --no-deps --format-version 1 --locked` JSON.
stdin:   changed paths, one per line (git diff --name-only HEAD^1 HEAD).
"""
import json
import sys
from pathlib import PurePosixPath

IGNORED_PREFIXES = ("docs/", "screenshots/")  # only if no test reads them


def main() -> None:
    meta = json.load(open(sys.argv[1]))
    root = PurePosixPath(meta["workspace_root"])
    members = set(meta["workspace_members"])
    pkgs = {p["name"]: p for p in meta["packages"] if p["id"] in members}
    dirs = {
        name: PurePosixPath(p["manifest_path"]).parent.relative_to(root)
        for name, p in pkgs.items()
    }
    if any(str(d) == "." for d in dirs.values()):
        print("--workspace")  # root package: everything is under it
        return

    rdeps = {name: set() for name in pkgs}
    for name, p in pkgs.items():
        for dep in p["dependencies"]:
            if dep["name"] in pkgs:
                rdeps[dep["name"]].add(name)

    paths = [l.strip() for l in sys.stdin if l.strip()]
    if not paths:
        print("--workspace")
        return

    hit = set()
    for path in paths:
        if path.startswith(IGNORED_PREFIXES) or (
            "/" not in path and path.endswith(".md")
        ):
            continue
        owners = [n for n, d in dirs.items() if PurePosixPath(path).is_relative_to(d)]
        if len(owners) != 1 or path.endswith("Cargo.toml"):
            print("--workspace")  # shared file, lockfile, CI, or a manifest
            return
        hit.add(owners[0])

    todo = list(hit)
    while todo:
        for r in rdeps[todo.pop()]:
            if r not in hit:
                hit.add(r)
                todo.append(r)

    if not hit or hit == set(pkgs):
        print("--workspace")
        return
    print(" ".join(f"-p {n}" for n in sorted(hit)))


if __name__ == "__main__":
    main()
```

When nothing but ignored docs changed, the example still prints `--workspace`; choose a cheaper path only when the job's other steps (lint, build) still run.

Workflow wiring:

```yaml
      - uses: actions/checkout@v4
        with:
          fetch-depth: ${{ github.event_name == 'pull_request' && 2 || 1 }}
      - id: pick
        run: |
          if [ "${{ github.event_name }}" = pull_request ]; then
            cargo metadata --no-deps --format-version 1 --locked > /tmp/meta.json
            pkgs=$(git diff --name-only HEAD^1 HEAD | python3 .github/scripts/affected_packages.py /tmp/meta.json)
          else
            pkgs=--workspace
          fi
          echo "pkgs=$pkgs" >> "$GITHUB_OUTPUT"
      - run: python3 -m unittest .github/scripts/test_affected_packages.py
      - run: cargo nextest run ${{ steps.pick.outputs.pkgs }} --locked
      - run: cargo test --doc ${{ steps.pick.outputs.pkgs }} --locked
```

Tests to write for the picker, each asserting the literal output:

- A leaf package's source file prints only that package.
- A core package's source file prints it plus every reverse dependency.
- `Cargo.lock`, the root `Cargo.toml`, any crate's `Cargo.toml`, and `.github/` files print `--workspace`.
- A path outside every member directory prints `--workspace`.
- An empty diff prints `--workspace`.
- A diff that reaches every package prints `--workspace`.
