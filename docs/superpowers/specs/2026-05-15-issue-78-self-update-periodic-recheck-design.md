# Self-update periodic re-check — design

Date: 2026-05-15
Status: pending review
Issue: #78 ("self-update: TUI only checks for new releases at startup —
no periodic re-check")
Ships as: v0.14.3 (PATCH)

## Problem

The `internal:self-update` plugin only checks for new releases when the
`HostStarting` hook fires (at TUI launch). A long-lived TUI session
never notices a release published mid-session, even if it stays open
for days. The 24h on-disk cache further suppresses re-checks across
restarts within the TTL, so a user who relaunches inside the cache
window also sees nothing new.

The user only learns about a new release on next launch *and* only when
either the cache has expired or the running version exceeds the cached
tag (the fix in #75 / commit f61924f).

## Approach

Extend the existing `on_event(HostStarting)` spawned task. Today it
runs the check-and-maybe-install body once and exits; instead wrap that
body in a `tokio::time::interval` loop:

- The first tick fires immediately, preserving today's startup
  behavior.
- Subsequent ticks fire every `PERIODIC_INTERVAL` (2 hours).
- Each tick runs the same `check_for_update` → `run_install` pipeline
  used by the startup path, so periodic detection auto-installs in
  exactly the same way (settled in brainstorming).

No new plugin hook, no host-level scheduler, no plugin-trait change —
the timer lives inside the existing tokio task.

## Per-tick skip rules

Evaluated at the top of each iteration. The pre-check state is
snapshotted under a brief lock and the guard is dropped before any
`.await`.

| State at tick                            | Action                                          |
|------------------------------------------|-------------------------------------------------|
| `Disabled` (opt-out detected)            | break loop                                      |
| `InstallMethod::Dev`                     | never starts the loop (existing check)          |
| `Installing { .. }`                      | skip this tick (no double-installer)            |
| `Updated { .. }`                         | break loop (awaiting user restart)              |
| `InstallFailed { latest: failed, .. }`   | run check; install only if new tag ≠ `failed`   |
| `Unknown` / `UpToDate` / `CheckFailed`   | run check + maybe install                       |
| `Available { .. }`                       | run check + maybe install (retry)               |

The `Installing` skip is non-trivial: the spawned interval task and
the plugin dispatcher run concurrently. A `/update` invocation goes
through `handle_slash` → `run_install`, which awaits the installer on
the dispatcher task while the spawned interval task is parked at
`tick().await`. When the tick fires, the spawned task observes
`Installing` and skips. Without this rule the spawned task would
race the dispatcher and reach `run_install` from two tasks
simultaneously.

The `Updated` break stops the loop because the binary on disk has
already been swapped; the running process is now waiting for the
user to relaunch, so further checks add no value.

The `InstallFailed` rule re-runs the check on every tick — picking
up any *newer* release that GitHub publishes — but skips the install
when the live check still resolves to the same tag that previously
failed. This avoids hammering a known-broken release while still
recovering automatically once a new release lands. The state
transition rules:

- Pre-state `InstallFailed { latest: failed, .. }`, check result
  `Available { latest: new, .. }` with `new == failed` → leave state
  as-is (preserves the error message for the banner).
- Pre-state `InstallFailed`, check result `Available { latest: new, .. }`
  with `new != failed` → transition to `Available { latest: new, .. }`,
  run install.
- Pre-state `InstallFailed`, check result `UpToDate` → transition to
  `UpToDate` (the failing release was likely withdrawn upstream or
  the user upgraded out-of-band).
- Pre-state `InstallFailed`, check result `CheckFailed` → leave
  state as-is (no new authoritative info).

## Cache interaction

The 24h on-disk cache (`~/.otto/update-check.json`) is a
**startup-skip** mechanism — it answers "should this process even hit
the network at launch?" It is **not** an in-process throttle.

The loop differentiates between the **first tick** (effectively
"startup") and **subsequent ticks** (true periodic re-checks):

- **First tick reads the cache** (preserving today's startup
  behavior at `mod.rs:351-360` — a fresh cache entry that is not
  stale-vs-current short-circuits the network call exactly as it
  does today).
- **Subsequent ticks bypass the cache read.** A periodic tick always
  means "go ask GitHub now" — the cache exists for startup savings,
  not in-process throttling.
- **All ticks that successfully fetch write the cache.** The cache
  file therefore always reflects the latest tag the most recent
  run-of-otto confirmed with GitHub, regardless of which tick
  produced it.

Implementation note: the first-vs-subsequent distinction is a
boolean flag captured outside the loop and flipped after the first
iteration completes; the existing cache-read block in the spawned
task moves inside `if is_first_tick { ... } else { /* fetch only */ }`.

## Interval

```rust
const PERIODIC_INTERVAL: Duration = Duration::from_secs(2 * 60 * 60);
```

Fixed at 2 hours. Not configurable via env var — YAGNI; can be added
later if users ask. The existing `OTTO_NO_UPDATE_CHECK` env var
and `--no-update-check` CLI flag continue to short-circuit the plugin
at construction, so the loop never starts when the user has opted out.

**Missed-tick behavior.** `tokio::time::interval` defaults to
`MissedTickBehavior::Burst`, which would fire back-to-back catch-up
ticks if the system is suspended (laptop closed, etc.) or if a single
check+install runs longer than `PERIODIC_INTERVAL`. The spawned task
**must** call `interval.set_missed_tick_behavior(MissedTickBehavior::Delay)`
immediately after constructing the interval. `Delay` semantics: skip
all missed ticks, then resume at the interval after `now`. This keeps
the cadence at "no more often than every 2h" regardless of suspends
or long-running installs.

For tests, a private `Duration` field on the plugin overrides the
const. The field defaults to `PERIODIC_INTERVAL`; a `#[cfg(test)]`
builder method (`with_periodic_interval`) lets tests drop it to
something small (e.g. `Duration::from_millis(50)`) so `tokio::time::pause()`
+ `advance()` can drive multiple ticks without real sleeping.

## Module changes

All edits land in `crates/otto/src/plugin/builtin/self_update/mod.rs`:

1. Add `PERIODIC_INTERVAL` const.
2. Add `periodic_interval: Duration` field on `SelfUpdatePlugin`,
   defaulted in `with_fetcher_and_installer`.
3. Add `#[cfg(test)] fn with_periodic_interval(self, d: Duration) -> Self`.
4. Refactor the body of the spawned task in `on_event` into a private
   `async fn run_check_once(...)` so the loop body is a single call.
5. Replace the one-shot spawn with a `loop { interval.tick().await;
   ... }` that consults the skip rules above before calling
   `run_check_once`. First `interval.tick()` resolves immediately, so
   the existing startup-check timing is preserved.

`check.rs`, `apply.rs`, `cache.rs`, `install_method.rs` are unchanged.
No public API change on `SelfUpdatePlugin` except the test-only
builder.

## Tests added

In the existing `mod tests` block in `mod.rs`. All new tests run
under `#[tokio::test(start_paused = true)]` so virtual time can be
advanced deterministically. A small per-test interval (e.g.
`Duration::from_millis(50)`) is wired in via `with_periodic_interval`,
and a small helper that interleaves `tokio::time::advance(...)` with
`yield_now().await` polls the plugin state without burning a real
2-hour clock.

Existing tests at `mod.rs:948` and `mod.rs:1003` use a `wait_for_state`
helper that only yields — it does not advance virtual time and so is
not reusable for the new tests. The new tests define their own
`advance_then_yield` helper.

**Locking discipline.** `HOME_LOCK` is held only around plugin
construction + locale setup (`rust_i18n::set_locale("en")`); the
guard is dropped before any `.await`. This mirrors
`locked_plugin_with_state` at `mod.rs:576-588` and satisfies the
crate-wide `-D clippy::await_holding_lock` lint. Where a test needs
the locale to remain `en` while awaiting, the lock is re-acquired
briefly for any post-await mutation — never held across `.await`.

**Test doubles added.** The current `ReleasesFetcher` impl
(`FixedFetcher`) has no invocation counter. A new `CountingFetcher`
that wraps a fixed tag string and increments an `AtomicUsize` per
`latest_tag()` call is required for tests 1, 2, 5, and 6. A new
`BlockingStubInstaller` whose `install()` awaits a
`tokio::sync::Notify::notified()` held by the test is required for
test 3.

1. **`periodic_check_runs_multiple_ticks`** — `CountingFetcher` with
   a tag below `CARGO_PKG_VERSION` so the classification lands at
   `UpToDate` (keeping the loop alive), `StubInstaller::ok()`,
   `with_install_method(InstallMethod::Installed)`, 50ms periodic
   interval, cache override pointed at a tempdir. Drive
   `on_event(HostStarting)`, yield, advance ≥ 2× the interval (with
   yields between advances). Assert
   `fetcher.invocation_count() >= 3` (one startup tick + at least
   two periodic ticks).
2. **`periodic_check_breaks_after_updated`** — `CountingFetcher("v99.99.99")`
   + `StubInstaller::ok()`, `InstallMethod::Installed`, 50ms
   interval. First tick lands the plugin in `Updated`. Advance 5×
   the interval. Assert `installer.invocation_count() == 1` and
   `fetcher.invocation_count() == 1` — the loop must have broken.
3. **`periodic_check_skips_during_concurrent_slash_install`** —
   models the real concurrency case. `FixedFetcher("v99.99.99")` +
   `BlockingStubInstaller`, `InstallMethod::Installed`, 50ms
   interval, cache override. Drive `on_event` (spawns the loop).
   On a separate task, call `plugin.handle_slash("update", vec![])`
   — that call parks at the installer's notify, leaving state at
   `Installing`. Advance 3× the interval with yields between
   advances; assert `installer.invocation_count() == 1` across the
   whole window (the spawned tick observed `Installing` and
   skipped). Release the notify; `await` the spawned handle to
   completion; assert state transitions to `Updated` and
   `installer.invocation_count()` is still 1.
4. **`periodic_check_does_not_start_in_dev`** — explicitly call
   `with_install_method(InstallMethod::Dev)`. Drive `on_event`,
   advance several intervals. Assert `installer.invocation_count() == 0`
   and `fetcher.invocation_count() == 0`. State must end at
   `Disabled`.
5. **`install_failed_periodic_skips_install_when_tag_unchanged`** —
   pre-seed state to `InstallFailed { latest: 99.99.99, error: "x", .. }`
   with `CountingFetcher("v99.99.99")` (same tag) +
   `StubInstaller::ok()`, `InstallMethod::Installed`, 50ms interval.
   Drive `on_event`, advance 3× the interval. Assert
   `installer.invocation_count() == 0` (skipped because the tag
   matches the previously failed one) and state remains
   `InstallFailed` (banner keeps showing the error).
6. **`install_failed_periodic_installs_when_new_tag_appears`** —
   pre-seed state to `InstallFailed { latest: 99.99.98, .. }` with
   `CountingFetcher("v99.99.99")` (new tag) + `StubInstaller::ok()`,
   `InstallMethod::Installed`, 50ms interval. Drive `on_event`,
   advance 1× the interval, yield. Assert
   `installer.invocation_count() == 1` and state transitions to
   `Updated { to: 99.99.99, .. }`.
7. **`first_tick_uses_cache_subsequent_tick_bypasses`** — pre-write
   a fresh cache entry at the override path with tag below current
   version (so it is a cache hit at startup and not stale). On
   first tick, assert `fetcher.invocation_count() == 0` (cache was
   honored). Advance 1× the interval; assert
   `fetcher.invocation_count() == 1` (subsequent tick bypassed
   cache and hit the fetcher).
8. **Existing tests stay green.** The four tests that exercise
   `on_event` today (`host_starting_auto_installs_on_available_then_writes_cache`,
   `host_starting_install_failure_transitions_to_install_failed`,
   `other_events_are_ignored`,
   `host_starting_bypasses_cache_when_current_version_is_ahead_of_cached_tag`)
   now observe the first tick of the interval but otherwise see the
   same behavior they assert today. They do not pause virtual time
   and rely on the first tick resolving immediately under tokio's
   default `Burst` semantics — confirmed via `tokio::time::interval`
   docs that the first call to `tick()` completes immediately.

## Concurrency notes

- The spawned task continues to be a `tokio::spawn` future on the host
  runtime. It clones `Arc<Mutex<UpdateState>>`, `Arc<dyn ReleasesFetcher>`,
  `Arc<dyn Installer>` at spawn time — no new locks.
- `state.lock()` is never held across an `.await` (the existing code
  already enforces this; the loop preserves it by lifting the
  classification body into a non-locking helper).
- The `rmcp ProgressDispatcher` gotcha (see `project_rmcp_progress_gotcha`
  memory) does not apply here — this plugin's network call goes through
  `reqwest` in `GithubReleasesFetcher`, not through an MCP RPC.

## Versioning, changelog, docs

- Bump workspace version `0.14.2` → `0.14.3` in the root `Cargo.toml`
  `[workspace.package]` block and mirror the bump in the
  `[workspace.dependencies]` literals for each in-workspace crate
  (per `feedback_semver` in project memory).
- Add a CHANGELOG entry using the existing repo style — top-level
  heading `## v0.14.3 — <one-line summary> (2026-05-15)` (matches
  `CHANGELOG.md:9`), with a `### Fixed` subsection referencing issue
  #78 and naming the new 2h periodic re-check behavior.
- Update the README's self-update paragraph to mention that the check
  also re-runs every 2 hours while the TUI is open, alongside the
  existing opt-out flag documentation (per `feedback_release_docs`).

## Out of scope

- Configurable interval via env var (deferred until requested).
- A separate "periodic check" plugin hook on the trait surface (the
  in-task `tokio::time::interval` is sufficient).
- Adjusting the 24h cache TTL — it remains a startup-only concern.
- Notify-only mode for periodic detection — auto-install was settled
  in brainstorming.

## Ship plan

One PR — localized fix in a single file plus version bump, CHANGELOG,
README:

1. Bump workspace version to 0.14.3 (Cargo.toml + workspace.dependencies
   literals).
2. Implement the loop + tests in `self_update/mod.rs`.
3. CHANGELOG entry.
4. README paragraph update.
5. Verify `cargo build`, `cargo test --workspace`, `rustup run stable
   cargo fmt`, `rustup run stable cargo clippy --workspace --all-targets
   -- -D warnings` all clean (per `feedback_match_ci_toolchain_locally`).
6. Open PR, wait for CI green (per `feedback_verify_ci_after_push`),
   merge.
7. Tag `v0.14.3` — cargo-dist owns the release lifecycle, do not run
   `gh release create` manually (per `feedback_cargo_dist_release`).
8. Post a comment on issue #78 closing it (per
   `feedback_keep_issue_updated`).
