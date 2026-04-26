import fs from "node:fs";

const file = process.argv[2];

if (!file) {
  console.error("Usage: node scripts/fail-on-critical.mjs <review-findings.json>");
  process.exit(2);
}

let review;
try {
  review = JSON.parse(fs.readFileSync(file, "utf8"));
} catch (error) {
  console.error(`Invalid review JSON: ${error.message}`);
  process.exit(2);
}

const findings = Array.isArray(review) ? review : review.findings;
if (!Array.isArray(findings)) {
  console.error("Review JSON must contain a findings array.");
  process.exit(2);
}

const critical = findings.filter((finding) => finding.severity === "CRITICAL");

if (critical.length > 0) {
  console.error(`Critical review findings: ${critical.length}`);
  for (const finding of critical) {
    console.error(`- ${finding.file}:${finding.line} ${finding.description}`);
  }
  process.exit(1);
}

console.log(`No critical findings. Total findings: ${findings.length}`);
