# Stop stdio tool child processes from inheriting the TUI's terminal — design

Date: 2026-09-11
Status: IMPLEMENTED
Source: savvagent/otto#146

## Problem

`savvagent/otto#146` reports that on the very next `/connect` after #132 ("fix DeepSeek /connect
never recovering from a rejected key") shipped, entering a valid DeepSeek API key through the
`/connect` modal froze the TUI on a screen of raw `tracing`/`rmcp` log output — `serve_inner INFO`
lines, `otto-tool-web starting on stdio`, `rmcp::service` initialization traces,
`InitializeRequestParams`/`InitializedNotification` traces — instead of returning to the normal chat
view. The header showed the provider as connected (`Otto — deepseek · deepseek-v4-flash`); the body
was garbled and the TUI stopped accepting input.

### Root cause (confirmed by reading the exact pinned dependency source, not inferred)

`otto`'s stdio tool child processes are spawned in `crates/otto-host/src/tools.rs` at three call
sites, all using `rmcp::transport::TokioChildProcess::new(cmd)`:

- `ToolRegistry::connect`'s tool-bash probe spawn, `crates/otto-host/src/tools.rs:499`
- `ToolRegistry::connect`'s eager (non-bash) tool spawn, `crates/otto-host/src/tools.rs:658`
- The lazy tool-bash blue-green respawn, `crates/otto-host/src/tools.rs:1234`

Before each of these, otto's own code redirects the child's stderr away from the terminal:
`redirect_tool_stderr` (`tools.rs:1372-1378`, called from `build_bash_command` at `tools.rs:1321`
and directly at `tools.rs:656`) opens a per-tool append log file under `~/.otto/logs/tools/` and
calls `cmd.stderr(Stdio::from(file))` (or `Stdio::null()` on open failure) on the
`tokio::process::Command` — with a doc comment stating the invariant explicitly: *"never inherit the
TUI's terminal."*

That invariant is violated by the framework call immediately downstream. `rmcp` 1.6.0 — the exact
version pinned in this repo's `Cargo.lock` (confirmed by reading
`~/.cargo/registry/src/.../rmcp-1.6.0/src/transport/child_process.rs`) — implements
`TokioChildProcess::new` as:

```rust
pub fn new(command: impl Into<CommandWrap>) -> std::io::Result<Self> {
    let (proc, _ignored) = TokioChildProcessBuilder::new(command).spawn()?;
    Ok(proc)
}
```

`TokioChildProcessBuilder::new` sets its own default stdio triple — `stdin: Stdio::piped()`,
`stdout: Stdio::piped()`, **`stderr: Stdio::inherit()`** — and `TokioChildProcessBuilder::spawn`
unconditionally re-applies all three onto the `Command` immediately before spawning:

```rust
pub fn spawn(mut self) -> std::io::Result<(TokioChildProcess, Option<ChildStderr>)> {
    self.cmd
        .command_mut()
        .stdin(self.stdin)
        .stdout(self.stdout)
        .stderr(self.stderr);   // <- always Stdio::inherit() when reached via `::new()`
    ...
}
```

Because this call happens *after* otto's own `cmd.stderr(...)` call, it silently overwrites it.
Every stdio tool child process spawned via the plain `TokioChildProcess::new(cmd)` convenience
constructor therefore inherits the parent (otto TUI) process's real stderr, regardless of what
`redirect_tool_stderr`/`build_bash_command` configured. `rmcp` does expose the escape hatch this bug
needs — `TokioChildProcess::builder(cmd).stderr(io).spawn()` — but none of the three call sites in
`tools.rs` use it.

### Why this was invisible until now, and why #132 exposed it

This defect has existed in every stdio tool spawn since the sandbox/stderr-redirect code was
written — it is not new in this PR's sense of "introduced by #132." What #132 changed is *when* a
brand-new `Host` (and therefore a brand-new `ToolRegistry`, spawning brand-new tool child processes)
can come into existence:

