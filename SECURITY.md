# Security model

Otto runs language-model-driven tool spawns. The sandbox layer is
designed to contain a compromised or misbehaving tool — including
third-party MCP servers — without crippling normal use.

## What the sandbox covers

As of v0.7, when `enabled = true` (the default on Linux and macOS):

- **Writes** outside the project root are denied.
- **Network** is denied for tool spawns by default. `tool-bash` is the one
  exception in v0.7.0: it currently inherits `allow_net = true` via a
  built-in per-tool fallback so common commands (`curl`, `cargo`, `npm`)
  work out of the box. A runtime permission prompt
  (`Once`/`Always-this-session`/`Deny`) replaces the static fallback in a
  follow-up release tracked under issue #17.
- **Reads** of well-known sensitive paths under `$HOME` are denied. The
  canonical list lives in `crates/otto-host/src/sensitive_paths.rs`
  (`SENSITIVE_HOME_STEMS`). On every supported platform it currently
  includes `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.netrc`, `~/.config/gh`,
  `~/.mozilla`, and `~/.config/google-chrome`. macOS additionally covers
  the Firefox and Chrome profile directories under `~/Library/Application
  Support`.

  Entries in `SENSITIVE_HOME_STEMS` that do not exist on disk at sandbox
  setup time are filtered out by the helper that resolves the list to
  real paths — so e.g. `~/.mozilla` is listed for all platforms but
  contributes a sandbox rule only when that directory actually exists
  for the running user (typical on Linux, uncommon on a default macOS
  install).

## What the sandbox does not cover

- **Windows** has no sandbox layer yet. Tools run unwrapped with a
  one-time warning. AppContainer + Job Objects support is on the v0.8
  roadmap.
- **Reads outside the sensitive list.** Tools can still read arbitrary
  files under `$HOME` that are not on the sensitive-path list. The list
  is conservative; open an issue if a path you treat as secret isn't
  covered.
- **Domain-level network policy.** `allow_net = true` opens the full
  network. A bundled allowlist (e.g. `crates.io`, `registry.npmjs.org`)
  is on the v0.8 roadmap.
- **In-process exfiltration via the host itself.** A compromised provider
  binary can read its own process memory and the data the host has
  already loaded. Provider trust is out of scope for the sandbox layer.

## How to disable

Interactive:

```
/sandbox off
```

Or in `~/.otto/sandbox.toml`:

```toml
enabled = false
```

A user who explicitly sets `enabled = false` (or `enabled = true`)
keeps that value across upgrade. The in-memory representation
(`SandboxMode`) distinguishes the explicit on/off choice from "no
preference declared," so future splash and TUI surfaces can suppress
the v0.7-style nag line for users who have made an explicit
selection.

## Reporting

Security reports: please use GitHub's Security Advisories feature on the
repository, or contact the maintainer listed in the workspace
`Cargo.toml`. Do not file security issues as public GitHub issues.
