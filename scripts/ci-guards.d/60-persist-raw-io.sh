# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 6. No raw spawn/write in app persist paths (rearch stage 6) ---------------
# Persist writes go through PersistService (app/persist_service.rs). A raw
# thread::spawn or fs::write in these files fails unless the line, or the line
# right above it, carries `ci-guard: allow-raw-io <reason>`.
raw_io_files=(
  "crates/grokhub-app/src/app/persist.rs"
  "crates/grokhub-app/src/app/projects.rs"
  "crates/grokhub-app/src/app/dock_ui.rs"
)
raw_io_bad=0
for path in "${raw_io_files[@]}"; do
  if [[ ! -f "$path" ]]; then
    echo "ci-guards: missing raw-io guarded file $path" >&2
    fail=1
    raw_io_bad=1
    continue
  fi
  hits=$(awk '
    /ci-guard: allow-raw-io [^ ]/ { allow = NR; next }
    /(^|[^A-Za-z0-9_])(std::)?(thread::spawn|fs::write)([^A-Za-z0-9_]|$)/ {
      if (allow != NR - 1) printf "%s:%d: %s\n", FILENAME, NR, $0
    }
  ' "$path")
  if [[ -n "$hits" ]]; then
    echo "ci-guards: raw thread::spawn / fs::write in a persist path (use PersistService or mark it \`// ci-guard: allow-raw-io <reason>\`):" >&2
    echo "$hits" >&2
    fail=1
    raw_io_bad=1
  fi
done
if (( raw_io_bad == 0 )); then
  echo "ci-guards: persist paths have no unmarked raw spawn/write"
fi
