#!/usr/bin/env bash
set -euo pipefail

# Kiro Code Review — Installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/briananderson1222/kiro-code-review/main/install.sh | bash
#   curl -fsSL ... | bash -s -- --component pr-review
#   curl -fsSL ... | bash -s -- --component codebase-scan
#   curl -fsSL ... | bash -s -- --component open-issue
#   ./install.sh --dir /path/to/repo

REPO="briananderson1222/kiro-code-review"
BRANCH="main"
TARGET_DIR="${TARGET_DIR:-.}"
COMPONENTS=()

usage() {
  cat <<EOF
Kiro Code Review Installer

Usage: install.sh [OPTIONS]

Options:
  --dir <path>         Target repo directory (default: current directory)
  --component <name>   Install specific component (repeatable). Options:
                         pr-review     — PR review workflow + agents
                         codebase-scan — Codebase scan workflow + agent
                         open-issue    — /open-issue slash command workflow
                         all           — Everything (default)
  --help               Show this help

Examples:
  install.sh                              # Install everything to current dir
  install.sh --component pr-review        # Just the PR review pipeline
  install.sh --component codebase-scan --component open-issue
  install.sh --dir ~/dev/my-project
EOF
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dir) TARGET_DIR="$2"; shift 2 ;;
    --component) COMPONENTS+=("$2"); shift 2 ;;
    --help) usage ;;
    *) echo "Unknown option: $1"; usage ;;
  esac
done

[[ ${#COMPONENTS[@]} -eq 0 ]] && COMPONENTS=("all")

TARGET_DIR=$(cd "$TARGET_DIR" && pwd)
echo "Installing to: $TARGET_DIR"

# Temp dir for downloads
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

fetch() {
  local path="$1"
  local dest="$TARGET_DIR/$path"
  mkdir -p "$(dirname "$dest")"
  curl -fsSL "https://raw.githubusercontent.com/$REPO/$BRANCH/$path" -o "$dest"
  echo "  ✓ $path"
}

should_install() {
  local component="$1"
  for c in "${COMPONENTS[@]}"; do
    [[ "$c" == "all" || "$c" == "$component" ]] && return 0
  done
  return 1
}

# Shared skill (needed by all review components)
install_shared() {
  echo ""
  echo "Shared:"
  fetch ".kiro/skills/review-criteria/SKILL.md"
  fetch ".kiro/agents/review-pass.json"
  fetch ".kiro/agents/review-scorer.json"
}

install_pr_review() {
  echo ""
  echo "PR Review:"
  fetch ".github/workflows/kiro-code-review.yml"
  fetch ".kiro/agents/code-reviewer.json"
  fetch ".kiro/prompts/code-review.md"
}

install_codebase_scan() {
  echo ""
  echo "Codebase Scan:"
  fetch ".github/workflows/kiro-codebase-scan.yml"
  fetch ".kiro/agents/codebase-scanner.json"
  fetch ".kiro/prompts/codebase-scan.md"
}

install_open_issue() {
  echo ""
  echo "Open Issue:"
  fetch ".github/workflows/open-issue-from-review.yml"
}

update_gitignore() {
  local gi="$TARGET_DIR/.gitignore"
  if [ -f "$gi" ]; then
    grep -qxF '.kiro/reviews/' "$gi" || echo '.kiro/reviews/' >> "$gi"
  else
    echo '.kiro/reviews/' > "$gi"
  fi
  echo ""
  echo "  ✓ .gitignore (added .kiro/reviews/)"
}

# --- Install ---

NEED_SHARED=false

if should_install "pr-review"; then
  NEED_SHARED=true
  install_pr_review
fi

if should_install "codebase-scan"; then
  NEED_SHARED=true
  install_codebase_scan
fi

if should_install "open-issue"; then
  install_open_issue
fi

if $NEED_SHARED; then
  install_shared
fi

update_gitignore

echo ""
echo "Done! Next steps:"
echo "  1. Add KIRO_API_KEY to repo secrets (Settings → Secrets → Actions)"
echo "  2. Commit the new files"
if should_install "open-issue"; then
  echo "  3. Ensure the open-issue workflow is on your default branch"
  echo "     (issue_comment events only trigger from the default branch)"
fi
echo ""
echo "Docs: https://github.com/$REPO#readme"
