---
name: Bug report
about: Something isn't working as documented
title: ''
labels: bug
assignees: ''
---

**What happened**
A clear description of the bug.

**What you expected**
What you expected to happen instead.

**Steps to reproduce**
The exact `boo` commands (redact any secrets/prompts). For example:

```
boo add --name repro --every 1h --command "..." --dir ...
boo run repro
```

**Environment**
- OS + version:
- `boo --version`:
- Installed via (`cargo build`, release binary, `boo install`):
- Runner (kiro / shell):

**Logs / output**
Relevant output from `boo logs <job>`, the daemon, or the terminal. Redact
prompts, tokens, and webhook URLs.
