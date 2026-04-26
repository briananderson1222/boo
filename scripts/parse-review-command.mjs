const [command, body = ""] = process.argv.slice(2);

if (!command) {
  console.error("Usage: node scripts/parse-review-command.mjs </command> <comment-body>");
  process.exit(2);
}

const escaped = command.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const match = body.match(new RegExp(`^${escaped}\\s+(\\d+)\\s*$`));

if (match) {
  console.log(match[1]);
}
