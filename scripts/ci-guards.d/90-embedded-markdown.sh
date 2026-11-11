# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 9. Markdown compiled into the binaries counts as code ---------------------
# ci.yml skips the build jobs for a pull request that only touches Markdown,
# docs/, screenshots/ or research/. A file under those rules that Rust pulls in
# with include_str! or include_bytes! changes the binaries, so the `embedded`
# path filter in ci.yml turns the build jobs back on for it. This guard fails
# when such a file sits where that filter does not reach, or when ci.yml and
# the list below stop agreeing.
embedded_yml=.github/workflows/ci.yml
embedded_want=$'crates/**/*.md\nsrc-tauri/**/*.md\nPRIVACY.md'
embedded_bad=0
if [[ ! -f "$embedded_yml" ]]; then
  echo "ci-guards: missing $embedded_yml" >&2
  fail=1
  embedded_bad=1
else
  embedded_have=$(awk '
    /^ +embedded:[[:space:]]*$/ { on = 1; next }
    on && /^ +- / { sub(/^ +- +/, ""); gsub(/\047|[[:space:]]+$/, ""); print; next }
    { on = 0 }
  ' "$embedded_yml")
  if [[ "$embedded_have" != "$embedded_want" ]]; then
    echo "ci-guards: the embedded filter in $embedded_yml must list exactly:" >&2
    echo "$embedded_want" >&2
    fail=1
    embedded_bad=1
  fi
  for embedded_output in code windows; do
    if ! grep -qE "^ +$embedded_output: .*steps\.paths\.outputs\.embedded == 'true'" "$embedded_yml"; then
      echo "ci-guards: the $embedded_output output in $embedded_yml must include steps.paths.outputs.embedded == 'true'" >&2
      fail=1
      embedded_bad=1
    fi
  done
fi

# One line per embedded file the build filters would skip: `ok <path>` when
# the embedded filter covers it, `bad <file>:<line>: <what>` when not. A path
# built with concat! or env! cannot be resolved here, so one that names
# Markdown or a skipped directory is `bad` too.
embedded_rc=0
embedded_scan=$(git ls-files -z -- '*.rs' | xargs -0 awk '
  function resolve(file, target,    dir, n, parts, i, m, out, path) {
    dir = file
    sub(/[^\/]*$/, "", dir)
    n = split(dir target, parts, "/")
    m = 0
    for (i = 1; i <= n; i++) {
      if (parts[i] == "" || parts[i] == ".") continue
      if (parts[i] == "..") { if (m > 0) m--; continue }
      out[++m] = parts[i]
    }
    path = ""
    for (i = 1; i <= m; i++) path = path (i > 1 ? "/" : "") out[i]
    return path
  }
  function check(arg,    target, path) {
    if (arg ~ /^[[:space:]]*"[^"]*"[[:space:]]*,?[[:space:]]*(\)|$)/) {
      target = arg
      sub(/^[[:space:]]*"/, "", target)
      sub(/".*$/, "", target)
      path = resolve(FILENAME, target)
      if (path !~ /\.md$/ && path !~ /^(docs|screenshots|research)\//) return
      if (path == "PRIVACY.md" || path ~ /^(crates|src-tauri)\/.*\.md$/) print "ok " path
      else printf "bad %s:%d: %s\n", FILENAME, FNR, path
    } else if (arg ~ /\.md"|(docs|screenshots|research)\//) {
      printf "bad %s:%d: a computed path, write it as one plain string\n", FILENAME, FNR
    }
  }
  FNR == 1 { pending = 0 }
  /^[[:space:]]*\/\// { next }
  {
    line = $0
    if (pending) { pending = 0; check(line) }
    while (match(line, /include_(str|bytes)!\(/)) {
      line = substr(line, RSTART + RLENGTH)
      if (line ~ /^[[:space:]]*$/) pending = 1
      else check(line)
    }
  }
') || embedded_rc=$?
if (( embedded_rc != 0 )); then
  echo "ci-guards: could not scan Rust sources for embedded files (rc=$embedded_rc)" >&2
  fail=1
  embedded_bad=1
fi
embedded_out=$(printf '%s\n' "$embedded_scan" | sed -n 's/^bad //p')
if [[ -n "$embedded_out" ]]; then
  echo "ci-guards: embedded files the ci.yml embedded filter does not cover (keep embedded Markdown under crates/ or src-tauri/ and other embedded files out of docs/, screenshots/ and research/, or extend the filter and this guard together):" >&2
  echo "$embedded_out" >&2
  fail=1
  embedded_bad=1
fi
if (( embedded_bad == 0 )); then
  embedded_count=$(printf '%s\n' "$embedded_scan" | grep -c '^ok ' || true)
  echo "ci-guards: $embedded_count embedded Markdown files, all covered by the ci.yml embedded filter"
fi
