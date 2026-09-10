# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Otto — a Rust-only, MCP-first terminal coding agent. Vision and scope live in `PRD.md`; the SPP wire format is in `crates/otto-protocol/SPEC.md`. Developer-facing details (slash commands, env vars, on-disk paths) are in `README.md`.

## Common commands

```bash
# Build everything. Required even for TUI-only work, because the TUI spawns
# `otto-tool-fs` at runtime and needs that binary to exist.
cargo build

# Run the TUI (default workspace member).
cargo run -p otto

# Tests.
cargo test --workspace
cargo test -p otto-host                     # single crate
cargo test -p otto-host -- name::of::test   # single test

# Continuous check / clippy via bacon.
bacon                # cargo check (default)
bacon clippy-all     # clippy across the workspace
bacon test           # cargo test

# Headless smoke-test (needs a provider — see README "Running providers as
# standalone MCP servers"):
cargo run -p otto-host --example headless -- "list my Cargo.toml"
```

`.env` and `.env.local` at repo root are auto-loaded on startup.

## Architecture (the parts you can't see by reading one file)

The core abstraction is "everything is MCP-shaped." A provider is just a `ProviderHandler` from `otto-mcp`; a tool is just a stdio MCP server. Each can be wrapped in a binary (`provider-anthropic` ships `otto-anthropic`, `tool-fs` ships `otto-tool-fs`), but providers are linked **in-process by default** via `InProcessProviderClient` — the binary form exists for wire-protocol debugging.

### Turn loop

`Host` (in `otto-host`) holds a `Box<dyn ProviderClient>` plus a `ToolRegistry`. `Host::run_turn_streaming` loops `provider.complete` → `tool_registry.call` until the model emits `end_turn`, forwarding `StreamEvent`s out as it goes. The tool-use loop, session state, and project-context loading (`OTTO.md` if present) all live here — the TUI is a thin shell on top.

### Host swap

The TUI keeps the active host as `Arc<RwLock<Option<Arc<Host>>>>`. Per-turn worker tasks **clone the `Arc<Host>` under a brief read lock** and then drop the guard before any `.await` — never hold the `RwLock` across awaits. `/connect` swaps the slot atomically. See `crates/otto/src/app.rs` and `tui.rs`.

### Provider transport split

- **In-process (default):** `InProcessProviderClient` wraps a `ProviderHandler` directly — no HTTP, no serialization round-trip.
- **MCP HTTP (opt-in):** when `OTTO_PROVIDER_URL` is set, the TUI connects to a remote provider over `rmcp`'s Streamable HTTP transport instead.

The `Host` only sees `Box<dyn ProviderClient>` and doesn't know which path is in use. There is **no provider registry inside the host**.

### Tool transport

Bundled tools are stdio child processes owned by `ToolRegistry` and reaped on shutdown. User-configured `/mcp` entries can add either more stdio tool servers or remote Streamable HTTP MCP servers alongside them. The TUI bakes in one (`otto-tool-fs`, locatable via `$PATH` or `OTTO_TOOL_FS_BIN`); additional built-in tools can still be added via `HostConfig::with_tool` in `crates/otto/src/main.rs`.

### `rmcp` ProgressDispatcher gotcha

`subscriber.next()` from `rmcp`'s `ProgressDispatcher` does **not** auto-close when the RPC completes. Forwarder tasks that pump progress notifications must `JoinHandle::abort()` after the request future resolves, or the caller's mpsc waiter will deadlock. This pattern is used in `provider-anthropic`/`provider-gemini` streaming paths.

## Workspace map (for navigation)

| Crate | Owns |
|---|---|
| `crates/otto` | TUI binary, `/connect`, `/mcp`, file picker, transcript persistence, the `PROVIDERS` registry. |
| `crates/otto-host` | `Host`, `ToolRegistry`, session state, project context (`OTTO.md`). |
| `crates/otto-protocol` | Pure types: `CompleteRequest`, `CompleteResponse`, `StreamEvent`, content blocks. |
| `crates/otto-mcp` | `ProviderClient` / `ProviderHandler` traits and the `InProcessProviderClient` bridge. |
| `crates/provider-anthropic`, `crates/provider-gemini` | Provider impls (libraries) + thin `otto-<vendor>` MCP-server binaries. |
| `crates/tool-fs` | `read_file` / `write_file` / `list_dir` / `glob` as a stdio MCP server. |

