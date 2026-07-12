#!/usr/bin/env node
// Post a normalized review markdown file as a PR review comment.
// Node replacement for the former post-pr-review.sh.
import { spawnSync } from "node:child_process";
import { statSync } from "node:fs";

const reviewFile = process.argv[2] || "review-output.md";
const { GITHUB_REPOSITORY, PR_NUMBER } = process.env;

if (!GITHUB_REPOSITORY || !PR_NUMBER) {
  console.error("GITHUB_REPOSITORY and PR_NUMBER are required.");
  process.exit(2);
}

let size = 0;
try {
  size = statSync(reviewFile).size;
} catch {
  size = 0;
}
if (size === 0) {
  console.error(`Review output file is empty: ${reviewFile}`);
  process.exit(2);
}

const result = spawnSync(
  "gh",
  ["pr", "review", PR_NUMBER, "--repo", GITHUB_REPOSITORY, "--comment", "--body-file", reviewFile],
  { stdio: "inherit" },
);
process.exit(result.status ?? 1);
