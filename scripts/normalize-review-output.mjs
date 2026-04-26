import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const SEVERITIES = new Set(["CRITICAL", "HIGH", "MEDIUM", "LOW"]);
const CONFIDENCES = new Set(["high", "medium", "low"]);

export function normalizeReviewOutput(rawMarkdown, options = {}) {
  const strict = Boolean(options.strict);
  const rawMarkdownPath = options.rawMarkdownPath ?? null;
  const warnings = [];
  const payload = extractPayload(rawMarkdown, warnings);

  if (!payload.ok) {
    if (strict) {
      throw new Error(payload.error);
    }
    warnings.push(payload.error);
  }

  const findings = payload.ok
    ? normalizeFindings(payload.value, { strict, warnings })
    : [];
  const uniqueFindings = dedupeFindings(findings, warnings);
  const document = {
    findings: uniqueFindings,
    counts: countFindings(uniqueFindings),
    warnings,
    source: {
      rawMarkdownPath,
      normalizedAt: options.normalizedAt ?? new Date().toISOString(),
      strict
    }
  };

  return {
    document,
    markdown: canonicalMarkdown(rawMarkdown, document)
  };
}

function extractPayload(rawMarkdown, warnings) {
  const comments = [...rawMarkdown.matchAll(/<!--\s*REVIEW_DATA:\s*([\s\S]*?)\s*-->/g)];
  if (comments.length > 1) {
    warnings.push(`Found ${comments.length} REVIEW_DATA payloads; using the last one.`);
  }

  const rawPayload = comments.at(-1)?.[1] ?? extractFencedJson(rawMarkdown);
  if (!rawPayload) {
    return { ok: false, error: "Missing REVIEW_DATA payload." };
  }

  try {
    return { ok: true, value: JSON.parse(rawPayload) };
  } catch (error) {
    return { ok: false, error: `Invalid REVIEW_DATA JSON: ${error.message}` };
  }
}

function extractFencedJson(rawMarkdown) {
  const match = rawMarkdown.match(/```(?:json)?\s*([\s\S]*?)```/i);
  return match?.[1]?.trim() || null;
}

function normalizeFindings(value, { strict, warnings }) {
  const rawFindings = Array.isArray(value) ? value : value?.findings;
  if (!Array.isArray(rawFindings)) {
    if (strict) {
      throw new Error("REVIEW_DATA must be an array or an object with a findings array.");
    }
    warnings.push("REVIEW_DATA was not an array or normalized object; using empty findings.");
    return [];
  }

  const findings = [];
  rawFindings.forEach((finding, index) => {
    try {
      findings.push(normalizeFinding(finding, index, strict, warnings));
    } catch (error) {
      if (strict) {
        throw error;
      }
      warnings.push(error.message);
    }
  });
  return findings;
}

function normalizeFinding(finding, index, strict, warnings) {
  if (!finding || typeof finding !== "object" || Array.isArray(finding)) {
    throw new Error(`Finding ${index + 1} is not an object.`);
  }

  const severity = normalizeSeverity(finding.severity, index);
  const confidence = normalizeConfidence(finding.confidence, index);
  const file = normalizeText(finding.file, `Finding ${index + 1} is missing file.`);
  const description = normalizeText(
    finding.description,
    `Finding ${index + 1} is missing description.`
  );
  const source = normalizeText(finding.source, `Finding ${index + 1} is missing source.`);
  const line = normalizeLine(finding.line, index, strict, warnings);

  return { severity, confidence, file, line, description, source };
}

function normalizeSeverity(value, index) {
  const severity = String(value ?? "").trim().toUpperCase();
  if (!SEVERITIES.has(severity)) {
    throw new Error(`Finding ${index + 1} has invalid severity: ${value ?? "missing"}.`);
  }
  return severity;
}

function normalizeConfidence(value, index) {
  const confidence = String(value ?? "").trim().toLowerCase();
  if (!CONFIDENCES.has(confidence)) {
    throw new Error(`Finding ${index + 1} has invalid confidence: ${value ?? "missing"}.`);
  }
  return confidence;
}

function normalizeText(value, errorMessage) {
  const text = String(value ?? "").trim();
  if (!text) {
    throw new Error(errorMessage);
  }
  return text;
}

function normalizeLine(value, index, strict, warnings) {
  if (value === null || value === undefined || value === "") {
    return null;
  }
  const line = Number(value);
  if (!Number.isInteger(line) || line < 1) {
    if (strict) {
      throw new Error(`Finding ${index + 1} has invalid line: ${value}.`);
    }
    warnings.push(`Finding ${index + 1} has invalid line: ${value}; normalized to null.`);
    return null;
  }
  return line;
}

function dedupeFindings(findings, warnings) {
  const seen = new Set();
  const unique = [];
  for (const finding of findings) {
    const key = [
      finding.severity,
      finding.confidence,
      finding.file,
      finding.line ?? "",
      finding.description,
      finding.source
    ].join("\u0000");
    if (seen.has(key)) {
      warnings.push(`Duplicate finding ignored: ${finding.file}:${finding.line ?? "?"}`);
      continue;
    }
    seen.add(key);
    unique.push(finding);
  }
  return unique;
}

function countFindings(findings) {
  const counts = { total: findings.length, critical: 0, high: 0, medium: 0, low: 0 };
  for (const finding of findings) {
    if (finding.severity === "CRITICAL") counts.critical += 1;
    if (finding.severity === "HIGH") counts.high += 1;
    if (finding.severity === "MEDIUM") counts.medium += 1;
    if (finding.severity === "LOW") counts.low += 1;
  }
  return counts;
}

function canonicalMarkdown(rawMarkdown, document) {
  const visible = rawMarkdown
    .replace(/<!--\s*REVIEW_DATA:\s*[\s\S]*?\s*-->/g, "")
    .trimEnd();
  const comment = `<!-- REVIEW_DATA: ${JSON.stringify(document)} -->`;
  return visible ? `${visible}\n\n${comment}\n` : `${comment}\n`;
}

function parseArgs(argv) {
  const args = {
    input: null,
    json: "review-findings.json",
    markdown: "review-output.md",
    strict: false
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--json") args.json = argv[++index];
    else if (arg === "--markdown") args.markdown = argv[++index];
    else if (arg === "--strict") args.strict = true;
    else if (arg === "--help") args.help = true;
    else if (!args.input) args.input = arg;
    else throw new Error(`Unexpected argument: ${arg}`);
  }
  return args;
}

function usage() {
  return "Usage: node scripts/normalize-review-output.mjs <raw-review.md> --json <path> --markdown <path> [--strict]";
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help || !args.input) {
    console.log(usage());
    process.exit(args.help ? 0 : 2);
  }

  const rawMarkdown = fs.readFileSync(args.input, "utf8");
  const result = normalizeReviewOutput(rawMarkdown, {
    strict: args.strict,
    rawMarkdownPath: args.input
  });

  fs.mkdirSync(path.dirname(args.json), { recursive: true });
  fs.mkdirSync(path.dirname(args.markdown), { recursive: true });
  fs.writeFileSync(args.json, `${JSON.stringify(result.document, null, 2)}\n`);
  fs.writeFileSync(args.markdown, result.markdown);

  if (process.env.GITHUB_OUTPUT) {
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `review_json_path=${args.json}\n`);
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `review_markdown_path=${args.markdown}\n`);
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `finding_count=${result.document.counts.total}\n`);
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `critical_count=${result.document.counts.critical}\n`);
  }

  for (const warning of result.document.warnings) {
    console.warn(`warning: ${warning}`);
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(2);
  });
}