## Extending

The README has step-by-step recipes — the short version:

- **New provider:** mirror `provider-gemini`, implement `ProviderHandler`, then append a `ProviderSpec` entry (and `build_*` factory) in `crates/otto/src/providers.rs::PROVIDERS`. The host needs no changes.
- **New tool:** mirror `crates/tool-fs` (stdio MCP server) and register it via `HostConfig::with_tool` in `crates/otto/src/main.rs`.
- **New user-configured MCP server:** use `/mcp` or add a `[[mcp_servers]]` entry in `~/.otto/config.toml`; secrets live in the keyring under `mcp:<server name>`.

## Persistence

- Transcripts: `~/.otto/transcripts/<unix>.json` (auto on `TurnComplete`, manual on `/save`).
- API keys and MCP-server secrets: OS keyring under service `otto`, account `<provider id>` or `mcp:<server name>`. Never written to disk in plaintext — `/connect` and `/mcp` are the only writers.

## Claude Code skills

This repo's own skills live in **two committed directories**, and neither is gitignored:

- `.github/skills/<name>/SKILL.md` holds the **canonical body** of this repo's own workflow skills — `otto-development` (plus its `agent-prompts.md` companion) and `creating-github-issues` — in the Copilot CLI's skill format. These are the single source of truth for the workflows themselves.
- `.claude/skills/<name>/SKILL.md` is what **Claude Code** discovers (it reads project skills from nowhere else), and holds two kinds of entry: Claude-Code-native skills with no canonical counterpart (`rust-engineer`, `tui-engineer`), and **adapter stubs** that delegate to a canonical body (`otto-development`, `creating-github-issues`).

An adapter stub is a small real file carrying three things: `name`/`description` frontmatter copied verbatim from the canonical skill (so both hosts trigger on exactly the same requests), a `> **Canonical body:**` line naming the canonical file(s) with an instruction to read them before acting, and — for `otto-development` only — a Copilot→Claude Code host-mechanism translation table. It is deliberately neither a symlink nor a copy: a symlink cannot carry that translation (the canonical body is written for the orchestrating Copilot CLI and says so) and would need `core.symlinks=true` on a Windows checkout, while a copy would duplicate ~100KB of the repo's most-edited process document and drift on every workflow edit.

**Adding a new skill under `.github/skills/` requires a `.claude/skills/<name>/SKILL.md` adapter stub in the same change.** `.github/scripts/check-claude-skill-stubs.sh`, run by CI's `lint` job, fails the build if a stub is missing, if a stub's `name`/`description` have drifted from the canonical, or if a stub points at a path that no longer exists. Run it locally before pushing with `bash .github/scripts/check-claude-skill-stubs.sh`.

Precedence, for a repo-authored skill: the **canonical body governs the workflow** and the **stub governs only host-mechanism translation** — a stub never overrides a rule, it only says how to carry it out with Claude Code's tools. Read the canonical body before acting; the stub is not a summary of it.

Generic or personal Claude Code skills pulled from `~/.claude/skills/` (e.g. a symlinked `creating-tickets`) are **not** added under this repo's `.claude/skills/` — they stay personal and machine-local in the contributor's home directory, never checked into this tree. Nothing needs to be gitignored as a result, since a correctly-scoped personal skill is never placed under the repo in the first place.

Any tracker-related skill (ticket creation, issue triage, etc.) used while working in this repo must defer to this repo's actual tracker — **GitHub Issues**, via `gh issue`/`gh pr` and the conventions in `.github/skills/otto-development` — never a JIRA-first or other generic tracker abstraction a personal skill might assume.

If a *personal* skill from `~/.claude/skills/` overlaps a repo skill in purpose (e.g. a personal ticket-creation skill vs. `creating-github-issues`' own conventions), this repo's own skill governs while working in this repo; the personal skill's generic guidance yields.
