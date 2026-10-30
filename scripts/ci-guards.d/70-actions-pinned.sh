# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 7. Actions pinned to commit SHAs ------------------------------------------
# A moved tag would run new code with the job's token (and, in release.yml,
# next to the signing secrets). Pin `owner/repo@<40-hex sha> # <tag>`;
# Dependabot (.github/dependabot.yml) bumps the pins. Local `./` actions are
# fine; `docker://` images need an `@sha256:` digest. Workflows (.yml/.yaml)
# and composite actions (.github/actions/*/action.yml) are both scanned.
shopt -s nullglob
workflows=(.github/workflows/*.yml .github/workflows/*.yaml)
composites=(.github/actions/*/action.yml .github/actions/*/action.yaml)
shopt -u nullglob
if ((${#workflows[@]} == 0)); then
  echo "ci-guards: no workflows found in .github/workflows/" >&2
  fail=1
else
  # grep exits 1 for no matches (fine) and 2 for a read error (fail closed).
  rc=0
  uses=$(grep -nHE '^[[:space:]]*(- )?uses:' "${workflows[@]}" "${composites[@]}") || rc=$?
  if ((rc > 1)); then
    echo "ci-guards: could not read workflow files (grep exit $rc)" >&2
    fail=1
  else
    unpinned=$(printf '%s\n' "$uses" | grep -vE \
      '^[^:]*:[0-9]+:[[:space:]]*(- )?uses:[[:space:]]*["'\'']?(\./[^[:space:]"'\'']*|docker://[^@[:space:]]+@sha256:[0-9a-f]{64}|[^@[:space:]"'\'']+@[0-9a-f]{40})["'\'']?([[:space:]]|$)' \
      | grep -v '^$' || true)
    if [[ -n "$unpinned" ]]; then
      echo "ci-guards: actions not pinned to a full commit SHA:" >&2
      echo "$unpinned" >&2
      fail=1
    else
      echo "ci-guards: scanned ${#workflows[@]} workflows + ${#composites[@]} composite actions, every action is pinned to a commit SHA"
    fi
  fi
fi
