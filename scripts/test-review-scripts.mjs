#!/usr/bin/env node
// Self-contained smoke tests for the Kiro review helper scripts.
// Node-only, no external dependencies. Node replacement for the former
// test-review-scripts.sh.
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const tmp = mkdtempSync(join(tmpdir(), "kiro-review-tests-"));

let passCount = 0;
let failCount = 0;
const pass = (d) => {
  console.log(`PASS: ${d}`);
  passCount++;
};
const fail = (d) => {
  console.log(`FAIL: ${d}`);
  failCount++;
};
const assertEq = (d, expected, actual) =>
  expected === actual ? pass(d) : fail(`${d} (expected: ${expected}, actual: ${actual})`);
const assertExit = (d, expected, actual) =>
  expected === actual ? pass(d) : fail(`${d} (expected exit ${expected}, actual exit ${actual})`);
const assertContains = (d, haystack, needle) =>
  haystack.includes(needle) ? pass(d) : fail(`${d} (expected output to contain: ${needle})`);

const node = (args, input) =>
  spawnSync("node", args, { encoding: "utf8", input });

try {
  // Test 1: A8 repro — visible finding numbering must match selection.
  // A LOW finding appears first in the source data, then a HIGH finding. The
  // rendered review shows blockers first, so displayed #1 must be the HIGH
  // finding and selecting #1 must return that same HIGH finding.
  const raw = join(tmp, "raw-review.md");
  writeFileSync(
    raw,
    'Some review text.\n\n<!-- REVIEW_DATA: [{"severity":"LOW","confidence":"low","file":"src/a.rs","line":1,"description":"low finding","source":"kiro"},{"severity":"HIGH","confidence":"high","file":"src/b.rs","line":2,"description":"high finding","source":"kiro"}] -->\n',
  );
  const findings = join(tmp, "review-findings.json");
  const outMd = join(tmp, "review-output.md");
  node([join(scriptDir, "normalize-review-output.mjs"), raw, "--json", findings, "--markdown", outMd]);
  assertContains(
    "rendered markdown lists HIGH finding first",
    readFileSync(outMd, "utf8"),
    "1. **[HIGH]** high finding",
  );

  const selected = node([join(scriptDir, "select-review-finding.mjs"), findings, "1"]);
  const finding = JSON.parse(selected.stdout).finding;
  assertEq("select-review-finding #1 returns the displayed HIGH finding", "high finding", finding.description);
  assertEq("select-review-finding #1 finding severity is HIGH", "HIGH", finding.severity);

  // Test 2: empty findings path.
  const rawEmpty = join(tmp, "raw-review-empty.md");
  writeFileSync(rawEmpty, "Nothing to see here.\n\n<!-- REVIEW_DATA: [] -->\n");
  const findingsEmpty = join(tmp, "review-findings-empty.json");
  node([
    join(scriptDir, "normalize-review-output.mjs"),
    rawEmpty,
    "--json", findingsEmpty,
    "--markdown", join(tmp, "review-output-empty.md"),
  ]);
  const emptySel = node([join(scriptDir, "select-review-finding.mjs"), findingsEmpty, "1"]);
  assertExit("select-review-finding on empty findings exits 1", 1, emptySel.status);
  assertContains(
    "select-review-finding on empty findings prints a clean message",
    emptySel.stdout + emptySel.stderr,
    "not found",
  );

  // Test 3: malformed finding missing required fields.
  const malformed = join(tmp, "malformed-findings.json");
  writeFileSync(malformed, '{"findings": [{"severity": "HIGH"}]}\n');
  const malformedSel = node([join(scriptDir, "select-review-finding.mjs"), malformed, "1"]);
  assertExit("select-review-finding on malformed finding exits 2", 2, malformedSel.status);
  assertContains(
    "select-review-finding on malformed finding prints a clean message",
    malformedSel.stdout + malformedSel.stderr,
    "missing required fields",
  );

  // Test 4: findings JSON is not an array.
  const notArray = join(tmp, "not-an-array.json");
  writeFileSync(notArray, '{"findings": "oops"}\n');
  const notArraySel = node([join(scriptDir, "select-review-finding.mjs"), notArray, "1"]);
  assertExit("select-review-finding on non-array findings exits 2", 2, notArraySel.status);
  assertContains(
    "select-review-finding on non-array findings prints a clean message",
    notArraySel.stdout + notArraySel.stderr,
    "findings array",
  );

  // Test 5: parse-review-command with a missing --file path.
  const missingFile = node([
    join(scriptDir, "parse-review-command.mjs"),
    "/open-issue",
    "--file",
    join(tmp, "does-not-exist.txt"),
  ]);
  assertExit("parse-review-command on missing file exits 2", 2, missingFile.status);
  assertContains(
    "parse-review-command on missing file prints a clean message",
    missingFile.stdout + missingFile.stderr,
    "Unable to read comment body file",
  );

  // Test 6: parse-review-command happy path.
  const parsed = node([join(scriptDir, "parse-review-command.mjs"), "/open-issue", "/open-issue 3"]);
  assertEq("parse-review-command parses a valid command", "3", parsed.stdout.trim());
} finally {
  rmSync(tmp, { recursive: true, force: true });
}

console.log(`\nResults: ${passCount} passed, ${failCount} failed.`);
process.exit(failCount > 0 ? 1 : 0);