- **Before #132:** the only path that ever called `Host::start` (and thus `ToolRegistry::connect`,
  and thus the buggy spawn sites) was the app's own startup sequence
  (`bootstrap_app_and_host`/`bootstrap_pool_host`), which completes *before* the TUI enters
  ratatui's alternate screen / raw terminal mode. Any inherited-stderr log lines from that first
  round of tool spawns land on the plain scrollback terminal and are immediately overwritten the
  moment the TUI takes over the screen. The corruption existed on stderr but was never visible.
- **After #132:** `apply_pending_pool_add` (`crates/otto/src/main.rs:1855-2020`) can now reach
  `bootstrap_first_pool_host` (`crates/otto/src/main.rs:2726-2777`) — which calls `Host::start(cfg)`
  again — from a call site reached **after the TUI event loop is already running and the terminal is
  already in alternate-screen raw mode** (the `/connect`-driven credential re-validation path with no
  host yet up, exactly DeepSeek's first-connect-of-session case in #146). This is the first code path
  that spawns *new* tool child processes mid-session, while ratatui already owns the screen. The
  inherited stderr writes now land directly in the live alternate-screen buffer instead of a
  soon-to-be-discarded scrollback region.
- The lazy tool-bash blue-green respawn (`tools.rs:1234`) has the identical defect and is reachable
  purely from a running session (any `tool-bash` call whose `allow_net` decision changes from the
  cached spawn key triggers a respawn) — it was not part of the reported symptom (the repro uses
  `otto-tool-web`, not `tool-bash`) but shares the exact same root cause and needs the exact same fix,
  or it remains a live landmine for the next person to trip.

### Why the TUI then "froze" / stopped responding

This is a terminal-corruption symptom, not a literal event-loop deadlock or a panicked worker task —
confirmed by process of elimination against the two other hypotheses the issue's Design Note raised:

- **Not a panicked worker task.** A panic in a spawned Tokio task does not kill the process or the
  TUI's own event-loop task, and nothing in `apply_pending_pool_add`'s or
  `bootstrap_first_pool_host`'s call chain uses `catch_unwind` recovery that would produce this
  specific garbled-log-output symptom rather than a note or a silent no-op if it panicked.
- **Not an event-loop stall under the host-swap `RwLock`.** `apply_pending_pool_add` and
  `bootstrap_first_pool_host` do not hold `host_slot`'s write guard (or any read guard) across the
  `Host::start(cfg).await` call that triggers the tool spawns — the write lock is only taken
  afterward, briefly, to store the finished `Arc<Host>` (`crates/otto/src/main.rs:2766`). The
  host-swap discipline (Load-Bearing Invariant 3) is intact on this path; it is not implicated.
- **Is raw bytes hitting the real terminal underneath ratatui.** ratatui/crossterm render by
  computing a diff against an internally-tracked previous-frame `Buffer` and emitting only the
  changed cells' escape sequences. When something *else* writes arbitrary bytes (here: a child
  process's inherited stderr, itself unbuffered `tracing` INFO/DEBUG lines with their own newlines
  and no ANSI/cursor-position awareness) directly to the same file descriptor outside ratatui's
  `Terminal::draw` calls, the real screen state drifts out of sync with ratatui's tracked `Buffer`.
  The next `draw()` computes its diff against the *stale* tracked buffer, not what is actually on
  screen, and emits escape sequences that no longer correspond to reality — producing exactly the
  reported symptom: a garbled screen that stops updating in any way a user can interpret as
  responsive, even though the underlying event loop, key-read task, and turn loop are all still
  running normally. (A live interactive reproduction to visually confirm this exact rendering failure
  mode is out of scope for this fix per the job's own framing — "a TUI freeze is hard to verify
  headlessly" — but the code-path elimination above is sufficient to rule out the other two
  hypotheses and to identify a fix that removes the actual byte leak at its source.)

## Approach

Stop every stdio tool child process from ever inheriting the TUI's real stderr, at the one place the
leak actually happens: the transport-construction call. Introduce one small internal helper in
`crates/otto-host/src/tools.rs`:

