# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 5. linux and windows fail when the changes job fails ----------------------
# Both are required checks, and GitHub counts a skipped job as passed. A job
# that only tests `needs.changes.outputs.*` is skipped when `changes` fails or
# times out, so the pull request could merge with nothing built. Each job must
# still run in that case (`!cancelled()` plus `needs.changes.result`) and fail
# in a step of its own.
gate_yml=.github/workflows/ci.yml
if [[ ! -f "$gate_yml" ]]; then
  echo "ci-guards: missing $gate_yml" >&2
  fail=1
else
  gate_bad=$(awk '
    function close_job() {
      if (job != "linux" && job != "windows") return
      seen[job] = 1
      if (!job_if)
        printf "%s: the job-level if: needs both !cancelled() and needs.changes.result != \047success\047 on one line\n", job
      if (!fail_step)
        printf "%s: needs a step with if: needs.changes.result != \047success\047 and run: exit 1\n", job
    }
    /^[^[:space:]#]/ { close_job(); job = ""; in_jobs = ($0 ~ /^jobs:/) }
    in_jobs && /^  [A-Za-z0-9_-]+:[[:space:]]*$/ {
      close_job()
      job = $1; sub(/:$/, "", job)
      job_if = 0; fail_step = 0; step_if = 0
      next
    }
    job == "" { next }
    /^    if:/ {
      if (index($0, "!cancelled()") && $0 ~ /needs\.changes\.result != \047success\047/) job_if = 1
      next
    }
    {
      if (step_if == NR - 1 && $0 ~ /^[[:space:]]+run: exit 1[[:space:]]*$/) fail_step = 1
      if ($0 ~ /^[[:space:]]+(- )?if: needs\.changes\.result != \047success\047[[:space:]]*$/) step_if = NR
    }
    END {
      close_job()
      if (!seen["linux"]) print "linux: job not found"
      if (!seen["windows"]) print "windows: job not found"
    }
  ' "$gate_yml")
  if [[ -n "$gate_bad" ]]; then
    echo "ci-guards: $gate_yml lets a required check skip when the changes job fails:" >&2
    echo "$gate_bad" >&2
    fail=1
  else
    echo "ci-guards: linux and windows fail when the changes job fails"
  fi
fi
