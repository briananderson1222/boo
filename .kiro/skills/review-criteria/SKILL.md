---
name: review-criteria
description: Shared code review definitions — severity levels, confidence scoring, issue format, review passes, and discard rules. Use when performing code reviews, scoring findings, or scanning codebases.
---

# Review Criteria

## Severity

Severity measures impact — how bad is this if it's real?

| Severity | Meaning | Examples |
|---|---|---|
| CRITICAL | Security vulnerability, data loss, auth bypass | SQL injection, hardcoded secrets, path traversal, missing auth check |
| HIGH | Bug that will break functionality | Null access, logic error, resource leak, race condition, missing error handling |
| MEDIUM | Maintainability or correctness concern | Duplication, unclear naming, deep nesting, missing validation on non-critical path |
| LOW | Style, convention, minor improvement | Formatting, naming preference, minor refactor opportunity |

## Confidence

Confidence measures certainty — is this actually a real issue?

| Score | Meaning |
|---|---|
| 90–100 | Confirmed. Can point to the exact line and explain the failure mode. |
| 70–89 | Very likely real. Evidence is strong but depends on runtime context. |
| 50–69 | Plausible. Could be real but might be a false positive. |
| 25–49 | Uncertain. Might be intentional or context-dependent. |
| 0–24 | Probably not real. Pre-existing, stylistic, or already caught by tooling. |

## Issue Format

Every finding must include:

```json
{
  "file": "<path>",
  "line": "<start>-<end>",
  "description": "<brief description>",
  "source": "bug | standards | git-history | code-comment | security | maintainability",
  "severity": "CRITICAL | HIGH | MEDIUM | LOW",
  "confidence": 0-100
}
```

## Review Passes

### Pass A — Standards Compliance
Check against project standards (CLAUDE.md, AGENTS.md, CONTRIBUTING.md, steering files). Only flag violations the standards explicitly call out.

### Pass B — Bug Scan
Logic errors, off-by-one, null/undefined access, resource leaks, race conditions. Focus on high-impact bugs, ignore style.

### Pass C — Security Scan
Injection (SQL, XSS, command), path traversal, hardcoded secrets/credentials, missing auth checks, CSRF, unsafe deserialization.

### Pass D — Git History Context
Recently fixed bugs being reintroduced, reverted changes re-applied, modifications contradicting recent intentional changes.

### Pass E — Code Comment Compliance
Changes violating `// WARNING:`, `// NOTE:`, `// IMPORTANT:` annotations or doc comments explaining invariants.

## Discard Rules

Discard any finding that is:
- Pre-existing (not introduced by these changes — PR mode only)
- Something a linter, typechecker, or CI would catch
- A general quality concern not explicitly required by project standards
- On lines the author did not modify (PR mode only)
- An intentional change consistent with the purpose of the changes