```rust
/// Spawn `cmd` as an MCP stdio child process with `stderr` honored exactly as
/// given, never rmcp's own `Stdio::inherit()` default.
///
/// `TokioChildProcess::new(cmd)` (the plain convenience constructor) always
/// wins the stdio race against any `cmd.stderr(...)` the caller already set —
/// `TokioChildProcessBuilder::new`'s own default triple (`stdin`/`stdout`
/// piped, `stderr` **inherited**) is unconditionally re-applied to the
/// `Command` inside `.spawn()`, discarding whatever the caller configured.
/// Every otto stdio-tool spawn site must therefore go through
/// `TokioChildProcess::builder(cmd).stderr(stderr).spawn()` instead of
/// `::new()` — this is the one place that does it, so a future spawn site
/// cannot regress back to the silently-broken default.
fn spawn_tool_transport(
    cmd: tokio::process::Command,
    stderr: std::process::Stdio,
) -> std::io::Result<(TokioChildProcess, Option<tokio::process::ChildStderr>)> {
    TokioChildProcess::builder(cmd).stderr(stderr).spawn()
}
```

`redirect_tool_stderr` already computes the exact `Stdio` otto wants (the opened log file, or
`Stdio::null()` on failure); change it to *return* that `Stdio` instead of mutating the `Command`
directly, and update all three call sites to pass the returned value into
`spawn_tool_transport(cmd, stderr)` instead of calling `TokioChildProcess::new(cmd)` directly. This
keeps the "single source of truth for what stderr should be" logic exactly where it already lives —
only the final wiring step changes.

The `_stderr_handle` (`Option<ChildStderr>`, populated only when spawned with `Stdio::piped()`) is
discarded: otto redirects to a file/`null`, never `piped()`, at these three sites, so the handle is
always `None` in production and there is nothing to read from it. The regression test (below) is the
one place that *does* pass `Stdio::piped()`, specifically to observe the handle.

## Scope

**In:**
- `crates/otto-host/src/tools.rs` — the new `spawn_tool_transport` helper; `redirect_tool_stderr`
  changed to return `std::process::Stdio` instead of mutating the `Command`; all three
  `TokioChildProcess::new(cmd)` call sites (`tools.rs:499`, `:658`, `:1234`) switched to
  `spawn_tool_transport`.
- One regression test in `crates/otto-host/src/tools.rs` (or a co-located `#[cfg(test)] mod`)
  proving a spawned tool child's stderr is captured into the caller-supplied `Stdio`, not inherited.
- `CHANGELOG.md` — a `Fixed` entry (added in the dedicated release PR per Non-Negotiable Rule 8 /
  Phase 4 step 12, not in this PR).

