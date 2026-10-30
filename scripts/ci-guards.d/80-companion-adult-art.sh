# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 8. No adult companion art in the repo --------------------------------------
# Adults-only companion outfits load only from `{config}/companions/<id>/adult/`
# on the user's PC (crates/grokhub-core/assets/companions/README.md). A tracked
# file under any `companions/**/adult/` path, or a shipped companion.toml that
# marks an outfit `adult = true`, fails here.
adult_rc=0
adult_paths=$(git ls-files -- '*companions/*/adult/*' 'companions/*/adult/*' 2>/dev/null) || adult_rc=$?
if (( adult_rc != 0 )); then
  echo "ci-guards: git ls-files for companion adult paths errored (rc=$adult_rc)" >&2
  fail=1
elif [[ -n "$adult_paths" ]]; then
  echo "ci-guards: adult companion art is tracked in the repo (it belongs only in the user's config folder):" >&2
  echo "$adult_paths" >&2
  fail=1
fi
adult_flags=""
shopt -s nullglob globstar
for toml in crates/grokhub-core/assets/companions/*/companion.toml; do
  if grep -nE '^[[:space:]]*adult[[:space:]]*=[[:space:]]*true' "$toml" >/dev/null; then
    adult_flags+="$toml"$'\n'
  fi
done
shopt -u nullglob globstar
if [[ -n "$adult_flags" ]]; then
  echo "ci-guards: a shipped companion.toml marks an outfit adult = true:" >&2
  printf '%s' "$adult_flags" >&2
  fail=1
fi
if [[ -z "$adult_paths" && -z "$adult_flags" ]]; then
  echo "ci-guards: no adult companion art in the repo"
fi
