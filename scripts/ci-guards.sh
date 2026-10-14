#!/usr/bin/env bash
# Repo friction guards (rearch stage 1). Fail CI when:
#   1) conflict markers are committed
#   2) god-file line budgets are exceeded (ratchet; raise only when intentional)
#   3) a PR edits Cargo.lock without a Cargo.toml dep edit or the dependencies label
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0

# --- 1. Conflict markers (study: ^<<<<<<< / ^>>>>>>>) ---------------------------
markers=$(git grep -nE '^(<<<<<<<|>>>>>>>)' -- . ':!.git/**' 2>/dev/null || true)
if [[ -n "$markers" ]]; then
  echo "ci-guards: conflict markers found:" >&2
  echo "$markers" >&2
  fail=1
else
  echo "ci-guards: no conflict markers"
fi

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

budget "crates/grokhub-app/src/app/mod.rs" 5900
budget "crates/grokhub-app/src/cards.rs" 4400
budget "crates/grokhub-core/src/slash.rs" 1900
budget "crates/grokhub-app/src/app/tests.rs" 29500

# --- 3. Cargo.lock needs dependencies label (pull_request only) ----------------
if [[ "${GITHUB_EVENT_NAME:-}" == "pull_request" ]]; then
  base_ref="${GITHUB_BASE_REF:-beta}"
  git fetch --no-tags --depth=100 origin "$base_ref" >/dev/null 2>&1 || true
  merge_base=$(git merge-base "origin/${base_ref}" HEAD 2>/dev/null || true)
  if [[ -z "$merge_base" ]]; then
    echo "ci-guards: warn: could not resolve merge-base against origin/${base_ref}; skipping Cargo.lock rule" >&2
  else
    mapfile -t changed < <(git diff --name-only "$merge_base"...HEAD)
    lock_changed=0
    toml_paths=()
    for f in "${changed[@]+"${changed[@]}"}"; do
      case "$f" in
        Cargo.lock) lock_changed=1 ;;
        Cargo.toml|*/Cargo.toml) toml_paths+=("$f") ;;
      esac
    done
    if (( lock_changed == 1 )); then
      toml_dep=0
      if ((${#toml_paths[@]} > 0)); then
        # Any non-version-only Cargo.toml hunk counts as a dep/manifest edit.
        if git diff -U0 "$merge_base"...HEAD -- "${toml_paths[@]}" \
          | grep -E '^\+[^+]|^-[^-]' \
          | grep -vE '^\+\+\+|^\-\-\-' \
          | grep -vE '^[+-][[:space:]]*version[[:space:]]*=' \
          | grep -q .; then
          toml_dep=1
        fi
      fi
      has_label=0
      # Prefer live PR labels (API/gh). The pull_request event payload is a
      # snapshot: a workflow re-run after adding `dependencies` still sees the
      # old labels unless we refetch. labeled/unlabeled triggers also re-run CI.
      labels_csv="${GITHUB_PR_LABELS:-}"
      if [[ -z "$labels_csv" ]]; then
        pr_number=""
        if [[ -n "${GITHUB_EVENT_PATH:-}" && -f "${GITHUB_EVENT_PATH}" ]]; then
          pr_number=$(python3 -c 'import json,os; e=json.load(open(os.environ["GITHUB_EVENT_PATH"])); print(e.get("pull_request",{}).get("number") or e.get("number") or "")')
        fi
        if [[ -n "$pr_number" && -n "${GITHUB_REPOSITORY:-}" ]]; then
          if command -v gh >/dev/null 2>&1; then
            labels_csv=$(GH_TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-}}" gh api "repos/${GITHUB_REPOSITORY}/pulls/${pr_number}" --jq '[.labels[].name]|join(",")' 2>/dev/null || true)
          elif [[ -n "${GITHUB_TOKEN:-}" ]]; then
            labels_csv=$(python3 -c "
import json,os,urllib.request
req=urllib.request.Request(
    f\"https://api.github.com/repos/{os.environ['GITHUB_REPOSITORY']}/pulls/{os.environ['PR_NUMBER']}\",
    headers={'Authorization': f\"Bearer {os.environ['GITHUB_TOKEN']}\", 'Accept': 'application/vnd.github+json', 'User-Agent': 'ci-guards'})
print(','.join(l['name'] for l in json.load(urllib.request.urlopen(req)).get('labels',[])))
" PR_NUMBER="$pr_number" 2>/dev/null || true)
          fi
        fi
      fi
      if [[ -z "$labels_csv" && -n "${GITHUB_EVENT_PATH:-}" && -f "${GITHUB_EVENT_PATH}" ]]; then
        labels_csv=$(python3 -c 'import json,os; e=json.load(open(os.environ["GITHUB_EVENT_PATH"])); print(",".join(l["name"] for l in e.get("pull_request",{}).get("labels",[])))')
      fi
      echo "ci-guards: PR labels=[${labels_csv}]"
      case ",${labels_csv}," in
        *,dependencies,*|*,deps,*) has_label=1 ;;
      esac
      if (( toml_dep == 0 && has_label == 0 )); then
        echo "ci-guards: Cargo.lock changed without a Cargo.toml dependency edit or the 'dependencies' label." >&2
        echo "ci-guards: add label 'dependencies' for a lock-only bump, or include the Cargo.toml dep change." >&2
        fail=1
      else
        echo "ci-guards: Cargo.lock change allowed (toml_dep=$toml_dep label=$has_label)"
      fi
    else
      echo "ci-guards: Cargo.lock unchanged"
    fi
  fi
else
  echo "ci-guards: skip Cargo.lock label rule (event=${GITHUB_EVENT_NAME:-local})"
fi

if (( fail != 0 )); then
  exit 1
fi
echo "ci-guards: ok"
