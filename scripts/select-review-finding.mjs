import fs from "node:fs";

const [file, number] = process.argv.slice(2);

if (!file || !number) {
  console.error("Usage: node scripts/select-review-finding.mjs <review-findings.json> <number>");
  process.exit(2);
}

let review;
try {
  const body = file === "-" ? fs.readFileSync(0, "utf8") : fs.readFileSync(file, "utf8");
  review = JSON.parse(body);
} catch (error) {
  console.error(`Invalid review JSON: ${error.message}`);
  process.exit(2);
}

const findings = Array.isArray(review) ? review : review.findings;
if (!Array.isArray(findings)) {
  console.error("Review JSON must contain a findings array.");
  process.exit(2);
}

const index = Number(number) - 1;
if (!Number.isInteger(index) || index < 0) {
  console.error("Finding number must be a positive integer.");
  process.exit(2);
}

const finding = findings[index];
if (!finding) {
  console.error(`Finding #${number} not found. Review has ${findings.length} finding(s).`);
  process.exit(1);
}

const shortDescription = finding.description.slice(0, 80);
const location = finding.line ? `${finding.file}#L${finding.line}` : finding.file;

console.log(JSON.stringify({
  total: findings.length,
  title: `Code review: ${shortDescription}`,
  shortDescription,
  body: [
    `**Severity:** ${finding.severity}`,
    `**Confidence:** ${finding.confidence}`,
    `**File:** \`${location}\``,
    `**Source:** ${finding.source}`,
    "",
    finding.description
  ].join("\n"),
  finding
}, null, 2));
