# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 8. No anti-cheat red flags in our own code --------------------------------
# Loadout never reads another process's memory, lists its modules, injects into
# it, or hooks the keyboard or mouse globally: anti-cheats flag all of these.
# A hit fails unless the line, or the line right above it, carries
# `ci-guard: allow-anticheat-api <reason>`.
anticheat_pattern='SetWindowsHookEx|WH_KEYBOARD_LL|WH_MOUSE_LL|PROCESS_VM_READ|PROCESS_VM_WRITE|PROCESS_VM_OPERATION|PROCESS_ALL_ACCESS|ReadProcessMemory|WriteProcessMemory|CreateRemoteThread|EnumProcessModules|TH32CS_SNAPMODULE|Module32First'
anticheat_hits=$(find crates -name '*.rs' -print0 | xargs -0 awk -v pat="$anticheat_pattern" '
  FNR == 1 { allow = 0 }
  /ci-guard: allow-anticheat-api [^ ]/ { allow = FNR; next }
  $0 ~ pat && $0 !~ /^[[:space:]]*\/\// {
    if (allow != FNR - 1) printf "%s:%d: %s\n", FILENAME, FNR, $0
  }
')
if [[ -n "$anticheat_hits" ]]; then
  echo "ci-guards: an API anti-cheats flag (memory reads, module lists, injection, global input hooks):" >&2
  echo "$anticheat_hits" >&2
  fail=1
else
  echo "ci-guards: no anti-cheat red-flag APIs"
fi
