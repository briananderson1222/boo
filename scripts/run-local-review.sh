#!/usr/bin/env bash
set -euo pipefail

base_ref="${1:-origin/main}"
output_file="${2:-review-output.md}"
json_file="${3:-review-findings.json}"

exec ./scripts/run-kiro-review.sh \
  --agent code-reviewer \
  --review-kind "local PR review" \
  --base-ref "$base_ref" \
  --output-markdown "$output_file" \
  --output-json "$json_file"
