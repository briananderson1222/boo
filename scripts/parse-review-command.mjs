import fs from "node:fs";

const [command, bodyArg = "", filePath] = process.argv.slice(2);

if (!command) {
  console.error("Usage: node scripts/parse-review-command.mjs </command> <comment-body>|--file <path>");
  process.exit(2);
}

const body = bodyArg === "--file" ? fs.readFileSync(filePath, "utf8") : bodyArg;
const escaped = command.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const match = body.match(new RegExp(`^${escaped}\\s+(\\d+)\\s*$`));

if (match) {
  console.log(match[1]);
}
