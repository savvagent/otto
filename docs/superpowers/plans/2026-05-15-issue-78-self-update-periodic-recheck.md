# Self-update Periodic Re-check Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `internal:self-update` plugin re-check GitHub Releases every 2 hours while the TUI is running, so a long-lived session picks up new releases without a restart. Ships as v0.14.3, closing issue #78.

**Architecture:** Extend the existing `on_event(HostStarting)` spawned task. Today it runs the check-and-maybe-install body once and exits; instead wrap that body in a `tokio::time::interval` loop. First tick fires immediately (preserves startup behavior, reads on-disk cache). Subsequent ticks fire every 2 hours, bypass the cache read, and run the same `check_for_update` → `run_install` pipeline. Skip rules at the top of each iteration: `Disabled` and `Updated` break the loop; `Installing` skips the tick; `InstallFailed { latest: failed, .. }` re-runs the check but installs only if the live tag differs from the previously failed one. `MissedTickBehavior::Delay` is set so suspends or long installs don't cause burst catch-up.

**Tech Stack:** Rust, `tokio::time::interval`, `async_trait`, existing `otto-plugin` trait surface, no new crates.

**Spec:** `docs/superpowers/specs/2026-05-15-issue-78-self-update-periodic-recheck-design.md`

**Branch:** `issue-78-self-update-periodic-recheck` (already created, includes spec + revision + plan commits).

**Toolchain note:** This crate enables `-D warnings` and `-D clippy::await_holding_lock`. The `dead_code` lint is also a hard error in the binary crate (`feedback_dead_code_in_binary_crate`). All `pub` items added below are consumed by non-test paths or test-only.

---

## File Structure

| Path | Role | Tasks |
|------|------|-------|
| `crates/otto/src/plugin/builtin/self_update/cache.rs` | Broaden `load` / `save` signatures from `&PathBuf` to `&Path` | 1 |
| `crates/otto/src/plugin/builtin/self_update/mod.rs` | Plugin implementation + tests | 1–6 |
| `Cargo.toml` (root) | Workspace version bump | 7 |
| `CHANGELOG.md` | Release notes entry | 8 |
| `README.md` | Self-update paragraph update | 9 |

Implementation work concentrates in `self_update/mod.rs`. The plan progressively grows the loop, adds test doubles, and finally adds the InstallFailed decision logic.

---

## Task 1: Broaden cache signatures to `&Path`

**Goal:** Make `cache::load` and `cache::save` accept `&Path` so the rest of the plan's helper signatures can use the idiomatic borrow form. No behavior change — `&PathBuf` deref-coerces to `&Path`, so existing callers (including the cache module's own tests) continue to work without edits.

**Files:**
- Modify: `crates/otto/src/plugin/builtin/self_update/cache.rs:11` (add `Path` import)
- Modify: `crates/otto/src/plugin/builtin/self_update/cache.rs:77` (`load` signature)
- Modify: `crates/otto/src/plugin/builtin/self_update/cache.rs:104` (`save` signature)
- Modify: `crates/otto/src/plugin/builtin/self_update/mod.rs:351` (call site uses `.as_ref()` → `.as_deref()`)

- [ ] **Step 1: Confirm baseline tests pass**

Run: `cargo test -p otto self_update`
Expected: all current tests pass.

- [ ] **Step 2: Update `cache.rs` imports**

At `cache.rs:11`, change:
```rust
use std::path::PathBuf;
```
to:
```rust
use std::path::{Path, PathBuf};
```

`PathBuf` is still needed for the return type of `cache_path()`.

- [ ] **Step 3: Broaden `cache::load`**

At `cache.rs:77`, change:
```rust
pub fn load(path: &PathBuf) -> Option<CacheEntry> {
```
to:
```rust
pub fn load(path: &Path) -> Option<CacheEntry> {
```

The function body is unchanged.

- [ ] **Step 4: Broaden `cache::save`**

At `cache.rs:104`, change:
```rust
pub fn save(path: &PathBuf, entry: &CacheEntry) {
```
to:
```rust
pub fn save(path: &Path, entry: &CacheEntry) {
```

The function body is unchanged.

- [ ] **Step 5: Update the call site in `mod.rs`**

At `mod.rs:351`, change `.as_ref()` to `.as_deref()`:

Before:
```rust
            let cached_fresh = cache_path
                .as_ref()
                .and_then(cache::load)
```

After:
```rust
            let cached_fresh = cache_path
                .as_deref()
                .and_then(cache::load)
```

(`Option<PathBuf>::as_deref()` returns `Option<&Path>`, matching the new `cache::load` signature.)

- [ ] **Step 6: Run tests**

Run: `cargo test -p otto self_update`
Expected: all tests pass — `&PathBuf` arguments in the cache module's tests deref-coerce to `&Path`.

- [ ] **Step 7: Clippy + fmt**

Run: `rustup run stable cargo clippy -p otto --all-targets -- -D warnings`
Run: `rustup run stable cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/otto/src/plugin/builtin/self_update/cache.rs \
        crates/otto/src/plugin/builtin/self_update/mod.rs
git commit -m "refactor(self-update/cache): take &Path instead of &PathBuf

Idiomatic borrow form for the load/save signatures. Existing
callers' &PathBuf arguments deref-coerce, so no caller changes
except the single mod.rs call site that switches as_ref() to
as_deref(). Prepares for the periodic-recheck helper signature."
```

---

## Task 2: Extract `run_check_once` helper (pure refactor)

**Goal:** Lift the body of the `tokio::spawn` block in `on_event` into a private free function `run_check_once`. No behavior change. All existing tests must pass unchanged.

**Files:**
- Modify: `crates/otto/src/plugin/builtin/self_update/mod.rs:306-411` (the `on_event` impl and the spawn body)

- [ ] **Step 1: Confirm baseline tests pass**

Run: `cargo test -p otto self_update::tests`
Expected: all tests in the existing `self_update::tests` module pass.

- [ ] **Step 2: Add the helper function**

Add this free function immediately after `run_install` (currently ends around `mod.rs:509`), before the `#[cfg(test)] mod tests` block at `mod.rs:511`:

