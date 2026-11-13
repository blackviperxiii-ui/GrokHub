# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 7c. A release tag is checked against VERSION before anything builds --------
# release.yml's plan job decides whether a tag builds. The update manifest
# takes its version from the tag and the binaries take theirs from VERSION, so
# plan runs scripts/release-tag-check.sh first. scripts/test-release-tag-check.sh
# tests that script; this guard pins the call. In the plan job, the first
# command of the tag branch must be the check, with `|| exit 1`, and nothing
# before the tag branch may write build=true.
tagcheck_yml=.github/workflows/release.yml
if [[ ! -f "$tagcheck_yml" ]]; then
  echo "ci-guards: missing $tagcheck_yml" >&2
  fail=1
else
  tagcheck_bad=$(awk '
    /^[^[:space:]#]/ { in_jobs = ($0 ~ /^jobs:/); job = "" }
    in_jobs && /^  [A-Za-z0-9_-]+:[[:space:]]*$/ { job = $1; sub(/:$/, "", job); next }
    job != "plan" { next }
    { seen_plan = 1; line = $0; gsub(/^[[:space:]]+|[[:space:]]+$/, "", line) }
    line == "" || line ~ /^#/ { next }
    want_check {
      want_check = 0
      if (line != "bash scripts/release-tag-check.sh \"$GITHUB_REF_NAME\" || exit 1")
        printf "line %d: the first command in the tag branch must be: bash scripts/release-tag-check.sh \"$GITHUB_REF_NAME\" || exit 1\n", NR
      next
    }
    line == "if [ \"$GITHUB_REF_TYPE\" = tag ]; then" { branches++; want_check = 1; next }
    !branches && index(line, "build=true") {
      printf "line %d: build=true is written before the tag branch\n", NR
    }
    END {
      if (!seen_plan) print "plan: job not found"
      else if (!branches) print "plan: no tag branch (if [ \"$GITHUB_REF_TYPE\" = tag ]; then)"
      if (want_check) print "plan: the tag branch is empty"
    }
  ' "$tagcheck_yml")
  if [[ -n "$tagcheck_bad" ]]; then
    echo "ci-guards: $tagcheck_yml could build a tag that scripts/release-tag-check.sh has not passed:" >&2
    echo "$tagcheck_bad" >&2
    fail=1
  else
    echo "ci-guards: release.yml checks a tag against VERSION before it builds"
  fi
fi
