# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# packaging/aur/.SRCINFO must match packaging/aur/PKGBUILD (no makepkg in CI).
if python3 scripts/aur-srcinfo.py --check; then
  echo "ci-guards: aur .SRCINFO matches PKGBUILD"
else
  echo "ci-guards: aur .SRCINFO drifts from PKGBUILD (python3 scripts/aur-srcinfo.py --write)" >&2
  fail=1
fi
