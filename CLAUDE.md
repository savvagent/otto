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

- `.github/skills/<name>/` holds the **canonical body** of this repo's own workflow skills — `otto-development` (plus its `agent-prompts.md` companion) and `creating-github-issues` — in the Copilot CLI's skill format. These are the single source of truth for the workflows themselves.
- `.claude/skills/<name>/` is what **Claude Code** discovers (it reads project skills from nowhere else), and holds two kinds of entry: Claude-Code-native skills with no canonical counterpart (`rust-engineer`, `tui-engineer`), and **ports** of a canonical body (`otto-development`, `creating-github-issues`).

A port is an **adapted copy**, complete and self-sufficient: a Claude Code session reads it start to finish without ever opening `.github/skills/`. It is adapted rather than symlinked or copied verbatim because the canonical bodies are written for the orchestrating Copilot CLI and name tools Claude Code does not have — `agent_type`, `mode: "sync"`, `read_agent`, `ask_user`, a session-scoped `todos` table — so an unadapted 100KB of workflow would name an unexecutable tool at every dispatch step. (A committed symlink would also need `core.symlinks=true` on a Windows checkout, and this repo ships and tests on Windows.) The adaptation touches **only** host-mechanism references: `name`/`description` frontmatter is byte-identical so both hosts trigger on exactly the same requests, and every Non-Negotiable Rule, phase gate, fix-loop cap, review requirement, repo convention and Load-Bearing Invariant carries across unchanged. `creating-github-issues` needs no adaptation at all — it is plain `gh issue`/`gh label` usage — so its port diverges only by its "ported from" note.

**Edit the canonical file, mirror the edit into the port, then regenerate the record. Never edit a port in place** — an in-place edit is lost work, because the next re-port overwrites it, and it is the drift the check exists to catch. Each port carries a `> **Ported for Claude Code**` note at the top saying so. Mirroring is usually verbatim: a canonical edit that lands nowhere near an adapted line (a typo fix, a new paragraph) is copied into the port as-is. Only an edit that lands *on* an adapted line needs the host-mechanism adaptation re-applied there — re-porting from scratch on an unrelated canonical edit is the over-correction that widens the divergence this design keeps narrow.

The divergence is recorded, not trusted: `.github/skills/<name>/claude-port/<relative path>.diff` holds the expected `diff -u canonical ported` for each ported file, so a reviewer reads the port's entire divergence from the canonical in one place and any *unintended* divergence shows up as a diff-of-diffs. `.github/scripts/check-claude-skill-ports.sh`, run by CI's `lint` job, recomputes each diff and fails the build if it no longer matches the record — which catches a canonical edited without mirroring and a port edited directly, those being the same failure from two sides. It also fails on a missing port, a missing, empty or orphaned expected diff, an orphaned *port* (a rule Claude Code executes that the canonical does not contain), a port that is a verbatim copy, a port missing its "Ported for Claude Code" note, `name`/`description` drift (reported as its own failure, since it is what silently changes trigger behaviour), and on `.github/skills/` containing no skills at all.

Two limits of that coverage, worth knowing before you rely on it. **The check verifies the content of ports only.** A Claude-Code-native skill (`rust-engineer`, `tui-engineer`) has no canonical and so nothing to diverge from: its *content* is verified by human review alone, and should be reviewed as carefully as any other committed instruction Claude Code executes. What the check does enforce is that such a skill is **declared**: every directory under `.claude/skills/` must either be a port (have a canonical directory) or be named in the `NATIVE_SKILLS` allowlist at the top of the script, so adding one is a deliberate, reviewable line rather than something a directory drifts into when its canonical is deleted. Adding a native skill therefore means adding its name there in the same change — and a personal skill symlinked in from `~/.claude/skills/`, which this repo already forbids, now fails the build rather than passing silently. And **records are certified for GNU `diff` only.** macOS ships BSD `diff`, whose unified output differs in details, so a record regenerated there can be committed locally-green and rejected by CI, which runs GNU diffutils. On macOS, regenerate with GNU diff on `PATH` (`brew install diffutils`, then `gdiff`-first via `PATH="$(brew --prefix diffutils)/libexec/gnubin:$PATH"`) rather than debugging a mismatch that is really a toolchain difference.

Two commands, both run from the repo root:

```bash
bash .github/scripts/check-claude-skill-ports.sh            # verify — what CI runs
bash .github/scripts/check-claude-skill-ports.sh --update   # regenerate every record
```

`--update` is the only way records are produced: it writes them through the same function the verifier compares against, so generation and verification cannot disagree about the diff's form. It is not a bypass for the *structural* failures — a missing port, an orphan, a verbatim copy, a missing "Ported for Claude Code" note and frontmatter drift stay red under `--update`, because regenerating a record fixes none of them. It **is** the bypass for the mismatch failure, which is the one this whole arrangement exists to catch: edit a canonical, skip the mirroring, run `--update`, and you get a clean green build over a port that is missing the new text. So run it *after* mirroring the edit by hand. Reaching for it because the build is red is the wrong reflex, and nothing in the script can stop you.

**The check compares bytes, not meaning.** A green build says the port's divergence from the canonical is still exactly the divergence a reviewer approved; it says nothing about whether the adaptation is *correct*. Re-adapting a rewritten dispatch section is human judgement, and the check can only tell you that the section changed.

**Adding a new skill under `.github/skills/` requires a port under `.claude/skills/<name>/` and an expected diff under `.github/skills/<name>/claude-port/` in the same change** — one per Markdown file in the canonical directory **at any depth**, companions like `agent-prompts.md` and any `reference/` subdirectory included. A companion the port lacks is one Claude Code can never read.

Precedence, for a repo-authored skill: the **canonical body is the source of truth for the workflow**, and the **port is the version Claude Code actually executes**. A discrepancy between them is therefore a bug in the port, not a decision — fix the port to match, rather than treating its wording as a local override.

Generic or personal Claude Code skills pulled from `~/.claude/skills/` (e.g. a symlinked `creating-tickets`) are **not** added under this repo's `.claude/skills/` — they stay personal and machine-local in the contributor's home directory, never checked into this tree. Nothing needs to be gitignored as a result, since a correctly-scoped personal skill is never placed under the repo in the first place.

Any tracker-related skill (ticket creation, issue triage, etc.) used while working in this repo must defer to this repo's actual tracker — **GitHub Issues**, via `gh issue`/`gh pr` and the conventions in `.github/skills/otto-development` — never a JIRA-first or other generic tracker abstraction a personal skill might assume.

If a *personal* skill from `~/.claude/skills/` overlaps a repo skill in purpose (e.g. a personal ticket-creation skill vs. `creating-github-issues`' own conventions), this repo's own skill governs while working in this repo; the personal skill's generic guidance yields.
