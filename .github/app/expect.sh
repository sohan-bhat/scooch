expectations=0
failures=0

expect() {
  local text="$1"
  shift
  expectations=$((expectations + 1))
  if "$@" >/dev/null 2>&1; then
    line "- yes: $text"
  else
    failures=$((failures + 1))
    line "- NO: $text"
  fi
}

finish() {
  line
  line "Expectations $expectations, failed $failures."
  if [ "$expectations" -eq 0 ] || [ "$failures" -gt 0 ]; then
    exit 1
  fi
  exit 0
}
