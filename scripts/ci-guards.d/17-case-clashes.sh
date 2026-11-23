# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- Names that differ only by letter case ------------------------------------
# Windows and macOS disks ignore letter case, so on those systems:
#   - two names in one folder that differ only by case are the same entry,
#     whether each is a file, a folder or a link, and a checkout keeps just one;
#   - a JS/TS import picks the first file that matches its name in any case.
#     `import "./ChatFind"` loaded chatFind.ts instead of ChatFind.tsx, and the
#     Windows release build failed (TS1149) while Linux CI stayed green.
#     `import "./games"` also means games/index.tsx, so a Games.tsx beside that
#     folder clashes the same way.
# Fails on any such pair among the tracked files. Names are compared by
# uppercasing one letter at a time, close to how Windows compares them, so É
# and é, or ς and σ, match too, while ß and ss stay apart. mawk's tolower()
# would fold ASCII letters only.
case_clashes=$(python3 - <<'PY'
import re
import subprocess
import sys

# check=True: a git failure stops the guard instead of passing it.
listing = subprocess.run(["git", "ls-files", "-z"], check=True, stdout=subprocess.PIPE).stdout
paths = [p for p in listing.decode("utf-8", "surrogateescape").split("\0") if p]

# An import names a module without its extension.
MODULE_EXT = re.compile(r"\.(?:d\.(?:ts|mts|cts)|tsx|ts|jsx|js|mjs|cjs|mts|cts)$")
first = {}
told = set()
out = []


def fold(name):
    """Uppercase each letter that has a one-letter uppercase form (not ß)."""
    return "".join(c.upper() if len(c.upper()) == 1 else c for c in name)


def note(kind, name, shown):
    """Report a name whose folded key was first seen spelled another way."""
    key = (kind, fold(name))
    seen_name, seen_shown = first.setdefault(key, (name, shown))
    if seen_name != name and (key, name) not in told:
        told.add((key, name))
        out.append(f"  {kind}: {seen_shown} and {shown}")


for path in paths:
    parts = path.split("/")
    # Each folder on the way down, then the file itself, in one namespace:
    # a disk that ignores case cannot hold docs/Guide and docs/guide/ either.
    for depth in range(1, len(parts)):
        folder = "/".join(parts[:depth])
        note("path", folder, folder + "/")
    note("path", path, path)
    match = MODULE_EXT.search(path)
    if match:
        module = path[: match.start()]
        note("module", module, path)
        # `import "./games"` resolves to games/index.* when no games.* exists.
        if module.endswith("/index"):
            note("module", module[: -len("/index")], path)

sys.stdout.buffer.write("\n".join(out).encode("utf-8", "surrogateescape"))
PY
)
if [[ -n "$case_clashes" ]]; then
  echo "ci-guards: these names differ only by letter case, which breaks Windows and macOS builds (rename one of each pair):" >&2
  echo "$case_clashes" >&2
  fail=1
else
  echo "ci-guards: no names that differ only by letter case"
fi
