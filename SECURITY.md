# Security policy

mdh drives devices over adb, builds and installs apps, and runs an MCP server that coding agents call. If you find
a way for any of that to do something it shouldn't — run commands it wasn't asked to, leak typed secrets, reach a
device or file outside what it was pointed at — please report it privately.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting: **Security → Report a vulnerability** on
[the repository](https://github.com/mobile-dev-harness/mobile-dev-harness/security/advisories/new). Please don't
open a public issue for it.

Include what you ran (the `mdh` version, the command or MCP call), what happened, and what you expected. You'll get
an answer within a week; fixes are released as a patch version and credited in the advisory unless you'd rather
not be named.

## Supported versions

The project is in 0.x: only the latest release gets fixes.
