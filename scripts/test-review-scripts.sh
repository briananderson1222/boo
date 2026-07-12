#!/usr/bin/env bash
# Self-contained smoke tests for the Kiro review helper scripts.
# Plain bash + node, no external dependencies.
set -uo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

pass_count=0
fail_count=0

pass() {
  echo "PASS: $1"
  pass_count=$((pass_count + 1))
}

fail() {
  echo "FAIL: $1"
  fail_count=$((fail_count + 1))
}

assert_eq() {
  local description="$1" expected="$2" actual="$3"
  if [[ "$expected" == "$actual" ]]; then
    pass "$description"
  else
    fail "$description (expected: $expected, actual: $actual)"
  fi
}

assert_exit_code() {
  local description="$1" expected="$2" actual="$3"
  if [[ "$expected" == "$actual" ]]; then
    pass "$description"
  else
    fail "$description (expected exit $expected, actual exit $actual)"
  fi
}

assert_contains() {
  local description="$1" haystack="$2" needle="$3"
  if [[ "$haystack" == *"$needle"* ]]; then
    pass "$description"
  else
    fail "$description (expected output to contain: $needle)"
  fi
}

### Test 1: A8 repro — visible finding numbering must match selection.
# A LOW finding appears first in the source data, followed by a HIGH
# finding. The rendered review shows blockers (HIGH/CRITICAL) first, so
# displayed finding #1 must be the HIGH finding, and selecting #1 must
# return that same HIGH finding.
cat > "$tmp_dir/raw-review.md" <<'EOF'
Some review text.

<!-- REVIEW_DATA: [{"severity":"LOW","confidence":"low","file":"src/a.rs","line":1,"description":"low finding","source":"kiro"},{"severity":"HIGH","confidence":"high","file":"src/b.rs","line":2,"description":"high finding","source":"kiro"}] -->
EOF

node "$script_dir/normalize-review-output.mjs" "$tmp_dir/raw-review.md" \
  --json "$tmp_dir/review-findings.json" --markdown "$tmp_dir/review-output.md" >/dev/null

markdown_contents="$(cat "$tmp_dir/review-output.md")"
assert_contains "rendered markdown lists HIGH finding first" "$markdown_contents" "1. **[HIGH]** high finding"

selected="$(node "$script_dir/select-review-finding.mjs" "$tmp_dir/review-findings.json" 1)"
selected_description="$(node -e 'console.log(JSON.parse(require("fs").readFileSync(0,"utf8")).finding.description)' <<<"$selected")"
assert_eq "select-review-finding #1 returns the displayed HIGH finding" "high finding" "$selected_description"

selected_severity="$(node -e 'console.log(JSON.parse(require("fs").readFileSync(0,"utf8")).finding.severity)' <<<"$selected")"
assert_eq "select-review-finding #1 finding severity is HIGH" "HIGH" "$selected_severity"

### Test 2: empty findings path.
cat > "$tmp_dir/raw-review-empty.md" <<'EOF'
Nothing to see here.

<!-- REVIEW_DATA: [] -->
EOF

node "$script_dir/normalize-review-output.mjs" "$tmp_dir/raw-review-empty.md" \
  --json "$tmp_dir/review-findings-empty.json" --markdown "$tmp_dir/review-output-empty.md" >/dev/null

set +e
select_output="$(node "$script_dir/select-review-finding.mjs" "$tmp_dir/review-findings-empty.json" 1 2>&1)"
select_exit=$?
set -e 2>/dev/null || true

assert_exit_code "select-review-finding on empty findings exits 1" "1" "$select_exit"
assert_contains "select-review-finding on empty findings prints a clean message" "$select_output" "not found"

### Test 3: malformed-input path — finding missing required fields.
cat > "$tmp_dir/malformed-findings.json" <<'EOF'
{"findings": [{"severity": "HIGH"}]}
EOF

set +e
malformed_output="$(node "$script_dir/select-review-finding.mjs" "$tmp_dir/malformed-findings.json" 1 2>&1)"
malformed_exit=$?
set -e 2>/dev/null || true

assert_exit_code "select-review-finding on malformed finding exits 2" "2" "$malformed_exit"
assert_contains "select-review-finding on malformed finding prints a clean message" "$malformed_output" "missing required fields"

### Test 4: malformed-input path — findings JSON is not an array.
cat > "$tmp_dir/not-an-array.json" <<'EOF'
{"findings": "oops"}
EOF

set +e
not_array_output="$(node "$script_dir/select-review-finding.mjs" "$tmp_dir/not-an-array.json" 1 2>&1)"
not_array_exit=$?
set -e 2>/dev/null || true

assert_exit_code "select-review-finding on non-array findings exits 2" "2" "$not_array_exit"
assert_contains "select-review-finding on non-array findings prints a clean message" "$not_array_output" "findings array"

### Test 5: parse-review-command.mjs with a missing --file path.
set +e
missing_file_output="$(node "$script_dir/parse-review-command.mjs" /open-issue --file "$tmp_dir/does-not-exist.txt" 2>&1)"
missing_file_exit=$?
set -e 2>/dev/null || true

assert_exit_code "parse-review-command on missing file exits 2" "2" "$missing_file_exit"
assert_contains "parse-review-command on missing file prints a clean message" "$missing_file_output" "Unable to read comment body file"

### Test 6: parse-review-command.mjs happy path still works.
parsed_number="$(node "$script_dir/parse-review-command.mjs" /open-issue "/open-issue 3")"
assert_eq "parse-review-command parses a valid command" "3" "$parsed_number"

echo
echo "Results: $pass_count passed, $fail_count failed."
if [[ "$fail_count" -gt 0 ]]; then
  exit 1
fi
exit 0