```rust
/// One pass of the version-check + maybe-install pipeline. Shared by
/// the `HostStarting` interval loop (each tick calls this) and the
/// auto-install path. Stateless aside from the shared `Arc`s and the
/// cache file — safe to call repeatedly.
async fn run_check_once(
    state: &Arc<Mutex<UpdateState>>,
    fetcher: &Arc<dyn ReleasesFetcher>,
    installer: &Arc<dyn Installer>,
    install_method: InstallMethod,
    current_version: &str,
    cache_path: Option<&std::path::Path>,
) {
    // 24h cache: if a fresh entry exists, skip the network. Tests
    // that exercise this code path pass an explicit override (set
    // via `with_cache_path_override`) so the production cache file
    // under the developer's real `$HOME` is never touched by the
    // suite.
    //
    // A cached `latest_tag` strictly older than the running binary
    // is treated as a cache miss: it implies the user upgraded
    // out-of-band (cargo install, downloaded tarball, package
    // manager) since the cache was written, so we have no
    // authoritative info about what's newer than the current
    // version and must re-fetch.
    let cached_fresh = cache_path
        .and_then(cache::load)
        .filter(|e| cache::is_fresh(e, cache::now_unix(), cache::DEFAULT_TTL_SECS))
        .filter(|e| {
            !matches!(
                check::compare_versions(current_version, &e.latest_tag),
                check::Comparison::Ahead | check::Comparison::Unparseable
            )
        });

    let result = if let Some(entry) = cached_fresh {
        tracing::debug!(tag = %entry.latest_tag, "self-update: using cached tag");
        check::classify_tag(current_version, &entry.latest_tag)
    } else {
        let fresh =
            check_for_update(current_version, install_method, fetcher.as_ref()).await;
        if let Some(path) = cache_path {
            if let Some(tag) = match &fresh {
                UpdateState::Available { latest, .. } => Some(format!("v{latest}")),
                UpdateState::UpToDate => Some(format!("v{current_version}")),
                _ => None,
            } {
                cache::save(
                    path,
                    &cache::CacheEntry {
                        schema_version: 1,
                        checked_at_unix: cache::now_unix(),
                        latest_tag: tag,
                    },
                );
            }
        }
        fresh
    };

    let pending_install = if let UpdateState::Available { current, latest } = &result {
        Some((current.clone(), latest.clone()))
    } else {
        None
    };

    if let Ok(mut guard) = state.lock() {
        *guard = result;
    }

    if let Some((current, latest)) = pending_install {
        let _ = run_install(Arc::clone(state), Arc::clone(installer), current, latest).await;
    }
}
```

- [ ] **Step 3: Replace the spawn body to call the helper**

In `on_event`, replace the `tokio::spawn(async move { ... });` block (currently `mod.rs:330-408`) with:

```rust
        tokio::spawn(async move {
            if matches!(install_method, InstallMethod::Dev) {
                if let Ok(mut guard) = state.lock() {
                    *guard = UpdateState::Disabled;
                }
                return;
            }

            let cache_path = cache_path_override.or_else(cache::cache_path);
            run_check_once(
                &state,
                &fetcher,
                &installer,
                install_method,
                &current_version,
                cache_path.as_deref(),
            )
            .await;
        });
```

- [ ] **Step 4: Run existing tests**

Run: `cargo test -p otto self_update::tests`
Expected: all tests pass. The behavior is identical — `run_check_once` is just the extracted body.

- [ ] **Step 5: Clippy + fmt**

Run: `rustup run stable cargo clippy -p otto --all-targets -- -D warnings`
Run: `rustup run stable cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/otto/src/plugin/builtin/self_update/mod.rs
git commit -m "refactor(self-update): extract run_check_once helper

Pure refactor: the body of on_event's spawn becomes a reusable
free function taking Option<&Path> for the cache path (now that
the cache module accepts &Path). No behavior change."
```

---

## Task 3: Add `periodic_interval` field + single-tick interval scaffolding

**Goal:** Introduce `periodic_interval: Duration` on `SelfUpdatePlugin`, a test-only setter, and the `tokio::time::interval` construct with `MissedTickBehavior::Delay`. Still only one tick is awaited (no loop yet), so behavior is unchanged.

**Files:**
- Modify: `crates/otto/src/plugin/builtin/self_update/mod.rs`

- [ ] **Step 1: Add `Duration` and `MissedTickBehavior` imports**

At `mod.rs:13-16` (the import block), the current imports are:
```rust
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
```

Add `std::time::Duration` and `tokio::time::MissedTickBehavior`:
```rust
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::time::MissedTickBehavior;
```

- [ ] **Step 2: Add `PERIODIC_INTERVAL` const**

Immediately after the existing `BANNER_SLOT_ID` const at `mod.rs:84`, add:

```rust
/// Re-check interval for the periodic loop spawned by `on_event(HostStarting)`.
/// First tick fires immediately (preserves startup behavior); subsequent
/// ticks fire every two hours. Tests override this via
/// [`SelfUpdatePlugin::with_periodic_interval`].
const PERIODIC_INTERVAL: Duration = Duration::from_secs(2 * 60 * 60);
```

- [ ] **Step 3: Add `periodic_interval` field**

In the `SelfUpdatePlugin` struct (`mod.rs:130-152`), add a new field after `cache_path_override`:

```rust
    /// Re-check cadence. Defaults to [`PERIODIC_INTERVAL`]; tests
    /// override via [`SelfUpdatePlugin::with_periodic_interval`] so
    /// `tokio::time::pause()` + `advance()` can drive multiple ticks
    /// without burning a real 2-hour wall clock.
    periodic_interval: Duration,
```

In `with_fetcher_and_installer` (`mod.rs:168-184`), initialize the new field:

```rust
        Self {
            install_method: detect(),
            state: Arc::new(Mutex::new(initial)),
            fetcher,
            installer,
            cache_path_override: None,
            periodic_interval: PERIODIC_INTERVAL,
        }
```

- [ ] **Step 4: Add the test-only setter**

Immediately after the existing `with_install_method` setter (currently `mod.rs:201-205`), add:

```rust
    /// Test-only: override the periodic re-check cadence. Default is
    /// [`PERIODIC_INTERVAL`] (2 hours); tests pass something tiny like
    /// `Duration::from_millis(50)` so they can drive multiple ticks
    /// under `tokio::time::pause()` + `advance()`.
    #[cfg(test)]
    pub fn with_periodic_interval(mut self, interval: Duration) -> Self {
        self.periodic_interval = interval;
        self
    }
```

- [ ] **Step 5: Wire the interval into the spawned task (still single-tick)**

Modify the spawned-task body added in Task 2. Replace the body of `tokio::spawn(async move { ... })` with:

