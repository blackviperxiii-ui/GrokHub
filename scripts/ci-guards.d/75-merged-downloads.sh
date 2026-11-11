# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 7b. Merged downloads leave out the unsigned Windows staging files ---------
# release.yml uploads the unsigned Windows installer and zip as the artifact
# `windows-signpath-unsigned` so SignPath can fetch them. They have the same
# file names as the signed ones in `windows`, so a download step that merges
# every artifact into one directory lets either copy win, and a release could
# ship an unsigned file. Every `merge-multiple` download must name what it
# takes with a quoted `pattern:` that does not match the staging artifact.
# Patterns are read as download-artifact reads them (minimatch): `*`, `?` and
# one level of `{a,b}`. Anything else fails here instead of being guessed at.
dl_staging=windows-signpath-unsigned

# dl_alternatives <pattern>: each brace alternative, one per line.
dl_alternatives() {
  local p="$1"
  if [[ "$p" =~ ^([^{}]*)\{([^{}]*)\}(.*)$ ]]; then
    local pre="${BASH_REMATCH[1]}" body="${BASH_REMATCH[2]}" post="${BASH_REMATCH[3]}" alt
    local -a alts
    IFS=, read -ra alts <<<"$body"
    for alt in "${alts[@]}"; do
      dl_alternatives "$pre$alt$post"
    done
  else
    printf '%s\n' "$p"
  fi
}

# dl_problem <quoted|bare> <pattern>: prints why the pattern is refused, or nothing.
dl_problem() {
  local kind="$1" pat="$2" alt
  if [[ "$kind" == bare && "$pat" == *"{"* ]]; then
    echo "quote the pattern (YAML reads an unquoted {a,b} as a map)"
    return
  fi
  if [[ ! "$pat" =~ ^[A-Za-z0-9_.*?{},-]+$ || "$pat" == *"{,"* || "$pat" == *",,"* \
    || "$pat" == *",}"* || "$pat" == *"{}"* || "$pat" == *".."* ]]; then
    echo "pattern '$pat' is more than names, *, ? and {a,b}; this guard cannot check it"
    return
  fi
  while IFS= read -r alt; do
    if [[ "$alt" == *[{},]* ]]; then
      echo "pattern '$pat' has unbalanced or nested braces; this guard cannot check it"
      return
    fi
    # Unquoted on purpose: the alternative is the glob.
    if [[ "$dl_staging" == $alt ]]; then
      echo "pattern '$pat' also takes $dl_staging"
      return
    fi
  done < <(dl_alternatives "$pat")
}

shopt -s nullglob
dl_files=(.github/workflows/*.yml .github/workflows/*.yaml)
shopt -u nullglob
# No workflows at all is the pinned-actions guard's failure to report.
if ((${#dl_files[@]} > 0)); then
  rc=0
  # One line per merging download step: file:line <TAB> none|flow|quoted|bare <TAB> pattern.
  dl_steps=$(awk '
    function flush() {
      if (dl && flow) printf "%s:%d\tflow\t\n", file, start
      else if (dl && merge) printf "%s:%d\t%s\t%s\n", file, start, kind, pat
      dl = 0; merge = 0; flow = 0; kind = "none"; pat = ""; in_step = 0
    }
    FNR == 1 { flush() }
    /^[[:space:]]*(#|$)/ { next }
    {
      match($0, /^[[:space:]]*/)
      ind = RLENGTH
      if ($0 ~ /^[[:space:]]*- /) {
        flush(); in_step = 1; step_ind = ind; start = FNR; file = FILENAME
      } else if (in_step && ind <= step_ind) {
        flush()
      }
      if (!in_step) next
      low = tolower($0)
      if (low ~ /(^|[[:space:]{,])["\047]?uses["\047]?[[:space:]]*:[[:space:]]*["\047]?actions\/download-artifact@/) dl = 1
      if (low ~ /(^|[[:space:]{,])["\047]?with["\047]?[[:space:]]*:[[:space:]]*\{/) flow = 1
      if (low ~ /^[[:space:]]*["\047]?merge-multiple["\047]?[[:space:]]*:/) {
        val = low
        sub(/^[^:]*:[[:space:]]*/, "", val)
        sub(/[[:space:]]+#.*$/, "", val)
        gsub(/["\047[:space:]]/, "", val)
        # Anything but a plain false may merge.
        if (val != "false") merge = 1
      }
      if (low ~ /^[[:space:]]*["\047]?pattern["\047]?[[:space:]]*:/) {
        val = $0
        sub(/^[^:]*:[[:space:]]*/, "", val)
        q = substr(val, 1, 1)
        if (q == "\"" || q == "\047") {
          val = substr(val, 2)
          end = index(val, q)
          val = end ? substr(val, 1, end - 1) : val
          kind = "quoted"
        } else {
          sub(/[[:space:]]+#.*$/, "", val)
          sub(/[[:space:]]+$/, "", val)
          kind = "bare"
        }
        pat = val
      }
    }
    END { flush() }
  ' "${dl_files[@]}") || rc=$?
  if ((rc != 0)); then
    echo "ci-guards: could not read workflow files (awk exit $rc)" >&2
    fail=1
  else
    dl_bad=""
    dl_count=0
    while IFS=$'\t' read -r dl_where dl_kind dl_pat; do
      [[ -n "$dl_where" ]] || continue
      dl_count=$((dl_count + 1))
      case "$dl_kind" in
        flow) dl_why="write the step's with: as a block, one input per line" ;;
        none) dl_why="merge-multiple needs a pattern: naming the artifacts to take" ;;
        *) dl_why=$(dl_problem "$dl_kind" "$dl_pat") ;;
      esac
      if [[ -n "$dl_why" ]]; then
        dl_bad+="$dl_where: $dl_why"$'\n'
      fi
    done <<<"$dl_steps"
    if [[ -n "$dl_bad" ]]; then
      echo "ci-guards: a merged artifact download could take the unsigned Windows staging files:" >&2
      printf '%s' "$dl_bad" >&2
      fail=1
    else
      echo "ci-guards: merged artifact downloads ($dl_count) leave out $dl_staging"
    fi
  fi
fi
