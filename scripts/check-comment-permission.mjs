const allowedPermissions = new Set(["admin", "maintain", "write"]);

const permission = (process.argv[2] || "").trim().toLowerCase();

if (!permission) {
  console.error("Usage: node scripts/check-comment-permission.mjs <permission>");
  process.exit(2);
}

if (allowedPermissions.has(permission)) {
  process.exit(0);
}

console.error(`Comment author permission '${permission}' is not allowed for this command.`);
process.exit(1);