```rust
        let periodic_interval = self.periodic_interval;
        tokio::spawn(async move {
            if matches!(install_method, InstallMethod::Dev) {
                if let Ok(mut guard) = state.lock() {
                    *guard = UpdateState::Disabled;
                }
                return;
            }

            let cache_path = cache_path_override.or_else(cache::cache_path);
            let mut interval = tokio::time::interval(periodic_interval);
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

            // First tick resolves immediately, matching today's startup
            // timing. The full loop with skip rules is added in Task 4.
            interval.tick().await;
            run_check_once(
                &state,
                &fetcher,
                &installer,
                install_method,
                &current_version,
                cache_path.as_deref(),
            )
            .await;
        });
```

The `let periodic_interval = self.periodic_interval;` line must appear with the other captures (after `let cache_path_override = self.cache_path_override.clone();` at the existing `mod.rs:328`).

- [ ] **Step 6: Run existing tests**

Run: `cargo test -p otto self_update::tests`
Expected: all tests pass. `interval.tick().await` resolves immediately for the first call, so behavior is identical to Task 2.

- [ ] **Step 7: Clippy + fmt**

Run: `rustup run stable cargo clippy -p otto --all-targets -- -D warnings`
Run: `rustup run stable cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/otto/src/plugin/builtin/self_update/mod.rs
git commit -m "feat(self-update): add periodic_interval scaffolding

Adds PERIODIC_INTERVAL const, periodic_interval field with a
test-only setter, and wraps run_check_once in tokio::time::interval
with MissedTickBehavior::Delay. Still single-tick — the multi-tick
loop follows. No behavior change for users."
```

---

## Task 4: Multi-tick loop + first-tick cache gating + four tests

**Goal:** Convert the single-tick await into a real loop with `Disabled` and `Updated` skip rules AND add the first-tick-only cache read. These two changes ship together because the multi-tick proof (`periodic_check_runs_multiple_ticks`) only works once subsequent ticks bypass the cache — otherwise the first tick poisons every subsequent tick into a cache hit and the fetcher count stalls at 1.

**Files:**
- Modify: `crates/otto/src/plugin/builtin/self_update/mod.rs`

- [ ] **Step 1: Add the `CountingFetcher` test double**

In the `#[cfg(test)] mod tests` block, immediately after `FixedFetcher` (currently around `mod.rs:521-529`), add:

```rust
    /// In-test releases fetcher that returns a fixed tag and counts
    /// how many times `latest_tag()` is invoked. Used by tests that
    /// need to assert the periodic loop is actually running ticks.
    struct CountingFetcher {
        tag: &'static str,
        invocations: AtomicUsize,
    }

    impl CountingFetcher {
        fn new(tag: &'static str) -> Self {
            Self {
                tag,
                invocations: AtomicUsize::new(0),
            }
        }
        fn invocation_count(&self) -> usize {
            self.invocations.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl ReleasesFetcher for CountingFetcher {
        async fn latest_tag(&self) -> anyhow::Result<String> {
            self.invocations.fetch_add(1, Ordering::SeqCst);
            Ok(self.tag.to_string())
        }
    }
```

`AtomicUsize` and `Ordering` are already imported in the test module (currently `mod.rs:518`).

- [ ] **Step 2: Write four failing tests**

Add the following helpers and tests inside the `#[cfg(test)] mod tests` block, after the existing `host_starting_bypasses_cache_when_current_version_is_ahead_of_cached_tag` test (at the bottom of the module, just before the closing `}` at `mod.rs:1111`):