**Out:**
- Any change to `crates/otto/src/main.rs`'s `apply_pending_pool_add` / `perform_connect` /
  `bootstrap_first_pool_host` control flow. That code is not the defect — it correctly triggers a
  legitimate, needed on-demand host rebuild (the #132 fix); it merely exercises the pre-existing
  tool-spawn stderr leak for the first time from a screen-owning context. No behavior change is
  needed there.
- Any change to the host-swap `RwLock` discipline in `crates/otto/src/app.rs`/`tui.rs` — confirmed
  not implicated (see "Why the TUI then froze" above).
- Any change to `redirect_tool_stderr`'s file-vs-null fallback policy, `~/.otto/logs/tools/` naming,
  or log rotation — unrelated to this defect.
- Upgrading or patching the `rmcp` dependency, or filing an upstream issue against it — the escape
  hatch (`TokioChildProcess::builder(...).stderr(...)`) already exists in the pinned 1.6.0 release;
  no upstream change is required.
- Any change to `crates/otto-host/src/sandbox.rs`'s `apply_linux`/`apply_macos` command-rewrite
  logic — that code already correctly restores env/cwd after replacing the whole `Command` with the
  `bwrap`/`sandbox-exec` wrapper, and `redirect_tool_stderr`/`spawn_tool_transport` are called *after*
  `apply_sandbox` at every site, so the sandbox rewrite does not interact with this fix.
- Provider stdio spawning — providers are linked in-process by default
  (`InProcessProviderClient`); the standalone `otto-<vendor>` binaries exist only for
  wire-protocol debugging and are not part of the connect path this issue reports on, so they are not
  touched by this fix. (The `otto-tool-web starting on stdio` line named in the bug report is a
  *tool* server's own startup log, not a provider's.)

## Public-interface changes

**None.** `spawn_tool_transport` and the changed `redirect_tool_stderr` signature are both
private (non-`pub`) functions internal to `crates/otto-host/src/tools.rs`; no `ProviderHandler`/
`ProviderClient` trait method, SPP wire type, tool MCP input schema, plugin ABI surface, slash
command, env var, or on-disk transcript/keyring format is added, renamed, or removed. Non-Negotiable
Rule 6 is not engaged. No crate gains a new dependency edge (rmcp's `builder()`/`stderr()` API is
already available in the already-depended-on `rmcp` 1.6.0).

## Assumptions

- **The fix belongs entirely in `otto-host`, not in `crates/otto`.** The defect is in how tool
  child-process transports are constructed, which is `ToolRegistry`'s own responsibility
  (Load-Bearing Invariant 5); `crates/otto/src/main.rs`'s connect/rebuild flow is a legitimate caller
  that should not need to know anything about stdio wiring internals.
- **A single shared helper, not three independent call-site edits, is the right shape.** All three
  sites need identical treatment (swap `::new(cmd)` for `::builder(cmd).stderr(s).spawn()`); a shared
  `spawn_tool_transport` makes the "always honor the caller's stderr" invariant impossible to
  regress at a fourth future call site by accident, and its doc comment records *why* — the
  library-level gotcha will not be obvious from the call site alone.
- **`redirect_tool_stderr` returning a value instead of mutating `cmd` is the minimal-diff shape.**
  The alternative (keep mutating `cmd`, then have `spawn_tool_transport` read back `cmd`'s already-set
  stderr) is not possible — `tokio::process::Command` does not expose a getter for a previously-set
  `Stdio`, only for program/args/envs/cwd. Returning the `Stdio` at computation time is the only way
  to both open the log file at most once and pass its `Stdio` explicitly through `.builder().stderr()`.
- **The regression test lives in `otto-host`, exercises the exact library interaction, and does not
  need a live TUI.** A real interactive-terminal reproduction of the garbled-screen symptom is
  explicitly out of scope per the job framing ("a TUI freeze is hard to verify headlessly"); the
  acceptance criterion is satisfiable, and more precisely targeted, by proving the underlying byte
  leak is closed — assert that a tool child process's stderr writes land in the `Stdio` the caller
  supplied, using the fixed `spawn_tool_transport` (or, before the fix lands, that assertion fails,
  which is how the test was validated during development — see the plan's TDD step).
- **The test spawns via `TokioChildProcess::builder(cmd).stderr(Stdio::piped()).spawn()` and reads
  the returned `ChildStderr` handle directly**, rather than attempting OS-level fd redirection of the
  test process's own stderr. This is both simpler and a strictly stronger proof for what
  `spawn_tool_transport` actually does: `Stdio::piped()` only returns `Some(ChildStderr)` from
  `TokioChildProcessBuilder::spawn` when the builder's `.stderr(...)` call actually won — exactly the
  behavior this fix restores and `TokioChildProcess::new(cmd)` (unfixed) cannot produce, since `::new`
  never accepts a caller override at all. Reading real output out of that handle is a stronger
  assertion than a type-level check would be.
- **Test child command is platform-conditional (`sh -c '...1>&2'` on Unix, `cmd /C '...1>&2'` on
  Windows)**, matching this repo's CI matrix (`otto-host` builds and tests on all three OSes, unlike
  `otto-canvas`/`otto`) rather than gating the whole test behind `#[cfg(unix)]` and losing Windows
  coverage of a fix that applies identically on that platform.

## Goal & Success Criteria

Every stdio tool child process spawned by `ToolRegistry` — at startup, on an on-demand mid-session
host rebuild (the #132/#146 path), and on a lazy tool-bash respawn — has its stderr captured into the
log file `redirect_tool_stderr` already computes (or discarded to `Stdio::null()` on that file's own
open failure), never inherited from the parent TUI process, regardless of when in the TUI's lifecycle
the spawn happens.

- All three `TokioChildProcess::new(cmd)` call sites in `crates/otto-host/src/tools.rs` are replaced
  with `spawn_tool_transport(cmd, stderr)`.
- A regression test proves a tool child process's stderr is captured into the caller-supplied `Stdio`
  and never silently defaults to inheriting the test process's own stderr.
- `cargo test -p otto-host` passes, including the new test.
- `cargo build --workspace --all-targets`, `cargo clippy --workspace --all-targets`, and
  `cargo fmt --all --check` are clean.
- Entering a DeepSeek API key via `/connect` with no host yet up (the #146 repro path) no longer has
  any code path capable of writing a spawned tool child's stderr to the TUI's real terminal — verified
  by code-path inspection (this exact call chain: `apply_pending_pool_add` →
  `bootstrap_first_pool_host` → `Host::start` → `ToolRegistry::connect` → `spawn_tool_transport`) since
  a live interactive TUI reproduction is out of scope per the job framing.

## Error Handling & Edge Cases

- **`redirect_tool_stderr`'s log-file-open failure still degrades to `Stdio::null()`, never to
  inherit.** This is existing, correct behavior (`tools.rs:1372-1378`'s own doc comment: "the
  invariant is 'never inherit the TUI's terminal', not 'must log everything'") and is preserved
  unchanged by this fix — only the *plumbing* of the resulting `Stdio` to the actual spawn call
  changes, not the fallback policy itself.
- **The discarded `Option<ChildStderr>` from `TokioChildProcessBuilder::spawn`.** Since otto never
  spawns tools with `Stdio::piped()` in production (always a file or `Stdio::null()`), the returned
  handle is always `None` at all three real call sites — verified by inspection of
  `redirect_tool_stderr`'s two return arms — so discarding it is not a resource leak or a missed
  cleanup obligation.
- **`apply_sandbox`'s whole-`Command`-replacement rewrite still runs before `spawn_tool_transport`.**
  Both existing call order constraints — `apply_sandbox` before `redirect_tool_stderr`/stdio wiring —
  are preserved verbatim; this fix only changes what happens to the `Stdio` value after it is
  computed, not when in the sequence it is computed.
- **The lazy tool-bash respawn path (`tools.rs:1234`) gets the identical fix even though it is not
  part of the reported repro**, because it shares the exact same root cause and is reachable from an
  ordinary, already-connected session (any `allow_net` change on a subsequent `tool-bash` call). Not
  including it here would leave a known, understood landmine unfixed in the same file, in the same
  change, for no scoping reason.

## Risks & Open Questions

- **None identified requiring escalation.** The root cause is confirmed against the exact pinned
  `rmcp` 1.6.0 source (not inferred from behavior or upstream changelogs), the fix path
  (`TokioChildProcess::builder(...).stderr(...)`) already exists in that exact pinned version, and the
  regression test validates the fixed behavior directly against the library's own documented API
  rather than against a reimplementation of `rmcp`'s internals.
- A live interactive-terminal reproduction of the garbled-screen symptom was not performed (per the
  job's own framing that this is hard to verify headlessly); the "why the TUI froze" explanation
  above is the strongest code-path-elimination account available without one, and is offered as
  explanation, not as an independently reproduced fact. If a future report shows the freeze persisting
  after this fix ships, that would falsify this account and warrant reopening investigation into the
  event-loop-stall/panicked-task hypotheses this spec ruled out by inspection.
