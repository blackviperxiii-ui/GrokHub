# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 7. Actions pinned to commit SHAs ------------------------------------------
# A moved tag would run new code with the job's token (and, in release.yml,
# next to the signing secrets). Pin `owner/repo@<40-hex sha> # <tag>`;
# Dependabot (.github/dependabot.yml) bumps the pins. Local `./` actions are fine.
unpinned=$(grep -nE '^[[:space:]]*(- )?uses:' .github/workflows/*.yml \
  | grep -vE 'uses:[[:space:]]*(\./|[^@[:space:]]+@[0-9a-f]{40}([[:space:]]|$))' || true)
if [[ -n "$unpinned" ]]; then
  echo "ci-guards: actions not pinned to a full commit SHA:" >&2
  echo "$unpinned" >&2
  fail=1
else
  echo "ci-guards: every workflow action is pinned to a commit SHA"
fi