```rust
    /// Advance virtual time and yield enough that the spawned task can
    /// observe the tick. Pause must already be active via
    /// `#[tokio::test(start_paused = true)]`.
    async fn advance_and_yield(by: Duration) {
        tokio::time::advance(by).await;
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    }

    /// Drive `n` periodic ticks by advancing virtual time `n` times at
    /// `step` each. Necessary because `MissedTickBehavior::Delay` (set on
    /// the production interval) collapses a single large advance into
    /// exactly ONE tick — it skips missed ticks and resumes after now.
    /// To produce multiple ticks we must advance step-by-step.
    async fn advance_n_ticks(n: usize, step: Duration) {
        for _ in 0..n {
            advance_and_yield(step).await;
        }
    }

    /// Spin until the plugin's state matches `predicate`, advancing virtual
    /// time by `step` between checks. Bounded to 200 iterations.
    async fn advance_until_state(
        plugin: &SelfUpdatePlugin,
        step: Duration,
        predicate: impl Fn(&UpdateState) -> bool,
    ) -> UpdateState {
        for _ in 0..200 {
            advance_and_yield(step).await;
            let s = plugin.state();
            if predicate(&s) {
                return s;
            }
        }
        panic!(
            "state predicate never matched; final state: {:?}",
            plugin.state()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn periodic_check_runs_multiple_ticks() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_path = tmp.path().join("update-check.json");
        let interval = Duration::from_millis(50);

        // Tag below CARGO_PKG_VERSION → classification lands at UpToDate
        // (the "Ahead" branch in compare_versions), keeping the loop alive.
        let fetcher = Arc::new(CountingFetcher::new("v0.0.1"));
        let installer = Arc::new(StubInstaller::ok());

        let mut p = {
            let _lock = HOME_LOCK.lock().unwrap();
            rust_i18n::set_locale("en");
            SelfUpdatePlugin::with_fetcher_and_installer(
                Arc::clone(&fetcher) as Arc<dyn ReleasesFetcher>,
                Arc::clone(&installer) as Arc<dyn Installer>,
            )
            .with_cache_path_override(cache_path)
            .with_install_method(InstallMethod::Installed)
            .with_periodic_interval(interval)
        };

        p.on_event(HostEvent::HostStarting).await.unwrap();
        // Let the first tick run.
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        // Drive three periodic ticks (Delay-mode interval requires
        // advancing one step at a time).
        advance_n_ticks(3, interval).await;

        assert!(
            fetcher.invocation_count() >= 3,
            "expected ≥3 fetch invocations (1 startup + ≥2 periodic), got {}",
            fetcher.invocation_count()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn periodic_check_breaks_after_updated() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_path = tmp.path().join("update-check.json");
        let interval = Duration::from_millis(50);

        let fetcher = Arc::new(CountingFetcher::new("v99.99.99"));
        let installer = Arc::new(StubInstaller::ok());

        let mut p = {
            let _lock = HOME_LOCK.lock().unwrap();
            rust_i18n::set_locale("en");
            SelfUpdatePlugin::with_fetcher_and_installer(
                Arc::clone(&fetcher) as Arc<dyn ReleasesFetcher>,
                Arc::clone(&installer) as Arc<dyn Installer>,
            )
            .with_cache_path_override(cache_path)
            .with_install_method(InstallMethod::Installed)
            .with_periodic_interval(interval)
        };

        p.on_event(HostEvent::HostStarting).await.unwrap();
        advance_until_state(&p, interval, |s| matches!(s, UpdateState::Updated { .. })).await;

        // Loop must have broken; advance several more intervals and confirm
        // the fetcher and installer were not re-invoked.
        advance_and_yield(interval * 5).await;
        assert_eq!(
            installer.invocation_count(),
            1,
            "installer must run exactly once before the Updated break"
        );
        assert_eq!(
            fetcher.invocation_count(),
            1,
            "fetcher must run exactly once before the Updated break"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn periodic_check_does_not_start_in_dev() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_path = tmp.path().join("update-check.json");
        let interval = Duration::from_millis(50);

        let fetcher = Arc::new(CountingFetcher::new("v99.99.99"));
        let installer = Arc::new(StubInstaller::ok());

        let mut p = {
            let _lock = HOME_LOCK.lock().unwrap();
            rust_i18n::set_locale("en");
            SelfUpdatePlugin::with_fetcher_and_installer(
                Arc::clone(&fetcher) as Arc<dyn ReleasesFetcher>,
                Arc::clone(&installer) as Arc<dyn Installer>,
            )
            .with_cache_path_override(cache_path)
            .with_install_method(InstallMethod::Dev)
            .with_periodic_interval(interval)
        };

        p.on_event(HostEvent::HostStarting).await.unwrap();
        advance_and_yield(interval * 5).await;

        assert_eq!(fetcher.invocation_count(), 0);
        assert_eq!(installer.invocation_count(), 0);
        assert_eq!(p.state(), UpdateState::Disabled);
    }

    #[tokio::test(start_paused = true)]
    async fn first_tick_uses_cache_subsequent_tick_bypasses() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_path = tmp.path().join("update-check.json");
        let interval = Duration::from_millis(50);

        // Pre-write a fresh cache entry with the current version as the tag,
        // so it is a startup cache HIT (not stale-vs-current) and classifies
        // to UpToDate.
        let current = env!("CARGO_PKG_VERSION");
        cache::save(
            &cache_path,
            &cache::CacheEntry {
                schema_version: 1,
                checked_at_unix: cache::now_unix(),
                latest_tag: format!("v{current}"),
            },
        );

        let fetcher = Arc::new(CountingFetcher::new("v99.99.99"));
        let installer = Arc::new(StubInstaller::ok());

        let mut p = {
            let _lock = HOME_LOCK.lock().unwrap();
            rust_i18n::set_locale("en");
            SelfUpdatePlugin::with_fetcher_and_installer(
                Arc::clone(&fetcher) as Arc<dyn ReleasesFetcher>,
                Arc::clone(&installer) as Arc<dyn Installer>,
            )
            .with_cache_path_override(cache_path)
            .with_install_method(InstallMethod::Installed)
            .with_periodic_interval(interval)
        };

        p.on_event(HostEvent::HostStarting).await.unwrap();
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            fetcher.invocation_count(),
            0,
            "first tick must use the on-disk cache when fresh and not stale"
        );

        // Second tick must bypass the cache and hit the fetcher.
        advance_and_yield(interval).await;
        assert!(
            fetcher.invocation_count() >= 1,
            "subsequent tick must bypass cache and call the fetcher, got {}",
            fetcher.invocation_count()
        );
    }
```

- [ ] **Step 3: Run the new tests — expect failures**

Run: `cargo test -p otto self_update::tests::periodic_check`
Run: `cargo test -p otto self_update::tests::first_tick`

Expected: All four tests fail (or hang — kill the run with Ctrl-C if a test hangs):
- `periodic_check_runs_multiple_ticks` FAILS: fetcher is called only once at the first tick; subsequent calls to `interval.tick().await` never happen because the spawned task has already exited.
- `periodic_check_breaks_after_updated` may PASS coincidentally (the spawned task already exits after one tick), but the loop infrastructure is what we want to verify; treat it as a "should still pass when the loop is added" check.
- `periodic_check_does_not_start_in_dev` PASSES (Dev short-circuit already in place).
- `first_tick_uses_cache_subsequent_tick_bypasses` FAILS: the second assertion (`>= 1`) fails because the spawned task has exited after the first cached read.

- [ ] **Step 4: Convert single-tick await to multi-tick loop with skip rules and first-tick cache gating**

Update the `run_check_once` signature to accept `is_first_tick`:

```rust
async fn run_check_once(
    state: &Arc<Mutex<UpdateState>>,
    fetcher: &Arc<dyn ReleasesFetcher>,
    installer: &Arc<dyn Installer>,
    install_method: InstallMethod,
    current_version: &str,
    cache_path: Option<&std::path::Path>,
    is_first_tick: bool,
) {
```

Inside `run_check_once`, replace the existing `let cached_fresh = ...` block with one gated on `is_first_tick`:

```rust
    let cached_fresh = if is_first_tick {
        cache_path
            .and_then(cache::load)
            .filter(|e| cache::is_fresh(e, cache::now_unix(), cache::DEFAULT_TTL_SECS))
            .filter(|e| {
                !matches!(
                    check::compare_versions(current_version, &e.latest_tag),
                    check::Comparison::Ahead | check::Comparison::Unparseable
                )
            })
    } else {
        None
    };
```

The cache *write* path lower in the function stays unchanged.

In `on_event`'s spawn body (from Task 3), replace the single-tick await with a real loop:

```rust
            let cache_path = cache_path_override.or_else(cache::cache_path);
            let mut interval = tokio::time::interval(periodic_interval);
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
            let mut is_first_tick = true;

            loop {
                interval.tick().await;

                // Snapshot pre-tick state. Skip rules evaluate before the
                // network call so we can short-circuit cheap cases.
                let pre_state = match state.lock() {
                    Ok(g) => g.clone(),
                    Err(_) => return,
                };
                match pre_state {
                    UpdateState::Disabled => return,
                    UpdateState::Updated { .. } => return,
                    _ => {}
                }

                run_check_once(
                    &state,
                    &fetcher,
                    &installer,
                    install_method,
                    &current_version,
                    cache_path.as_deref(),
                    is_first_tick,
                )
                .await;
                is_first_tick = false;
            }
```

- [ ] **Step 5: Run the new tests — expect pass**

Run: `cargo test -p otto self_update::tests::periodic_check`
Run: `cargo test -p otto self_update::tests::first_tick`

Expected: all four tests pass.

- [ ] **Step 6: Run the full module's tests**

Run: `cargo test -p otto self_update::tests`
Expected: all pass (28 existing + 4 new = 32 total).

- [ ] **Step 7: Clippy + fmt**

Run: `rustup run stable cargo clippy -p otto --all-targets -- -D warnings`
Run: `rustup run stable cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/otto/src/plugin/builtin/self_update/mod.rs
git commit -m "feat(self-update): multi-tick loop with first-tick cache gating

Converts the single-tick await to a tokio::time::interval loop with
MissedTickBehavior::Delay. First tick reads the on-disk cache to
preserve startup savings; subsequent ticks bypass the cache. Skip
rules at the top of each iteration break the loop on Disabled and
Updated.

Adds CountingFetcher and four tests: multi-tick execution, the
Updated break, the Dev install-method short-circuit, and the
first-tick-cache / subsequent-tick-bypass split."
```

---

## Task 5: `Installing` skip rule + concurrent `/update` test

**Goal:** Add the `Installing` skip rule and prove it works via a test that models a concurrent `/update` invocation parked on a blocking installer. The slash task must reach `Installing` *before* the periodic loop starts so the loop's first tick deterministically observes the skip case.

**Files:**
- Modify: `crates/otto/src/plugin/builtin/self_update/mod.rs`

- [ ] **Step 1: Add `BlockingStubInstaller` test double**

In the `#[cfg(test)] mod tests` block, after `CountingFetcher`, add:

```rust
    /// In-test installer that parks `install()` on a `Notify` until the
    /// test explicitly releases it. Counts invocations the same way
    /// `StubInstaller` does. Used to model the concurrent `/update`
    /// case where state stays at `Installing` across a tick boundary.
    struct BlockingStubInstaller {
        notify: Arc<tokio::sync::Notify>,
        invocations: AtomicUsize,
    }

    impl BlockingStubInstaller {
        fn new(notify: Arc<tokio::sync::Notify>) -> Self {
            Self {
                notify,
                invocations: AtomicUsize::new(0),
            }
        }
        fn invocation_count(&self) -> usize {
            self.invocations.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Installer for BlockingStubInstaller {
        async fn install(&self, _latest: &Version) -> anyhow::Result<()> {
            self.invocations.fetch_add(1, Ordering::SeqCst);
            self.notify.notified().await;
            Ok(())
        }
    }
```

- [ ] **Step 2: Write the failing test**

Add inside the `mod tests` block, after the four tests from Task 4. Sequencing: start the slash task FIRST (so the slash-driven install enters `BlockingStubInstaller::install` and parks at the `notify`, leaving state at `Installing`), THEN start `on_event(HostStarting)`. The loop's first tick will observe `Installing` from the start.

```rust
    #[tokio::test(start_paused = true)]
    async fn periodic_check_skips_during_concurrent_slash_install() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_path = tmp.path().join("update-check.json");
        let interval = Duration::from_millis(50);

        let release = Arc::new(tokio::sync::Notify::new());
        let installer = Arc::new(BlockingStubInstaller::new(Arc::clone(&release)));
        let fetcher = Arc::new(FixedFetcher("v99.99.99"));

        let mut p = {
            let _lock = HOME_LOCK.lock().unwrap();
            rust_i18n::set_locale("en");
            SelfUpdatePlugin::with_fetcher_and_installer(
                Arc::clone(&fetcher) as Arc<dyn ReleasesFetcher>,
                Arc::clone(&installer) as Arc<dyn Installer>,
            )
            .with_cache_path_override(cache_path)
            .with_install_method(InstallMethod::Installed)
            .with_periodic_interval(interval)
        };
        // Pre-seed Available so handle_slash will pick the Install branch
        // immediately (no need to wait for the loop's first tick to set it).
        *p.state.lock().unwrap() = UpdateState::Available {
            current: Version::parse("0.10.0").unwrap(),
            latest: Version::parse("99.99.99").unwrap(),
        };

        // STEP A: Start the slash task BEFORE the periodic loop. The slash
        // helper shares the primary plugin's state + installer via Arc, so
        // the install it triggers mutates `p.state` directly. helper.handle_slash
        // calls run_install -> sets state to Installing -> awaits the notify.
        let shared_state = Arc::clone(&p.state);
        let shared_installer = Arc::clone(&installer) as Arc<dyn Installer>;
        let slash_task = tokio::spawn(async move {
            let mut helper = SelfUpdatePlugin::with_fetcher_and_installer(
                Arc::new(FixedFetcher("v99.99.99")),
                shared_installer,
            )
            .with_install_method(InstallMethod::Installed);
            helper.state = shared_state;
            let _ = helper.handle_slash("update", vec![]).await;
        });

        // Yield enough for the slash task to enter run_install and park at
        // the installer's notify.
        for _ in 0..32 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(p.state(), UpdateState::Installing { .. }),
            "slash task must park at installer with state Installing, got {:?}",
            p.state()
        );
        assert_eq!(installer.invocation_count(), 1);

        // STEP B: Now start the periodic loop. Its first tick must observe
        // Installing (set by the slash task) and skip.
        p.on_event(HostEvent::HostStarting).await.unwrap();

        // Drive three periodic ticks. The loop must observe Installing on
        // every iteration and skip — no second installer invocation.
        advance_n_ticks(3, interval).await;
        assert_eq!(
            installer.invocation_count(),
            1,
            "loop must skip while state is Installing; got {} invocations",
            installer.invocation_count()
        );

        // Release the notify; the slash task's install completes and the
        // shared state transitions to Updated.
        release.notify_one();
        let _ = slash_task.await;
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(p.state(), UpdateState::Updated { .. }),
            "expected Updated after install completes, got {:?}",
            p.state()
        );
        assert_eq!(installer.invocation_count(), 1);
    }
```

- [ ] **Step 3: Run the test — expect failure**

Run: `cargo test -p otto self_update::tests::periodic_check_skips_during_concurrent_slash_install`

Expected: FAIL. Without the `Installing` skip rule, the loop's first tick falls through to `run_check_once`, classifies `Available`, calls `run_install`, which sets state back to `Installing` (no-op overwrite) and invokes `BlockingStubInstaller::install` a SECOND time — `installer.invocation_count()` becomes 2, failing the `== 1` assertion.

- [ ] **Step 4: Add the Installing skip rule**

In the spawn body in `on_event`, update the match statement at the top of the loop iteration:

```rust
                match pre_state {
                    UpdateState::Disabled => return,
                    UpdateState::Updated { .. } => return,
                    UpdateState::Installing { .. } => continue,
                    _ => {}
                }
```

- [ ] **Step 5: Run the test — expect pass**

Run: `cargo test -p otto self_update::tests::periodic_check_skips_during_concurrent_slash_install`
Expected: PASS.

- [ ] **Step 6: Run full module tests**

Run: `cargo test -p otto self_update::tests`
Expected: all pass (28 + 5 new).

- [ ] **Step 7: Clippy + fmt**

Run: `rustup run stable cargo clippy -p otto --all-targets -- -D warnings`
Run: `rustup run stable cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/otto/src/plugin/builtin/self_update/mod.rs
git commit -m "feat(self-update): skip periodic tick while state is Installing

Adds the Installing skip rule and a test that models the real
concurrency case: the slash-driven install starts FIRST (parking
at a Notify-backed installer), then the HostStarting loop is
spawned and its first tick observes Installing → continue.
BlockingStubInstaller backs the parking behavior."
```

---

## Task 6: `InstallFailed` tag-changed decision logic

**Goal:** When pre-tick state is `InstallFailed { latest: failed, .. }`, run the network check but install only if the live tag differs from `failed`. Adds two tests.

**Files:**
- Modify: `crates/otto/src/plugin/builtin/self_update/mod.rs`

- [ ] **Step 1: Write the two failing tests**

Add inside `mod tests`, after the test from Task 5:

```rust
    #[tokio::test(start_paused = true)]
    async fn install_failed_periodic_skips_install_when_tag_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_path = tmp.path().join("update-check.json");
        let interval = Duration::from_millis(50);

        let fetcher = Arc::new(CountingFetcher::new("v99.99.99"));
        let installer = Arc::new(StubInstaller::ok());

        let mut p = {
            let _lock = HOME_LOCK.lock().unwrap();
            rust_i18n::set_locale("en");
            SelfUpdatePlugin::with_fetcher_and_installer(
                Arc::clone(&fetcher) as Arc<dyn ReleasesFetcher>,
                Arc::clone(&installer) as Arc<dyn Installer>,
            )
            .with_cache_path_override(cache_path)
            .with_install_method(InstallMethod::Installed)
            .with_periodic_interval(interval)
        };
        // Pre-seed with the SAME tag the fetcher will return.
        *p.state.lock().unwrap() = UpdateState::InstallFailed {
            current: Version::parse("0.10.0").unwrap(),
            latest: Version::parse("99.99.99").unwrap(),
            error: "previous failure".into(),
        };

        p.on_event(HostEvent::HostStarting).await.unwrap();
        advance_n_ticks(3, interval).await;

        assert_eq!(
            installer.invocation_count(),
            0,
            "must not re-attempt install when live tag matches previously failed tag"
        );
        assert!(
            matches!(p.state(), UpdateState::InstallFailed { .. }),
            "state must remain InstallFailed; got {:?}",
            p.state()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn install_failed_periodic_installs_when_new_tag_appears() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_path = tmp.path().join("update-check.json");
        let interval = Duration::from_millis(50);

        // Fetcher returns 99.99.99; pre-seed InstallFailed with 99.99.98
        // so the live tag is genuinely newer than the failed one.
        let fetcher = Arc::new(CountingFetcher::new("v99.99.99"));
        let installer = Arc::new(StubInstaller::ok());

        let mut p = {
            let _lock = HOME_LOCK.lock().unwrap();
            rust_i18n::set_locale("en");
            SelfUpdatePlugin::with_fetcher_and_installer(
                Arc::clone(&fetcher) as Arc<dyn ReleasesFetcher>,
                Arc::clone(&installer) as Arc<dyn Installer>,
            )
            .with_cache_path_override(cache_path)
            .with_install_method(InstallMethod::Installed)
            .with_periodic_interval(interval)
        };
        *p.state.lock().unwrap() = UpdateState::InstallFailed {
            current: Version::parse("0.10.0").unwrap(),
            latest: Version::parse("99.99.98").unwrap(),
            error: "previous failure".into(),
        };

        p.on_event(HostEvent::HostStarting).await.unwrap();
        let final_state =
            advance_until_state(&p, interval, |s| matches!(s, UpdateState::Updated { .. })).await;

        assert_eq!(installer.invocation_count(), 1);
        match final_state {
            UpdateState::Updated { to, .. } => assert_eq!(to.to_string(), "99.99.99"),
            other => unreachable!("predicate guarantees Updated: {other:?}"),
        }
    }
```

- [ ] **Step 2: Run the tests — expect failure**

Run: `cargo test -p otto self_update::tests::install_failed_periodic`
Expected: `install_failed_periodic_skips_install_when_tag_unchanged` FAILS — the current loop body sees pre-state `InstallFailed`, runs the check, classifies `Available`, then calls `run_install` and installs. The `installer.invocation_count() == 0` assertion fails (count is 1).

`install_failed_periodic_installs_when_new_tag_appears` may pass coincidentally with current behavior.

- [ ] **Step 3: Add `pre_state` parameter to `run_check_once`**

Change the signature:

```rust
async fn run_check_once(
    state: &Arc<Mutex<UpdateState>>,
    fetcher: &Arc<dyn ReleasesFetcher>,
    installer: &Arc<dyn Installer>,
    install_method: InstallMethod,
    current_version: &str,
    cache_path: Option<&std::path::Path>,
    is_first_tick: bool,
    pre_state: UpdateState,
) {
```

Replace the existing tail of the function (the block starting with `let pending_install = if let UpdateState::Available { current, latest } = &result { ... };` through the run_install call) with:

```rust
    // Decide what to publish + whether to install based on the pre-tick state.
    //
    // Special case: when pre-state is InstallFailed with tag `failed`, the
    // periodic loop re-runs the check (in case GitHub has published a newer
    // release) but only installs when the live tag differs from `failed`.
    // This avoids hammering a known-broken release while still recovering
    // automatically once a new release lands.
    let (publish, pending_install) = match (&pre_state, &result) {
        // Same-tag install failure: keep the failure context, do nothing.
        (
            UpdateState::InstallFailed { latest: failed, .. },
            UpdateState::Available { latest: new, .. },
        ) if failed == new => (None, None),

        // Live check now says we're up-to-date — clear the InstallFailed banner.
        (UpdateState::InstallFailed { .. }, UpdateState::UpToDate) => {
            (Some(UpdateState::UpToDate), None)
        }

        // Network failure with no new info — preserve the InstallFailed state.
        (UpdateState::InstallFailed { .. }, UpdateState::CheckFailed) => (None, None),

        // Either pre-state is not InstallFailed, or it is and the live tag
        // differs. Publish the new classification; install if Available.
        _ => {
            let install = if let UpdateState::Available { current, latest } = &result {
                Some((current.clone(), latest.clone()))
            } else {
                None
            };
            (Some(result), install)
        }
    };

    if let Some(new_state) = publish {
        if let Ok(mut guard) = state.lock() {
            *guard = new_state;
        }
    }

    if let Some((current, latest)) = pending_install {
        let _ = run_install(Arc::clone(state), Arc::clone(installer), current, latest).await;
    }
```

- [ ] **Step 4: Pass `pre_state` from the loop**

In `on_event`'s spawn body, the loop already snapshots `pre_state` for the skip rules. Pass that same value to `run_check_once`:

```rust
                run_check_once(
                    &state,
                    &fetcher,
                    &installer,
                    install_method,
                    &current_version,
                    cache_path.as_deref(),
                    is_first_tick,
                    pre_state,
                )
                .await;
                is_first_tick = false;
```

- [ ] **Step 5: Run the new tests**

Run: `cargo test -p otto self_update::tests::install_failed_periodic`
Expected: both pass.

- [ ] **Step 6: Run the full module's tests**

Run: `cargo test -p otto self_update::tests`
Expected: all pass (28 existing + 7 new = 35 total). If the existing `slash_update_when_install_failed_retries_install` (currently at `mod.rs:846`) fails, the regression is in the new decision logic — `handle_slash` calls `run_install` directly without going through `run_check_once`, so it should be unaffected; debug before moving on.

- [ ] **Step 7: Clippy + fmt**

Run: `rustup run stable cargo clippy -p otto --all-targets -- -D warnings`
Run: `rustup run stable cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/otto/src/plugin/builtin/self_update/mod.rs
git commit -m "feat(self-update): periodic InstallFailed retries only on new tag

When pre-tick state is InstallFailed { latest: failed, .. }, the
periodic loop re-runs the network check but installs only when the
live tag differs from the previously failed one. Live UpToDate
clears the failure; CheckFailed preserves it. Two new tests cover
same-tag-skip and new-tag-installs."
```

---

## Task 7: Bump workspace version 0.14.2 → 0.14.3

**Files:**
- Modify: `Cargo.toml` (root)

- [ ] **Step 1: Edit the workspace version + dependency literals**

Update these specific lines (verified against grep output 2026-05-15):

- Line 20: `version = "0.14.2"` → `version = "0.14.3"`
- Line 28: `otto-plugin = { path = "crates/otto-plugin", version = "0.14.2" }` → `version = "0.14.3"`
- Line 29: `otto-protocol = { path = "crates/otto-protocol", version = "0.14.2" }` → `version = "0.14.3"`
- Line 30: `otto-mcp = { path = "crates/otto-mcp", version = "0.14.2" }` → `version = "0.14.3"`
- Line 31: `otto-host = { path = "crates/otto-host", version = "0.14.2" }` → `version = "0.14.3"`

(No per-crate `Cargo.toml` literals — each crate inherits `version.workspace = true`.)

- [ ] **Step 2: Verify build**

Run: `cargo build -p otto`
Expected: success. The `Cargo.lock` will update to reflect the new version.

- [ ] **Step 3: Verify no stray 0.14.2 references**

Run: `grep -rn "0\\.14\\.2" --include="*.toml" --include="*.lock"`
Expected: no matches besides historical CHANGELOG content (Task 8 leaves the 0.14.2 entry intact).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: bump workspace version to 0.14.3"
```

---

## Task 8: Add CHANGELOG entry

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Insert the new entry**

Add the following block immediately after line 7 (just before `## v0.14.2 — Gemini tool calls...` at line 9 — verified against grep output 2026-05-15):

```markdown
## v0.14.3 — Self-update plugin re-checks GitHub Releases every 2 hours (2026-05-15)

### Fixed

- **Long-running TUI sessions now notice new releases.** Previously,
  the `internal:self-update` plugin only consulted the GitHub Releases
  API when the `HostStarting` hook fired (i.e., at TUI launch), so a
  session that stayed open for days would never observe a release
  published mid-session. The spawned check task now runs on a
  `tokio::time::interval` with a 2-hour cadence: the first tick
  preserves today's startup behavior (and the 24h on-disk cache at
  `~/.otto/update-check.json`), while subsequent ticks bypass the
  cache and re-query GitHub. New releases auto-install in exactly the
  same way as the startup path; the banner shows the progression and
  the existing restart hint fires on exit.

  The 2-hour interval is fixed (no env var override yet — file an
  issue if you need one). `MissedTickBehavior::Delay` is set so a
  suspended laptop or a long install never produces a burst of
  catch-up network calls when the system resumes.

  Skip rules per tick:
  - `Disabled` / `Updated` end the loop (opt-out, or binary already
    swapped and awaiting restart).
  - `Installing` skips the tick — covers the case where the user
    typed `/update` and the dispatcher task is parked on the
    installer.
  - `InstallFailed { latest: T }` re-runs the check on every tick to
    pick up any *newer* release GitHub publishes, but skips the
    install when the live check still resolves to `T`. This avoids
    hammering a known-broken release while still recovering
    automatically once a new release lands.

  Closes #78.

```

(Keep the blank line at the end of the block so it separates cleanly from the existing `## v0.14.2` heading.)

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: changelog entry for v0.14.3 (#78)"
```

---

## Task 9: Update README self-update paragraph

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Edit the `/update` row in the slash commands table**

Currently at `README.md:123` (verified against grep output 2026-05-15):

```
| `/update` | Re-run the latest-release install. As of the next release the TUI installs available updates automatically on launch (the banner above the prompt reports progress); `/update` is only needed to retry after a failed install or to force the install before the next polling window. Replaces every binary in the release archive — `otto` plus the six helpers. Opt out with `OTTO_NO_UPDATE_CHECK=1` or `--no-update-check`. |
```

Replace with:

```
| `/update` | Re-run the latest-release install. The TUI checks for new releases on launch AND re-checks every 2 hours while the TUI is open, auto-installing any newer release (the banner above the prompt reports progress). `/update` is only needed to retry after a failed install or to force the install before the next 2-hour tick. Replaces every binary in the release archive — `otto` plus the six helpers. Opt out with `OTTO_NO_UPDATE_CHECK=1` or `--no-update-check`. |
```

- [ ] **Step 2: Edit the env-var description**

Currently at `README.md:317`:

```
| `OTTO_NO_UPDATE_CHECK` | `otto` | (unset) | When set, disables the launch-time version check and `/update`. CLI equivalent: `--no-update-check`. |
```

Replace with:

```
| `OTTO_NO_UPDATE_CHECK` | `otto` | (unset) | When set, disables the launch-time and periodic (2-hour) version check, and the `/update` slash command. CLI equivalent: `--no-update-check`. |
```

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: README mentions periodic self-update re-check (#78)"
```

---

## Task 10: Full local verification

**Goal:** Match CI before pushing.

- [ ] **Step 1: Full workspace build**

Run: `cargo build --workspace --all-targets`
Expected: clean compile.

- [ ] **Step 2: Full test suite**

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 3: Stable-toolchain fmt check**

Run: `rustup run stable cargo fmt --all -- --check`
Expected: no diff. (If diff, run `rustup run stable cargo fmt --all` and re-commit.)

- [ ] **Step 4: Stable-toolchain clippy**

Run: `rustup run stable cargo clippy --workspace --all-targets -- -D warnings`
Expected: zero warnings.

- [ ] **Step 5: No new commit unless fmt/clippy required a fix**

If any of the above produced a fix, commit it with:

```bash
git add -u
git commit -m "chore: clippy/fmt fixups for v0.14.3"
```

Otherwise proceed directly to Task 11.

---

## Task 11: Open PR, verify CI, merge, tag

**Files:**
- No file changes — git/gh operations only. Delegate to the `git-expert` subagent (per `~/.claude/CLAUDE.md`).

- [ ] **Step 1: Push the branch**

Delegate to git-expert with this exact directive:

> Push branch `issue-78-self-update-periodic-recheck` to `origin` with `-u`. From `/home/robhicks/dev/ai-coder`. Confirm the push succeeded and the upstream is set. Do NOT push any other branches. Do NOT force-push.

- [ ] **Step 2: Open the PR**

Delegate to git-expert:

> From `/home/robhicks/dev/ai-coder` on branch `issue-78-self-update-periodic-recheck`. Open a PR against `master`. Title: `feat(self-update): periodic 2h re-check (#78)`. Body should reference issue #78, summarize the change in 2–3 bullets (interval loop with first-tick-immediate, MissedTickBehavior::Delay, skip rules, InstallFailed-tag-changed retry policy), and include a Test plan with cargo test, cargo clippy, and cargo fmt commands. No "Generated with Claude…", no Co-Authored-By trailer, no emoji markers. Use `gh pr create --title ... --body-file -` via heredoc.

Return the PR URL.

- [ ] **Step 3: Wait for CI green**

Run: `gh pr checks <PR-number> --watch`
Expected: all checks pass. Per `feedback_verify_ci_after_push`, do not claim "push is good" until `gh run` confirms green for the head SHA.

If a check fails, read the log via `gh run view <run-id> --log-failed`, fix locally, push, and re-watch.

- [ ] **Step 4: Update issue #78**

Delegate to git-expert:

> Post a comment on issue #78 referencing the PR URL and stating "Fix landed in <SHA>; ships in v0.14.3." Do not close the issue yet — cargo-dist's tag-push workflow does the release.

- [ ] **Step 5: Merge the PR**

Delegate to git-expert:

> Merge PR #<number> using `gh pr merge --squash --delete-branch`. Confirm `origin/master` advanced.

- [ ] **Step 6: Tag v0.14.3**

Delegate to git-expert:

> From local `master` (after pulling the merge), create tag `v0.14.3` and push it to `origin`. Do NOT run `gh release create` — cargo-dist owns the GitHub Release lifecycle on tag push and a manually-created release blocks its workflow (per `feedback_cargo_dist_release`).

- [ ] **Step 7: Confirm cargo-dist release workflow ran**

Run: `gh run list --workflow=release.yml --limit 3`
Expected: a release workflow triggered by the tag push, currently running or completed. If completed green, the GitHub Release with the v0.14.3 binaries is live.

- [ ] **Step 8: Close issue #78**

Once the release is live, delegate to git-expert:

> Close issue #78 with a final comment linking to the v0.14.3 GitHub Release. No self-attribution.

---

## Self-review notes

**Spec coverage:** Every section of the spec is implemented:

- **"Approach"** (interval loop, `MissedTickBehavior::Delay`) → Tasks 3, 4.
- **"Per-tick skip rules" table:**
  - `Disabled` / `Updated` breaks → Task 4
  - `Installing` skip → Task 5
  - `InstallFailed { latest: failed }` retry-only-on-new-tag → Task 6
- **"Cache interaction"** (first tick reads, subsequent bypass, all writes) → Task 4 (depends on Task 1's `&Path` broadening for clean signatures).
- **"Interval"** + `MissedTickBehavior::Delay` → Task 3.
- **"Module changes"** 1–5 in the spec map to Tasks 2, 3, 4.
- **"Tests added"** 1–8 in the spec → all delivered: tests 1–3 + 7 in Task 4, test 8 ("existing stay green") verified by every task running the full module suite, test 3 in Task 5, tests 5–6 in Task 6.
- **"Concurrency notes"** → realized in Task 5's spawn-task structure + the `&Arc<…>` borrows in `run_check_once`.
- **"Versioning, changelog, docs"** → Tasks 7, 8, 9.
- **"Out of scope"** items not implemented here: configurable interval, plugin-trait hook, cache-TTL changes, notify-only mode — confirmed.
- **"Ship plan"** → Tasks 10–11.

**Placeholder scan:** No `TBD`, `TODO`, or "add appropriate X" lines. Every code step contains the complete code. Every command step contains the exact command + expected output.

**Type consistency:** `run_check_once` signature evolves: Task 2 = 6 params, Task 4 adds `is_first_tick: bool`, Task 6 adds `pre_state: UpdateState`. Each task explicitly redeclares the full signature in its step, and updates the single call site accordingly. `CountingFetcher`, `BlockingStubInstaller`, and the `advance_*` helpers are introduced in the task that first uses them and referenced consistently thereafter.
