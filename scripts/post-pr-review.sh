#!/usr/bin/env bash
set -euo pipefail

review_file="${1:-review-output.md}"

if [[ -z "${GITHUB_REPOSITORY:-}" || -z "${PR_NUMBER:-}" ]]; then
  echo "GITHUB_REPOSITORY and PR_NUMBER are required." >&2
  exit 2
fi

if [[ ! -s "$review_file" ]]; then
  echo "Review output file is empty: $review_file" >&2
  exit 2
fi

gh pr review "$PR_NUMBER" \
  --repo "$GITHUB_REPOSITORY" \
  --comment \
  --body-file "$review_file"
