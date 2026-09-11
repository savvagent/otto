# tool-stderr-inherit-fix Implementation Plan

**Goal:** Stop every stdio tool child process `ToolRegistry` spawns from inheriting the TUI's real
terminal stderr, by routing all three spawn sites in `crates/otto-host/src/tools.rs` through
`rmcp::transport::TokioChildProcess::builder(cmd).stderr(stderr).spawn()` instead of the plain
`TokioChildProcess::new(cmd)` convenience constructor, whose default `Stdio::inherit()` silently
overwrites whatever `redirect_tool_stderr`/`build_bash_command` already set on the `Command`. Fixes
`savvagent/otto#146`.

**Architecture:** A single internal helper, `spawn_tool_transport`, added to
`crates/otto-host/src/tools.rs`; `redirect_tool_stderr` changed to return the `std::process::Stdio`
it computes instead of mutating the `Command` in place; the three existing
`TokioChildProcess::new(cmd)` call sites switched to call the new helper with that returned `Stdio`.
No crate boundary changes — everything lands inside `otto-host`, which already owns
`ToolRegistry`/stdio-transport construction (Load-Bearing Invariant 5). No host-swap `RwLock` code is
touched (`crates/otto/src/app.rs`/`tui.rs` are untouched by this plan) and no streaming provider path
is touched (the `ProgressDispatcher` forwarder-abort pattern is not engaged).

