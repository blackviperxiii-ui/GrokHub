#!/usr/bin/env bash
# Repo friction guards (rearch stages 1–3). Fail CI when:
#   1) conflict markers are committed
#   2) god-file line budgets are exceeded (ratchet; raise only when intentional)
#   3) a PR edits Cargo.lock without a Cargo.toml dep edit or the dependencies label
#   4) VERSION copies disagree (cargo xtask bump --check)
#   5) a non-release PR touches version lockstep files (needs `release` label)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0

# --- 1. Conflict markers (study: ^<<<<<<< / ^>>>>>>>) ---------------------------
# git grep exits 1 for "no match"; anything else is an error and must fail.
grep_rc=0
markers=$(git grep -nE '^(<<<<<<<|>>>>>>>)' -- . ':!.git/**' 2>/dev/null) || grep_rc=$?
if (( grep_rc > 1 )); then
  echo "ci-guards: git grep for conflict markers errored (rc=$grep_rc)" >&2
  fail=1
elif [[ -n "$markers" ]]; then
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

# --- Shared PR helpers (labels + version fingerprints) -------------------------
pr_labels_csv=""
pr_merge_base=""
pr_changed=()

fetch_pr_labels() {
  # Prefer live PR labels (API/gh). The pull_request event payload is a
  # snapshot: a workflow re-run after adding a label still sees the old
  # labels unless we refetch. labeled/unlabeled triggers also re-run CI.
  pr_labels_csv="${GITHUB_PR_LABELS:-}"
  if [[ -n "$pr_labels_csv" ]]; then
    return
  fi
  local pr_number=""
  if [[ -n "${GITHUB_EVENT_PATH:-}" && -f "${GITHUB_EVENT_PATH}" ]]; then
    pr_number=$(python3 -c 'import json,os; e=json.load(open(os.environ["GITHUB_EVENT_PATH"])); print(e.get("pull_request",{}).get("number") or e.get("number") or "")')
  fi
  if [[ -n "$pr_number" && -n "${GITHUB_REPOSITORY:-}" ]]; then
    if command -v gh >/dev/null 2>&1; then
      pr_labels_csv=$(GH_TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-}}" gh api "repos/${GITHUB_REPOSITORY}/pulls/${pr_number}" --jq '[.labels[].name]|join(",")' 2>/dev/null || true)
    elif [[ -n "${GITHUB_TOKEN:-}" ]]; then
      pr_labels_csv=$(
        PR_NUMBER="$pr_number" python3 <<'PY' 2>/dev/null || true
import json, os, urllib.request
req = urllib.request.Request(
    f"https://api.github.com/repos/{os.environ['GITHUB_REPOSITORY']}/pulls/{os.environ['PR_NUMBER']}",
    headers={
        "Authorization": f"Bearer {os.environ['GITHUB_TOKEN']}",
        "Accept": "application/vnd.github+json",
        "User-Agent": "ci-guards",
    },
)
print(",".join(l["name"] for l in json.load(urllib.request.urlopen(req)).get("labels", [])))
PY
      )
    fi
  fi
  if [[ -z "$pr_labels_csv" && -n "${GITHUB_EVENT_PATH:-}" && -f "${GITHUB_EVENT_PATH}" ]]; then
    pr_labels_csv=$(python3 -c 'import json,os; e=json.load(open(os.environ["GITHUB_EVENT_PATH"])); print(",".join(l["name"] for l in e.get("pull_request",{}).get("labels",[])))')
  fi
}

workspace_package_version() {
  local file="$1"
  [[ -f "$file" ]] || { echo ""; return; }
  python3 - "$file" <<'PY'
import sys
text = open(sys.argv[1], encoding="utf-8").read()
try:
    pkg = text.split("[workspace.package]", 1)[1].split("[workspace.dependencies]", 1)[0]
except IndexError:
    print("")
    raise SystemExit(0)
for line in pkg.splitlines():
    s = line.strip()
    if s.startswith("version") and "=" in s:
        print(s.split("=", 1)[1].strip().strip('"'))
        break
else:
    print("")
PY
}

cabin_reports_version_line() {
  local file="$1"
  [[ -f "$file" ]] || { echo ""; return; }
  grep -E 'assert_eq!\(env!\("CARGO_PKG_VERSION"\),' "$file" | head -n1 || true
}

grokhub_lock_versions() {
  local file="$1"
  [[ -f "$file" ]] || { echo ""; return; }
  python3 - "$file" <<'PY'
import sys
text = open(sys.argv[1], encoding="utf-8").read().splitlines()
want = {"grokhub-app", "grokhub-agent", "grokhub-core", "grokhub-hub", "grokhub-proto"}
out = []
i = 0
while i < len(text):
    line = text[i]
    if line.startswith("name = \"") and line[8:-1] in want:
        name = line[8:-1]
        ver = ""
        if i + 1 < len(text) and text[i + 1].startswith("version = \""):
            ver = text[i + 1][11:-1]
        out.append(f"{name}={ver}")
    i += 1
print(",".join(sorted(out)))
PY
}

