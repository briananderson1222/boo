#!/usr/bin/env node
// Run a Kiro review agent headlessly over the PR diff and normalize the output.
// Node replacement for the former run-kiro-review.sh (keeps scripts/ single-language).
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));

let agent = "code-reviewer";
let baseRef = "origin/main";
let reviewKind = "PR review";
let outputMarkdown = "review-output.md";
let outputJson = "review-findings.json";
let strict = false;

const args = process.argv.slice(2);
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  switch (a) {
    case "--agent": agent = args[++i]; break;
    case "--base-ref": baseRef = args[++i]; break;
    case "--review-kind": reviewKind = args[++i]; break;
    case "--output-markdown": outputMarkdown = args[++i]; break;
    case "--output-json": outputJson = args[++i]; break;
    case "--strict": strict = true; break;
    case "--help":
      console.log(
        "Usage: run-kiro-review.mjs --agent <agent> --base-ref <ref> --review-kind <name> --output-markdown <path> --output-json <path> [--strict]",
      );
      process.exit(0);
      break;
    default:
      console.error(`Unknown argument: ${a}`);
      process.exit(2);
  }
}

if (!process.env.KIRO_API_KEY) {
  console.error("KIRO_API_KEY is required for Kiro headless mode.");
  process.exit(2);
}

const version = spawnSync("kiro-cli", ["--version"], { encoding: "utf8" });
if (version.error) {
  console.error("kiro-cli is not available on PATH.");
  process.exit(2);
}
process.stdout.write(version.stdout || "");

// Fall back to the previous commit if the base ref can't be resolved.
if (spawnSync("git", ["rev-parse", "--verify", baseRef], { stdio: "ignore" }).status !== 0) {
  baseRef = "HEAD~1";
}

const MAX_BUFFER = 64 * 1024 * 1024;
const tmp = mkdtempSync(join(tmpdir(), "kiro-review-"));
try {
  const diffFile = join(tmp, "pr.diff");
  const rawOutput = join(tmp, "raw-review.md");

  let diff = spawnSync("git", ["diff", `${baseRef}...HEAD`], { encoding: "utf8", maxBuffer: MAX_BUFFER });
  if (diff.status !== 0) {
    diff = spawnSync("git", ["diff", baseRef, "HEAD"], { encoding: "utf8", maxBuffer: MAX_BUFFER });
  }
  writeFileSync(diffFile, diff.stdout || "");

  // The diff is referenced by file path rather than embedded in the prompt so
  // large PRs can't exceed the process argument limit.
  const prompt = `Run a ${reviewKind} for this diff. The full unified diff has been written to the file at ${diffFile}. Use your read tool to read that file before evaluating; it is not included inline here. Return concise markdown findings.

Your final line is mandatory automation data. End with exactly one single-line HTML comment matching this shape:
<!-- REVIEW_DATA: [{"severity":"HIGH","confidence":"medium","file":"src/app.js","line":1,"description":"short actionable description","source":"kiro"}] -->

If there are no findings, the final line must be exactly:
<!-- REVIEW_DATA: [] -->

Do not omit REVIEW_DATA. Do not wrap REVIEW_DATA in a code fence.`;

  // The prompt is passed as a single argv element (no shell), so no quoting is
  // needed and metacharacters cannot break out.
  const kiro = spawnSync(
    "kiro-cli",
    ["chat", "--no-interactive", "--agent-engine", "v3", "--trust-tools=read,grep", "--agent", agent, prompt],
    { encoding: "utf8", maxBuffer: MAX_BUFFER, stdio: ["ignore", "pipe", "inherit"] },
  );
  if (kiro.status !== 0) {
    console.error(`kiro-cli exited with status ${kiro.status}`);
    process.exit(kiro.status || 1);
  }
  writeFileSync(rawOutput, kiro.stdout || "");

  const normalizeArgs = [
    join(scriptDir, "normalize-review-output.mjs"),
    rawOutput,
    "--json", outputJson,
    "--markdown", outputMarkdown,
    "--title", reviewKind,
  ];
  if (strict) normalizeArgs.push("--strict");
  const normalize = spawnSync("node", normalizeArgs, { stdio: "inherit" });
  if (normalize.status !== 0) process.exit(normalize.status || 1);

  process.stdout.write(readFileSync(outputMarkdown, "utf8"));
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
