#!/usr/bin/env bash
set -euo pipefail

# Kiro Code Review — Installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/briananderson1222/kiro-code-review/main/install.sh | bash
#   curl -fsSL ... | bash -s -- --component pr-review
#   ./install.sh --dir /path/to/repo --pr

REPO="briananderson1222/kiro-code-review"
BRANCH="main"
TARGET_DIR="${TARGET_DIR:-.}"
SOURCE_DIR=""
COMPONENTS=()
OPEN_PR=false

usage() {
  cat <<EOF
Kiro Code Review Installer

Usage: install.sh [OPTIONS]

Options:
  --dir <path>         Target repo directory (default: git root or cwd)
  --source <path>      Install from local directory instead of GitHub
  --pr                 Create a branch, commit, and open a PR
  --component <name>   Install specific component (repeatable). Options:
                         pr-review     — PR review workflow + agents
                         codebase-scan — Codebase scan workflow + agent
                         open-issue    — /open-issue slash command workflow
                         all           — Everything (default)
  --help               Show this help

Examples:
  install.sh                                    # Install everything
  install.sh --pr                               # Install and open a PR
  install.sh --component pr-review              # Just PR review
  install.sh --component pr-review --pr         # PR review + open PR
  install.sh --source ./kiro-code-review        # From local clone
EOF
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dir) TARGET_DIR="$2"; shift 2 ;;
    --source) SOURCE_DIR="$2"; shift 2 ;;
    --pr) OPEN_PR=true; shift ;;
    --component) COMPONENTS+=("$2"); shift 2 ;;
    --help) usage ;;
    *) echo "Unknown option: $1"; usage ;;
  esac
done

[[ ${#COMPONENTS[@]} -eq 0 ]] && COMPONENTS=("all")

# Auto-detect git repo root when no explicit --dir
if [ "$TARGET_DIR" = "." ]; then
  ROOT=$(git rev-parse --show-toplevel 2>/dev/null || true)
  [ -n "$ROOT" ] && TARGET_DIR="$ROOT"
fi

TARGET_DIR=$(cd "$TARGET_DIR" && pwd)
echo "Installing to: $TARGET_DIR"

if $OPEN_PR; then
  cd "$TARGET_DIR"
  if ! git rev-parse --is-inside-work-tree &>/dev/null; then
    echo "Error: --pr requires a git repository"
    exit 1
  fi
  BRANCH_NAME="feat/kiro-code-review"
  git checkout -b "$BRANCH_NAME" 2>/dev/null || git checkout "$BRANCH_NAME"
  echo "On branch: $BRANCH_NAME"
fi

fetch() {
  local path="$1"
  local dest="$TARGET_DIR/$path"
  mkdir -p "$(dirname "$dest")"
  if [ -n "$SOURCE_DIR" ]; then
    cp "$SOURCE_DIR/$path" "$dest"
  else
    curl -fsSL "https://raw.githubusercontent.com/$REPO/$BRANCH/$path" -o "$dest"
  fi
  echo "  ✓ $path"
}

should_install() {
  local component="$1"
  for c in "${COMPONENTS[@]}"; do
    [[ "$c" == "all" || "$c" == "$component" ]] && return 0
  done
  return 1
}

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

$NEED_SHARED && install_shared
update_gitignore

# --- PR ---

if $OPEN_PR; then
  cd "$TARGET_DIR"
  INSTALLED=$(IFS=,; echo "${COMPONENTS[*]}")
  git add -A
  git commit -m "feat: add Kiro code review ($INSTALLED)

Installed via kiro-code-review install.sh"
  git push -u origin "$BRANCH_NAME"

  PR_BODY="Adds AI-powered code review via [Kiro CLI](https://kiro.dev/docs/cli/).

## Components installed
$(should_install "pr-review" && echo "- **PR Review** — runs on every PR, submits formal GitHub reviews")
$(should_install "codebase-scan" && echo "- **Codebase Scan** — on-demand full-repo scan, creates summary issue")
$(should_install "open-issue" && echo "- **Open Issue** — \`/open-issue\` slash command to track findings")

## Setup required
1. Add \`KIRO_API_KEY\` to repo secrets (Settings → Secrets → Actions)"

  PR_URL=$(gh pr create --title "feat: add Kiro code review" --body "$PR_BODY")
  echo ""
  echo "PR opened: $PR_URL"
else
  echo ""
  echo "Done! Next steps:"
  echo "  1. Add KIRO_API_KEY to repo secrets (Settings → Secrets → Actions)"
  echo "  2. Commit the new files"
  if should_install "open-issue"; then
    echo "  3. Ensure the open-issue workflow is on your default branch"
  fi
fi

echo ""
echo "Docs: https://github.com/$REPO#readme"