reference_version_fingerprint() {
  local file="$1" version="$2"
  [[ -f "$file" ]] || { echo ""; return; }
  python3 - "$file" "$version" <<'PY'
import sys
text = open(sys.argv[1], encoding="utf-8").read()
v = sys.argv[2]
needles = [
    f"**v{v}**",
    f"grokhub-linux-v{v}.tar.gz",
    f"GrokHub-Setup-{v}.exe",
    f"grokhub-windows-v{v}.zip",
    f"GrokHub {v}-beta.",
    f"GrokHub {v} (main",
    f"GrokHub {v}",
]
print("|".join(f"{n}:{text.count(n)}" for n in needles))
PY
}

# --- 3 + 5. PR-only rules (Cargo.lock hatch + non-release version files) -------
if [[ "${GITHUB_EVENT_NAME:-}" == "pull_request" ]]; then
  base_ref="${GITHUB_BASE_REF:-beta}"
  # Full history for merge-base (no shallow --depth that can miss the fork point).
  git fetch --no-tags origin "$base_ref" >/dev/null 2>&1 || true
  pr_merge_base=$(git merge-base "origin/${base_ref}" HEAD 2>/dev/null || true)
  if [[ -z "$pr_merge_base" ]]; then
    echo "ci-guards: could not resolve merge-base against origin/${base_ref}; failing PR file rules" >&2
    fail=1
  else
    # Capture first: mapfile < <(...) would swallow a git diff failure and
    # leave an empty file list that skips every PR rule.
    if ! changed_out=$(git diff --name-only "$pr_merge_base"...HEAD); then
      echo "ci-guards: git diff against merge-base failed; failing PR file rules" >&2
      fail=1
      changed_out=""
    fi
    mapfile -t pr_changed <<<"$changed_out"
    [[ -n "$changed_out" ]] || pr_changed=()
    fetch_pr_labels
    echo "ci-guards: PR labels=[${pr_labels_csv}]"

    # --- 3. Cargo.lock needs dependencies label --------------------------------
    lock_changed=0
    toml_paths=()
    for f in "${pr_changed[@]+"${pr_changed[@]}"}"; do
      case "$f" in
        Cargo.lock) lock_changed=1 ;;
        Cargo.toml|*/Cargo.toml) toml_paths+=("$f") ;;
      esac
    done
    if (( lock_changed == 1 )); then
      toml_dep=0
      if ((${#toml_paths[@]} > 0)); then
        if git diff -U0 "$pr_merge_base"...HEAD -- "${toml_paths[@]}" \
          | grep -E '^\+[^+]|^-[^-]' \
          | grep -vE '^\+\+\+|^\-\-\-' \
          | grep -vE '^[+-][[:space:]]*version[[:space:]]*=' \
          | grep -q .; then
          toml_dep=1
        fi
      fi
      # `release` only unlocks Cargo.lock when grokhub-* package versions changed
      # (xtask bump). Other lock churn still needs `dependencies` or a toml dep edit.
      has_lock_hatch=0
      case ",${pr_labels_csv}," in
        *,dependencies,*|*,deps,*) has_lock_hatch=1 ;;
      esac
      case ",${pr_labels_csv}," in
        *,release,*)
          old_lock=$(mktemp)
          git show "$pr_merge_base:Cargo.lock" >"$old_lock" 2>/dev/null || true
          old_gv=$(grokhub_lock_versions "$old_lock")
          new_gv=$(grokhub_lock_versions "Cargo.lock")
          rm -f "$old_lock"
          if [[ -n "$old_gv" && -n "$new_gv" && "$old_gv" != "$new_gv" ]]; then
            has_lock_hatch=1
          fi
          ;;
      esac
      if (( toml_dep == 0 && has_lock_hatch == 0 )); then
        echo "ci-guards: Cargo.lock changed without a Cargo.toml dependency edit or the 'dependencies'/'release' (grokhub-* versions) label." >&2
        echo "ci-guards: add label 'dependencies' for a lock-only dep bump, 'release' for xtask bump of grokhub-* versions, or include the Cargo.toml dep change." >&2
        fail=1
      else
        echo "ci-guards: Cargo.lock change allowed (toml_dep=$toml_dep hatch=$has_lock_hatch)"
      fi
    else
      echo "ci-guards: Cargo.lock unchanged"
    fi

    # --- 5. Non-release PRs may not touch version lockstep files ---------------
    # Escape hatch: label `release` (promotion / hotfix that runs xtask bump).
    has_release_label=0
    case ",${pr_labels_csv}," in
      *,release,*) has_release_label=1 ;;
    esac

    version_hits=()
    for f in "${pr_changed[@]+"${pr_changed[@]}"}"; do
      case "$f" in
        VERSION|CHANGELOG.md|packaging/PKGBUILD|packaging/aur/PKGBUILD|packaging/windows/grokhub.iss)
          version_hits+=("$f")
          ;;
      esac
    done

    if printf '%s\n' "${pr_changed[@]+"${pr_changed[@]}"}" | grep -qx 'Cargo.toml'; then
      old_toml=$(mktemp)
      git show "$pr_merge_base:Cargo.toml" >"$old_toml" 2>/dev/null || true
      old_ver=$(workspace_package_version "$old_toml")
      new_ver=$(workspace_package_version "Cargo.toml")
      rm -f "$old_toml"
      if [[ -z "$old_ver" || -z "$new_ver" ]]; then
        echo "ci-guards: could not parse Cargo.toml [workspace.package].version (old='${old_ver}' new='${new_ver}')" >&2
        fail=1
      elif [[ "$old_ver" != "$new_ver" ]]; then
        version_hits+=("Cargo.toml[workspace.package].version (${old_ver}->${new_ver})")
      fi
    fi

    if printf '%s\n' "${pr_changed[@]+"${pr_changed[@]}"}" | grep -qx 'crates/grokhub-app/src/cli.rs'; then
      old_cli=$(mktemp)
      git show "$pr_merge_base:crates/grokhub-app/src/cli.rs" >"$old_cli" 2>/dev/null || true
      old_line=$(cabin_reports_version_line "$old_cli")
      new_line=$(cabin_reports_version_line "crates/grokhub-app/src/cli.rs")
      rm -f "$old_cli"
      if [[ -z "$old_line" || -z "$new_line" ]]; then
        echo "ci-guards: could not find the cabin_reports_version line in cli.rs (old/new)" >&2
        fail=1
      elif [[ "$old_line" != "$new_line" ]]; then
        version_hits+=("crates/grokhub-app/src/cli.rs (cabin_reports_version)")
      fi
    fi

    if printf '%s\n' "${pr_changed[@]+"${pr_changed[@]}"}" | grep -qx 'Cargo.lock'; then
      old_lock=$(mktemp)
      git show "$pr_merge_base:Cargo.lock" >"$old_lock" 2>/dev/null || true
      old_gv=$(grokhub_lock_versions "$old_lock")
      new_gv=$(grokhub_lock_versions "Cargo.lock")
      rm -f "$old_lock"
      if [[ -z "$old_gv" || -z "$new_gv" ]]; then
        echo "ci-guards: could not read grokhub-* versions from Cargo.lock (old/new)" >&2
        fail=1
      elif [[ "$old_gv" != "$new_gv" ]]; then
        version_hits+=("Cargo.lock (grokhub-* version)")
      fi
    fi

    if printf '%s\n' "${pr_changed[@]+"${pr_changed[@]}"}" | grep -qx 'docs/REFERENCE.md'; then
      ver_now=$(tr -d '[:space:]' <VERSION)
      old_ref=$(mktemp)
      git show "$pr_merge_base:docs/REFERENCE.md" >"$old_ref" 2>/dev/null || true
      ver_old=$(git show "$pr_merge_base:VERSION" 2>/dev/null | tr -d '[:space:]' || true)
      fp_old_a=$(reference_version_fingerprint "$old_ref" "${ver_old:-$ver_now}")
      fp_new_a=$(reference_version_fingerprint "docs/REFERENCE.md" "${ver_old:-$ver_now}")
      fp_old_b=$(reference_version_fingerprint "$old_ref" "$ver_now")
      fp_new_b=$(reference_version_fingerprint "docs/REFERENCE.md" "$ver_now")
      rm -f "$old_ref"
      if [[ "$fp_old_a" != "$fp_new_a" || "$fp_old_b" != "$fp_new_b" ]]; then
        version_hits+=("docs/REFERENCE.md (version tokens)")
      fi
    fi

    if ((${#version_hits[@]} > 0)); then
      if (( has_release_label == 1 )); then
        echo "ci-guards: version-file edits allowed (release label): ${version_hits[*]}"
      else
        echo "ci-guards: non-release PR touches version lockstep files:" >&2
        for h in "${version_hits[@]}"; do
          echo "  - $h" >&2
        done
        echo "ci-guards: add label 'release' for a promotion/hotfix (cargo xtask bump), or move notes to changelog.d/." >&2
        fail=1
      fi
    else
      echo "ci-guards: no version lockstep file edits"
    fi
  fi
else
  echo "ci-guards: skip PR file rules (event=${GITHUB_EVENT_NAME:-local})"
fi

# --- 4. Version lockstep (rearch stage 2) --------------------------------------
if cargo xtask bump --check; then
  echo "ci-guards: xtask bump --check ok"
else
  echo "ci-guards: xtask bump --check failed (VERSION copies disagree)" >&2
  fail=1
fi

if (( fail != 0 )); then
  exit 1
fi
echo "ci-guards: ok"
