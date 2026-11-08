# Security Policy

Hangar starts programs on your machine and passes your input to them, so security reports are taken
seriously.

## Reporting a vulnerability

Please **do not open a public issue.** Use GitHub's
[private vulnerability reporting](../../security/advisories/new) instead. Include the Hangar version,
your operating system, steps to reproduce and the impact you expect.

You will get an initial response within a week. Once a fix is released, the advisory is published with
credit to you unless you prefer otherwise.

## Supported versions

Only the latest release receives security fixes. Hangar updates itself; updates are signed and
verified before they are installed.

## Scope

In scope, for example:

- a working folder, profile or update source that makes Hangar run code you did not ask for
- the frontend reaching backend capabilities that were not explicitly granted
- secrets leaking into logs, config files or saved scrollback beyond what the settings describe
- bypassing the update signature check

Out of scope: vulnerabilities in the agent CLIs themselves; please report those to their vendors.
