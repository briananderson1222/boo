#!/usr/bin/env bash
set -euo pipefail

agent="code-reviewer"
base_ref="origin/main"
review_kind="PR review"
output_markdown="review-output.md"
output_json="review-findings.json"
strict_args=()
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --agent) agent="$2"; shift 2 ;;
    --base-ref) base_ref="$2"; shift 2 ;;
    --review-kind) review_kind="$2"; shift 2 ;;
    --output-markdown) output_markdown="$2"; shift 2 ;;
    --output-json) output_json="$2"; shift 2 ;;
    --strict) strict_args=(--strict); shift ;;
    --help)
      echo "Usage: $0 --agent <agent> --base-ref <ref> --review-kind <name> --output-markdown <path> --output-json <path> [--strict]"
      exit 0
      ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [[ -z "${KIRO_API_KEY:-}" ]]; then
  echo "KIRO_API_KEY is required for Kiro headless mode." >&2
  exit 2
fi

if ! command -v kiro-cli >/dev/null 2>&1; then
  echo "kiro-cli is not available on PATH." >&2
  exit 2
fi

kiro-cli --version

if ! git rev-parse --verify "$base_ref" >/dev/null 2>&1; then
  base_ref="HEAD~1"
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

diff_file="$tmp_dir/pr.diff"
raw_output="$tmp_dir/raw-review.md"

git diff "$base_ref"...HEAD > "$diff_file" 2>/dev/null || git diff "$base_ref" HEAD > "$diff_file"

kiro-cli chat \
  --no-interactive \
  --trust-tools=read,grep \
  --agent "$agent" \
  "Run a ${review_kind} for this diff. Return concise markdown findings.

Your final line is mandatory automation data. End with exactly one single-line HTML comment matching this shape:
<!-- REVIEW_DATA: [{\"severity\":\"HIGH\",\"confidence\":\"medium\",\"file\":\"src/app.js\",\"line\":1,\"description\":\"short actionable description\",\"source\":\"kiro\"}] -->

If there are no findings, the final line must be exactly:
<!-- REVIEW_DATA: [] -->

Do not omit REVIEW_DATA. Do not wrap REVIEW_DATA in a code fence.

$(cat "$diff_file")" > "$raw_output"

node "$script_dir/normalize-review-output.mjs" "$raw_output" \
  --json "$output_json" \
  --markdown "$output_markdown" \
  --title "$review_kind" \
  "${strict_args[@]}"

cat "$output_markdown"
