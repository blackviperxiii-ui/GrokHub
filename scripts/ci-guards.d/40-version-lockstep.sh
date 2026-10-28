# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 4. Version lockstep (rearch stage 2) --------------------------------------
if cargo xtask bump --check; then
  echo "ci-guards: xtask bump --check ok"
else
  echo "ci-guards: xtask bump --check failed (VERSION copies disagree)" >&2
  fail=1
fi
