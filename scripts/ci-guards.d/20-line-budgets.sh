# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 2. Line-count ratchet -----------------------------------------------------
# Budgets are intentional ceilings. Shrink freely; raise only in a PR that
# explains the growth (or after a split that moves lines out).
budget() {
  local path="$1" max="$2"
  local lines
  if [[ ! -f "$path" ]]; then
    echo "ci-guards: missing budgeted file $path" >&2
    fail=1
    return
  fi
  lines=$(wc -l <"$path" | tr -d ' ')
  if (( lines > max )); then
    echo "ci-guards: $path has $lines lines (budget $max). Split or raise the budget with a reason." >&2
    fail=1
  else
    echo "ci-guards: $path $lines / $max"
  fi
}

budget "crates/grokhub-app/src/app/mod.rs" 5950 # Nav::Home + home_layout (Order -38) + web_search_ui (#974)
budget "crates/grokhub-app/src/cards.rs" 4400
budget "crates/grokhub-core/src/slash/mod.rs" 1650
budget "crates/grokhub-app/src/app/tests.rs" 2800