**Tech Stack:** Existing workspace only — `crates/otto-host` (`tools.rs`), the already-depended-on
`rmcp` 1.6.0 (`TokioChildProcess::builder`/`.stderr`/`.spawn`, already available, no `Cargo.toml`
change). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-11-tool-stderr-inherit-fix-design.md` — read it first. This
plan implements it exactly.

**Release line:** next PATCH after whatever `workspace.package.version` reads at cut time (currently
`0.30.2` as of branch creation — re-read at cut time per the release step below, since `origin/main`
moves while this branch is open). Internal bug fix, no public-interface change → PATCH per
`CHANGELOG.md`'s convention.

**Branch:** `host/tool-stderr-inherit-fix`

## File Map

**New files**
- `docs/superpowers/specs/2026-09-11-tool-stderr-inherit-fix-design.md` — design source of truth
  (already committed).
- `docs/superpowers/plans/2026-09-11-tool-stderr-inherit-fix.md` — this plan.

**Modified files**
- `crates/otto-host/src/tools.rs` — new `spawn_tool_transport` helper; `redirect_tool_stderr`
  signature change (mutate → return); three call sites updated; one new regression test.
- `CHANGELOG.md` — a `Fixed` entry (added in the dedicated release PR per Phase 4 step 12, not in
  this PR — see Task 2).

## Task 1: Fix the stderr-inherit leak at all three tool-spawn call sites

**Files:**
- Modify: `crates/otto-host/src/tools.rs`

- [ ] **Step 1: Write the failing regression test first.** Add a new test to
  `crates/otto-host/src/tools.rs` (in the existing `#[cfg(test)] mod tests` if one exists in this
  file, else a new `#[cfg(test)] mod tests` block near the bottom of the file, matching this crate's
  existing test-module convention — check with `grep -n "mod tests" crates/otto-host/src/tools.rs`
  first):

  ```rust
  #[tokio::test]
  async fn tool_child_process_honors_explicit_stderr_redirect() {
      use tokio::io::AsyncReadExt;

      let mut cmd = if cfg!(windows) {
          let mut c = tokio::process::Command::new("cmd");
          c.args(["/C", "echo REGRESSION_MARKER_TOOL_STDERR 1>&2"]);
          c
      } else {
          let mut c = tokio::process::Command::new("sh");
          c.arg("-c").arg("echo REGRESSION_MARKER_TOOL_STDERR 1>&2");
          c
      };
      // TokioChildProcess::new(cmd) (the buggy path this test guards against)
      // would silently force Stdio::inherit() here regardless of what we ask
      // for — see redirect_tool_stderr's doc comment and the design spec's
      // root-cause section. spawn_tool_transport must make our explicit
      // Stdio::piped() win instead.
      let (_transport, stderr) = spawn_tool_transport(cmd, std::process::Stdio::piped())
          .expect("spawn should succeed");
      let mut stderr = stderr.expect(
          "stderr must be piped back when the caller explicitly requests \
           Stdio::piped() — if this is None, the child's stderr silently \
           inherited the test process's own stderr instead",
      );
      let mut buf = String::new();
      stderr
          .read_to_string(&mut buf)
          .await
          .expect("read piped stderr");
      assert!(
          buf.contains("REGRESSION_MARKER_TOOL_STDERR"),
          "expected the child's stderr output to be captured via the piped \
           handle, got: {buf:?}"
      );
  }
  ```

  This will not compile yet (`spawn_tool_transport` doesn't exist) — that's the expected
  failing-first state; proceed to Step 2.

- [ ] **Step 2: Confirm the test fails to compile.**
  ```bash
  cargo test -p otto-host tool_child_process_honors_explicit_stderr_redirect
  ```
  Expect a compile error: `cannot find function 'spawn_tool_transport' in this scope`.

- [ ] **Step 3: Add the `spawn_tool_transport` helper.** In `crates/otto-host/src/tools.rs`, near
  `redirect_tool_stderr` (currently around line 1372), add:

  ```rust
  /// Spawn `cmd` as an MCP stdio child process with `stderr` honored exactly
  /// as given — never `rmcp`'s own `Stdio::inherit()` default.
  ///
  /// `TokioChildProcess::new(cmd)` (the plain convenience constructor) always
  /// wins the stdio race against any `cmd.stderr(...)` the caller already
  /// set: `TokioChildProcessBuilder::new`'s own default triple (`stdin`/
  /// `stdout` piped, `stderr` **inherited**) is unconditionally re-applied to
  /// the `Command` inside `.spawn()`, discarding whatever the caller
  /// configured (confirmed against the pinned `rmcp` 1.6.0 source —
  /// `TokioChildProcessBuilder::spawn`'s `self.cmd.command_mut().stdin(..)
  /// .stdout(..).stderr(..)` call). Every otto stdio-tool spawn site must
  /// therefore go through `TokioChildProcess::builder(cmd).stderr(stderr)
  /// .spawn()` — this is the one place that does it, so a future spawn site
  /// cannot silently regress back to inheriting the TUI's real terminal.
  ///
  /// Returns the `Option<ChildStderr>` `rmcp` hands back (`Some` only when
  /// spawned with `Stdio::piped()`) so callers/tests that need to observe
  /// stderr directly can; production call sites pass a file or
  /// `Stdio::null()` and discard it.
  fn spawn_tool_transport(
      cmd: tokio::process::Command,
      stderr: std::process::Stdio,
  ) -> std::io::Result<(TokioChildProcess, Option<tokio::process::ChildStderr>)> {
      TokioChildProcess::builder(cmd).stderr(stderr).spawn()
  }
  ```

  Check the exact import path/name for `ChildStderr` already in scope in this file
  (`grep -n "^use " crates/otto-host/src/tools.rs | grep -i rmcp`) and match it rather than
  introducing a second import path for the same type.

- [ ] **Step 4: Run the test again — it should now compile and pass.**
  ```bash
  cargo test -p otto-host tool_child_process_honors_explicit_stderr_redirect
  ```
  Expect: PASS. (To confirm this test actually guards the regression rather than passing
  vacuously, temporarily revert the helper's body to call `TokioChildProcess::new(cmd)` instead —
  ignoring the `stderr` parameter — rerun the test, confirm it now FAILS with the "stderr must be
  piped back" panic message, then restore the real `.builder(cmd).stderr(stderr).spawn()` body
  before continuing. This throwaway revert is not committed.)

- [ ] **Step 5: Change `redirect_tool_stderr` to return `Stdio` instead of mutating `cmd`.** Current
  signature (around line 1372):
  ```rust
  fn redirect_tool_stderr(cmd: &mut tokio::process::Command, command: &Path) {
      let stderr = match tool_stderr_log_file(command) {
          Ok(file) => std::process::Stdio::from(file),
          Err(_) => std::process::Stdio::null(),
      };
      cmd.stderr(stderr);
  }
  ```
  Change to:
  ```rust
  fn redirect_tool_stderr(command: &Path) -> std::process::Stdio {
      match tool_stderr_log_file(command) {
          Ok(file) => std::process::Stdio::from(file),
          Err(_) => std::process::Stdio::null(),
      }
  }
  ```
  Update its doc comment (currently says "Redirect a tool subprocess's stderr to..." describing an
  in-place mutation) to describe returning the `Stdio` instead, and keep the existing "never inherit
  the TUI's terminal, not must log everything" invariant sentence — it is still true and still the
  point of this function.

- [ ] **Step 6: Update the three call sites.**

  **6a — `build_bash_command`** (currently around line 1283-1323, calls `apply_sandbox` then
  `redirect_tool_stderr(&mut cmd, command); cmd` returning a bare `Command`): change its return type
  to `(tokio::process::Command, std::process::Stdio)` and its final lines from
  ```rust
  redirect_tool_stderr(&mut cmd, command);
  cmd
  ```
  to
  ```rust
  let stderr = redirect_tool_stderr(command);
  (cmd, stderr)
  ```
  Update its signature and doc comment accordingly.

  **6b — tool-bash probe spawn** (currently around line 490-499):
  ```rust
  let probe_cmd = build_bash_command(...);
  ...
  let transport = match TokioChildProcess::new(probe_cmd)
      .with_context(|| format!("spawn tool-bash probe: {label}"))
  {
      Ok(transport) => transport,
      ...
  };
  ```
  becomes
  ```rust
  let (probe_cmd, probe_stderr) = build_bash_command(...);
  ...
  let transport = match spawn_tool_transport(probe_cmd, probe_stderr)
      .map(|(transport, _stderr_handle)| transport)
      .with_context(|| format!("spawn tool-bash probe: {label}"))
  {
      Ok(transport) => transport,
      ...
  };
  ```

  **6c — eager (non-bash) tool spawn** (currently around line 643-660):
  ```rust
  let wrapper = apply_sandbox(&mut cmd, command, project_root, sandbox);
  let allow_net = sandbox.net_allowed_for(command);
  log_sandbox_wrapper(&label, &wrapper, allow_net, sandbox.is_enabled());
  redirect_tool_stderr(&mut cmd, command);

  let transport = match TokioChildProcess::new(cmd)
      .with_context(|| format!("spawn tool server: {label}"))
  {
  ```
  becomes
  ```rust
  let wrapper = apply_sandbox(&mut cmd, command, project_root, sandbox);
  let allow_net = sandbox.net_allowed_for(command);
  log_sandbox_wrapper(&label, &wrapper, allow_net, sandbox.is_enabled());
  let stderr = redirect_tool_stderr(command);

  let transport = match spawn_tool_transport(cmd, stderr)
      .map(|(transport, _stderr_handle)| transport)
      .with_context(|| format!("spawn tool server: {label}"))
  {
  ```

  **6d — lazy tool-bash respawn** (currently around line 1225-1234):
  ```rust
  let cmd = build_bash_command(...);
  let label = self.config.command.display().to_string();
  let transport = match TokioChildProcess::new(cmd) {
      Ok(t) => t,
      Err(e) => { ... }
  };
  ```
  becomes
  ```rust
  let (cmd, stderr) = build_bash_command(...);
  let label = self.config.command.display().to_string();
  let transport = match spawn_tool_transport(cmd, stderr) {
      Ok((t, _stderr_handle)) => t,
      Err(e) => { ... }
  };
  ```

  In all four spots, keep every other line (error messages, `label`, status pushes) unchanged —
  only the `Command`-building/spawning lines move.

- [ ] **Step 7: Run the full otto-host test suite.**
  ```bash
  cargo test -p otto-host
  ```
  Expect: all tests pass, including the new one. Pay particular attention to any existing test that
  exercises `ToolRegistry::connect` or the lazy bash respawn path (search
  `grep -n "fn.*test" crates/otto-host/src/tools.rs` to find them) — confirm none relied on the old
  `redirect_tool_stderr(&mut cmd, ...)` signature or on `build_bash_command` returning a bare
  `Command`.

- [ ] **Step 8: Full workspace build and lint.**
  ```bash
  cargo build --workspace --all-targets
  cargo clippy --workspace --all-targets
  ```
  Expect both clean (CI runs clippy with `RUSTFLAGS=-D warnings`) — pay attention to any
  `unused_variables`/`must_use` warning on the now-returned `Option<ChildStderr>` at the three
  production call sites; the plan's `_stderr_handle` naming above already suppresses the "unused"
  lint, but re-check after the real edit.

- [ ] **Step 9: Public-interface note.** No SPP wire type, `ProviderHandler`/`ProviderClient` method,
  tool MCP schema, plugin ABI surface, slash command, env var, or on-disk transcript/keyring format
  touched — Non-Negotiable Rule 6 is not engaged. `spawn_tool_transport` and the changed
  `redirect_tool_stderr`/`build_bash_command` signatures are all private (non-`pub`) to
  `crates/otto-host/src/tools.rs`.

- [ ] **Step 10: Host-swap / streaming invariants — vacuously satisfied.** No
  `crates/otto/src/app.rs` or `crates/otto/src/tui.rs` touched, so the host-swap `RwLock` rule is not
  engaged. No streaming provider path touched, so the `ProgressDispatcher` forwarder-abort pattern is
  not engaged. Confirmed by the spec's "Why the TUI then froze" section and independently verified
  during spec review.

- [ ] **Step 11: Format and commit.**
  ```bash
  cargo fmt --all
  git add crates/otto-host/src/tools.rs
  git commit -m "otto-host: stop tool child processes from inheriting the TUI's stderr"
  ```

## Task 2: Cut the release (notes only — performed per Phase 4 step 12, not in this PR)

**Files:** none in this PR.

- [ ] **Step 1:** This PR does **not** bump `workspace.package.version` and does **not** add a
  `CHANGELOG.md` section — that happens in the dedicated release PR after this merges, per
  Non-Negotiable Rule 8 / Phase 4 step 12. Re-read `workspace.package.version` at cut time (it may
  have moved past `0.30.2` if another PR merges first) and cut the next PATCH from whatever it then
  reads. The `CHANGELOG.md` entry: a `Fixed` bullet along the lines of "stdio tool child processes
  (tool-fs, tool-web, tool-bash, etc.) no longer inherit the TUI's terminal stderr — a pre-existing
  defect in how `rmcp`'s stdio transport was constructed that could corrupt the TUI's rendered screen
  the first time a tool server was spawned after the terminal entered raw/alternate-screen mode (most
  visibly on a mid-session `/connect` that builds a fresh host on demand)."
