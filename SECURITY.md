# Security Policy

## Supported versions

Boo is pre-1.0 and moves quickly. Security fixes land on the latest release
and on `main`; older tagged versions are not separately patched.

## Reporting a vulnerability

Please report security issues **privately** rather than opening a public issue.

- Use GitHub's [private vulnerability reporting](https://github.com/briananderson1222/boo/security/advisories/new)
  (Security → Advisories → Report a vulnerability), or
- email the maintainer at the address on their GitHub profile.

Please include: what the issue is, how to reproduce it, the affected version or
commit, and the impact you observed. A proof-of-concept helps but is not
required.

You can expect an acknowledgement within a few days. Once a fix is available,
it will be released and the reporter credited (unless you prefer otherwise).

## Scope and things to know

Boo runs AI-agent and shell commands on your machine on a schedule, so a few
behaviors are intentional and not vulnerabilities on their own:

- **`--runner shell` / `--command`** run arbitrary shell commands by design —
  they are the job owner's own configuration.
- **`--trust-all-tools` / `--trust-tools`** grant tool permissions to the agent
  as the job owner explicitly requests.

Some hardening is deliberately opt-in or local-machine-scoped:

- **`boo://` deep links** can trigger jobs. `boo://run` and `boo://resume`
  require per-job `--allow-url-trigger` (default off) precisely because any web
  page can open such a link.
- **`~/.boo`** (job prompts, config with webhook URLs, run transcripts) is
  created `0700` with `0600` files on Unix.

Reports about these being *ineffective* (e.g. a way to bypass the
`allow_url_trigger` gate, or files created world-readable) are in scope and
welcome.
