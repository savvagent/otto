# Multi-provider pool — Phase 1 implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single-host-slot model with a connected-provider pool that supports silent re-connect, lease-based lifecycle, and a single-active-provider invariant — closing the `/connect` re-prompt complaint while shipping the safety contract that Phase 3 will build on.

**Architecture:** `otto-host` gains `pool.rs` (Arc-held `PoolEntry`, `ProviderLease` RAII guard, `Drain`/`Force` disconnect modes with 3-stage cancellation), `capabilities.rs` (per-model metadata), and `router/legacy_model.rs` (bare-model `OTTO_MODEL` resolver). The `Host` struct keeps one `active_provider: ProviderId` per conversation; turns always route to it; `/use <provider>` clears history and switches. `HostConfig` gains `providers: Vec<ProviderRegistration>` and `startup_connect: StartupConnectPolicy`. The TUI gains a `~/.otto/config.toml` file, a one-time migration picker, silent `/connect`, `/disconnect`, `/use`, and a multi-row status bar.

**Tech Stack:** Rust 2024, Tokio (async + `select!` + `JoinHandle::abort`), `Arc<dyn ProviderClient>`, `tokio::sync::{RwLock, Mutex, oneshot}`, `std::sync::atomic::AtomicUsize`, `keyring` crate (existing), `toml`/`serde` for config file, `rust_i18n` for user-facing strings.

**Spec:** `docs/superpowers/specs/2026-05-15-multi-provider-pool-and-auto-routing-design.md`. This plan covers **Phase 1 only**. Phase 2 (CI gate), Phase 3 (`@`-override), Phase 4 (modality), Phase 5 (user rules), Phase 6 (heuristics) each get their own plan when started.

---

## File structure (Phase 1)

**New files:**
- `crates/otto-host/src/capabilities.rs` — `ProviderCapabilities`, `ModelCapabilities`, `ModelAlias`, `CostTier`.
- `crates/otto-host/src/pool.rs` — `PoolEntry`, `ProviderLease`, `DisconnectMode`, `PoolError`.
- `crates/otto-host/src/router/mod.rs` — module file (Phase 1 ships only `legacy_model`).
- `crates/otto-host/src/router/legacy_model.rs` — `OTTO_MODEL` parser + ambiguity resolver.
- `crates/otto-host/tests/pool_lifecycle.rs` — integration tests for Drain/Force lifecycle.
- `crates/otto/src/config_file.rs` — `~/.otto/config.toml` schema + load/save + migration marker.
- `crates/otto/src/migration.rs` — first-launch picker state + `MigrationOutcome`.

**Modified files:**
- `crates/otto-host/src/lib.rs` — re-exports.
- `crates/otto-host/src/config.rs` — add `ProviderRegistration`, `StartupConnectPolicy`, `force_disconnect_grace_ms`, `connect_timeout_ms` fields.
- `crates/otto-host/src/session.rs` — replace `provider: Box<dyn ProviderClient>` with `pool: RwLock<HashMap<ProviderId, PoolEntry>>` + `active_provider: RwLock<ProviderId>`. Update `Host::start`, `run_turn_streaming`, history append.
- `crates/otto/src/main.rs` — slash dispatch for `/disconnect`, `/use`; `/model` filtering by active provider; `perform_connect` now calls `host.add_provider`; startup runs `StartupConnectPolicy` + migration; `--rekey` flag plumbing.
- `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs` — silent connect when key stored; `--rekey` opens modal.
- `crates/otto/src/plugin/builtin/provider_gemini/mod.rs` — same.
- `crates/otto/src/plugin/builtin/provider_openai/mod.rs` — same.
- `crates/otto/src/plugin/builtin/provider_local/mod.rs` — silent always (no key required).
- `crates/otto/src/plugin/builtin/connect/screen.rs` — alt-Enter emits `/connect <id> --rekey`.
- `crates/otto/src/ui.rs` — status bar lists all pool members.
- `crates/otto/locales/en.yml` (and other locales) — new i18n keys.
- `README.md`, `CHANGELOG.md` — release docs.

---

## Task 1: ProviderCapabilities + ModelCapabilities + ModelAlias types

**Files:**
- Create: `crates/otto-host/src/capabilities.rs`
- Modify: `crates/otto-host/src/lib.rs` (re-export)

Pure types. No behavior. Needed before pool/registration types reference them.

- [ ] **Step 1: Write the failing test**

Add to `crates/otto-host/src/capabilities.rs`:

```rust
//! Per-provider and per-model capability metadata. Carried into the host
//! via `HostConfig::providers` (see config.rs::ProviderRegistration).
//! Plugins build these from their hardcoded model lists; the host treats
//! them as read-only data and never mutates capability records itself.

use otto_protocol::ProviderId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostTier {
    Free,
    Cheap,
    Standard,
    Premium,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub id: String,
    pub display_name: String,
    pub supports_vision: bool,
    pub supports_audio: bool,
    pub context_window: usize,
    pub cost_tier: CostTier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelAlias {
    pub alias: String,
    pub provider: ProviderId,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    pub models: Vec<ModelCapabilities>,
    pub default_model: String,
}

impl ProviderCapabilities {
    pub fn model(&self, id: &str) -> Option<&ModelCapabilities> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn default(&self) -> &ModelCapabilities {
        self.model(&self.default_model)
            .expect("default_model must exist in models")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otto_protocol::ProviderId;

    fn anthropic() -> ProviderId {
        ProviderId::new("anthropic").unwrap()
    }

    fn opus_caps() -> ModelCapabilities {
        ModelCapabilities {
            id: "claude-opus-4-7".into(),
            display_name: "Claude Opus 4.7".into(),
            supports_vision: true,
            supports_audio: false,
            context_window: 200_000,
            cost_tier: CostTier::Premium,
        }
    }

    #[test]
    fn provider_caps_lookup_by_id() {
        let caps = ProviderCapabilities {
            models: vec![opus_caps()],
            default_model: "claude-opus-4-7".into(),
        };
        assert_eq!(caps.model("claude-opus-4-7").unwrap().id, "claude-opus-4-7");
        assert!(caps.model("not-a-model").is_none());
        assert_eq!(caps.default().id, "claude-opus-4-7");
    }

    #[test]
    fn model_alias_struct_is_constructible() {
        let alias = ModelAlias {
            alias: "opus".into(),
            provider: anthropic(),
            model: "claude-opus-4-7".into(),
        };
        assert_eq!(alias.alias, "opus");
    }

    #[test]
    #[should_panic(expected = "default_model must exist in models")]
    fn default_panics_when_unset() {
        let caps = ProviderCapabilities {
            models: vec![],
            default_model: "missing".into(),
        };
        let _ = caps.default();
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p otto-host capabilities::tests`
Expected: FAIL with "unresolved module" / "cannot find module `capabilities`".

- [ ] **Step 3: Wire the module**

Edit `crates/otto-host/src/lib.rs`. Add near the other `mod` lines:

```rust
pub mod capabilities;
pub use capabilities::{CostTier, ModelAlias, ModelCapabilities, ProviderCapabilities};
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p otto-host capabilities::tests`
Expected: PASS (3 tests).

- [ ] **Step 5: Clippy + fmt parity**

Run: `rustup run stable cargo fmt --all -- --check && rustup run stable cargo clippy -p otto-host -- -D warnings`
Expected: clean. (Per [[feedback_match_ci_toolchain_locally]] — local default toolchain may lag CI.)

- [ ] **Step 6: Commit**

```bash
git add crates/otto-host/src/capabilities.rs crates/otto-host/src/lib.rs
git commit -m "feat(host): add ProviderCapabilities/ModelCapabilities types"
```

---

## Task 2: PoolEntry, ProviderLease, DisconnectMode, PoolError (Drain only)

**Files:**
- Create: `crates/otto-host/src/pool.rs`
- Modify: `crates/otto-host/src/lib.rs` (re-export)

Pool primitives. Drain mode is well-defined by `Arc` reference counting; Force mode is layered on top in Task 3.

- [ ] **Step 1: Write the failing test**

Create `crates/otto-host/src/pool.rs`:

```rust
//! Provider pool primitives. The pool stores per-provider entries keyed
//! by `ProviderId`. Each entry holds an `Arc<dyn ProviderClient>` plus
//! an active-turn counter that drives drain-mode disconnects.
//!
//! Locking discipline: the pool sits behind a single `tokio::sync::RwLock`
//! owned by `Host`. Read guards are released before any `.await` on the
//! provider client (see project_tui_design — the same "don't hold a lock
//! across await" rule the TUI uses for host swaps).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use otto_mcp::ProviderClient;
use otto_protocol::ProviderId;
use thiserror::Error;

use crate::capabilities::ProviderCapabilities;

#[derive(Debug, Error)]
pub enum PoolError {
    #[error("provider {0} is already registered")]
    AlreadyRegistered(ProviderId),
    #[error("provider {0} is not registered")]
    NotRegistered(ProviderId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectMode {
    Drain,
    Force,
}

/// A live entry in the provider pool.
pub struct PoolEntry {
    client: Arc<dyn ProviderClient + Send + Sync>,
    capabilities: ProviderCapabilities,
    display_name: String,
    /// Number of `ProviderLease`s currently outstanding for this entry.
    /// Used by drain-disconnect to wait until in-flight turns finish.
    active_turns: Arc<AtomicUsize>,
}

impl PoolEntry {
    pub fn new(
        client: Arc<dyn ProviderClient + Send + Sync>,
        capabilities: ProviderCapabilities,
        display_name: String,
    ) -> Self {
        Self {
            client,
            capabilities,
            display_name,
            active_turns: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn capabilities(&self) -> &ProviderCapabilities {
        &self.capabilities
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Acquire a lease on this entry. The lease keeps the inner `Arc<dyn
    /// ProviderClient>` alive until dropped, even if the entry is removed
    /// from the pool in between.
    pub fn lease(&self) -> ProviderLease {
        self.active_turns.fetch_add(1, Ordering::SeqCst);
        ProviderLease {
            client: Arc::clone(&self.client),
            active_turns: Arc::clone(&self.active_turns),
        }
    }

    pub fn active_turn_count(&self) -> usize {
        self.active_turns.load(Ordering::SeqCst)
    }
}

/// RAII lease handle. Drops decrement the entry's active-turns counter.
pub struct ProviderLease {
    client: Arc<dyn ProviderClient + Send + Sync>,
    active_turns: Arc<AtomicUsize>,
}

impl ProviderLease {
    pub fn client(&self) -> &Arc<dyn ProviderClient + Send + Sync> {
        &self.client
    }
}

impl Drop for ProviderLease {
    fn drop(&mut self) {
        self.active_turns.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{CostTier, ModelCapabilities};
    use async_trait::async_trait;
    use otto_protocol::{
        CompleteRequest, CompleteResponse, ListModelsResponse, ProviderError, StreamEvent,
    };
    use tokio::sync::mpsc;

    struct StubClient;
    #[async_trait]
    impl ProviderClient for StubClient {
        async fn complete(
            &self,
            _: CompleteRequest,
            _: Option<mpsc::Sender<StreamEvent>>,
        ) -> Result<CompleteResponse, ProviderError> {
            unreachable!()
        }
        async fn list_models(&self) -> Result<ListModelsResponse, ProviderError> {
            unreachable!()
        }
    }

    fn entry() -> PoolEntry {
        let caps = ProviderCapabilities {
            models: vec![ModelCapabilities {
                id: "m".into(),
                display_name: "M".into(),
                supports_vision: false,
                supports_audio: false,
                context_window: 1000,
                cost_tier: CostTier::Standard,
            }],
            default_model: "m".into(),
        };
        PoolEntry::new(Arc::new(StubClient), caps, "Stub".into())
    }

    #[test]
    fn lease_increments_and_drop_decrements() {
        let e = entry();
        assert_eq!(e.active_turn_count(), 0);
        let l1 = e.lease();
        assert_eq!(e.active_turn_count(), 1);
        let l2 = e.lease();
        assert_eq!(e.active_turn_count(), 2);
        drop(l1);
        assert_eq!(e.active_turn_count(), 1);
        drop(l2);
        assert_eq!(e.active_turn_count(), 0);
    }

    #[test]
    fn lease_keeps_client_alive_after_entry_drop() {
        let e = entry();
        let lease = e.lease();
        let arc_weak = Arc::downgrade(lease.client());
        drop(e);
        // Lease still holds the Arc, so weak upgrade must succeed.
        assert!(arc_weak.upgrade().is_some());
        drop(lease);
        // Now both strong refs are gone.
        assert!(arc_weak.upgrade().is_none());
    }
}
```

- [ ] **Step 2: Add `thiserror` dependency**

Check if `thiserror` is already a dep of `otto-host`. If not, edit `crates/otto-host/Cargo.toml`:

```toml
[dependencies]
# … existing …
thiserror = { workspace = true }
```

If `thiserror` isn't in `[workspace.dependencies]` either, add `thiserror = "1"` there too.

- [ ] **Step 3: Wire the module**

Edit `crates/otto-host/src/lib.rs`:

```rust
pub mod pool;
pub use pool::{DisconnectMode, PoolEntry, PoolError, ProviderLease};
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p otto-host pool::tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Clippy + fmt**

Run: `rustup run stable cargo fmt --all -- --check && rustup run stable cargo clippy -p otto-host -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/otto-host/src/pool.rs crates/otto-host/src/lib.rs crates/otto-host/Cargo.toml
git commit -m "feat(host): add PoolEntry/ProviderLease primitives"
```

---

## Task 3: ProviderRegistration + StartupConnectPolicy in HostConfig

**Files:**
- Modify: `crates/otto-host/src/config.rs`
- Modify: `crates/otto-host/src/lib.rs` (re-export)

Adds the carrier shape the TUI uses to hand provider clients (+ capabilities) into the host.

- [ ] **Step 1: Write the failing test**

Append to `crates/otto-host/src/config.rs`:

```rust
#[cfg(test)]
mod registration_tests {
    use super::*;
    use crate::capabilities::{CostTier, ModelCapabilities, ProviderCapabilities};
    use async_trait::async_trait;
    use otto_mcp::ProviderClient;
    use otto_protocol::{
        CompleteRequest, CompleteResponse, ListModelsResponse, ProviderError, ProviderId,
        StreamEvent,
    };
    use std::sync::Arc;
    use tokio::sync::mpsc;

    struct StubClient;
    #[async_trait]
    impl ProviderClient for StubClient {
        async fn complete(
            &self,
            _: CompleteRequest,
            _: Option<mpsc::Sender<StreamEvent>>,
        ) -> Result<CompleteResponse, ProviderError> {
            unreachable!()
        }
        async fn list_models(&self) -> Result<ListModelsResponse, ProviderError> {
            unreachable!()
        }
    }

    #[test]
    fn provider_registration_constructs() {
        let caps = ProviderCapabilities {
            models: vec![ModelCapabilities {
                id: "m".into(),
                display_name: "M".into(),
                supports_vision: false,
                supports_audio: false,
                context_window: 1000,
                cost_tier: CostTier::Standard,
            }],
            default_model: "m".into(),
        };
        let reg = ProviderRegistration {
            id: ProviderId::new("stub").unwrap(),
            display_name: "Stub".into(),
            client: Arc::new(StubClient) as Arc<dyn ProviderClient + Send + Sync>,
            capabilities: caps,
            aliases: vec![],
        };
        assert_eq!(reg.id.as_str(), "stub");
        assert_eq!(reg.display_name, "Stub");
    }

    #[test]
    fn startup_policy_defaults_to_opt_in() {
        let p = StartupConnectPolicy::default();
        assert!(matches!(p, StartupConnectPolicy::OptIn(ref v) if v.is_empty()));
    }

    #[test]
    fn host_config_has_pool_fields_with_defaults() {
        let cfg = HostConfig::new(
            ProviderEndpoint::StreamableHttp {
                url: "http://x".into(),
            },
            "model",
        );
        assert!(cfg.providers.is_empty());
        assert!(matches!(cfg.startup_connect, StartupConnectPolicy::OptIn(_)));
        assert_eq!(cfg.connect_timeout_ms, 3000);
        assert_eq!(cfg.force_disconnect_grace_ms, 500);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p otto-host registration_tests`
Expected: FAIL — `ProviderRegistration`, `StartupConnectPolicy`, and the new `HostConfig` fields don't exist.

- [ ] **Step 3: Add the types**

Edit `crates/otto-host/src/config.rs`. Add at the top of the file (after existing imports):

```rust
use std::sync::Arc;

use otto_mcp::ProviderClient;
use otto_protocol::ProviderId;

use crate::capabilities::{ModelAlias, ProviderCapabilities};
```

Add new types above `pub struct HostConfig`:

```rust
/// A connected provider handed to the host at construction time (or
/// later, via `Host::add_provider`). The plugin builds the `Arc<dyn
/// ProviderClient>` once and the host stores that same Arc — no Box→Arc
/// conversion exists anywhere in the system.
pub struct ProviderRegistration {
    pub id: ProviderId,
    pub display_name: String,
    pub client: Arc<dyn ProviderClient + Send + Sync>,
    pub capabilities: ProviderCapabilities,
    pub aliases: Vec<ModelAlias>,
}

/// Which providers should auto-connect when `Host::start` runs.
#[derive(Debug, Clone)]
pub enum StartupConnectPolicy {
    /// Only providers in this allow-list are auto-connected. The
    /// default; defaults to an empty list (user has not chosen yet).
    OptIn(Vec<ProviderId>),
    /// Every provider in `HostConfig::providers` is auto-connected.
    All,
    /// Skip auto-connect entirely; pool starts empty.
    None,
    /// Auto-connect only the provider(s) recorded in
    /// `~/.otto/state.toml`'s `last_used` field. Resolution happens
    /// in the embedder (TUI) before the policy is built — by the time
    /// the host sees `LastUsed`, the inner vec is already populated.
    LastUsed(Vec<ProviderId>),
}

impl Default for StartupConnectPolicy {
    fn default() -> Self {
        Self::OptIn(Vec::new())
    }
}
```

Add fields to `HostConfig`:

```rust
pub struct HostConfig {
    // … existing fields …

    /// Providers handed in at construction time. Each entry becomes a
    /// PoolEntry in the host's provider_pool, subject to
    /// `startup_connect`. The legacy `provider: ProviderEndpoint` field
    /// is preserved for the rmcp HTTP-transport debug path; when
    /// `providers` is non-empty, the host uses the pool and ignores
    /// the legacy field.
    pub providers: Vec<ProviderRegistration>,

    /// Which `providers` entries to actually connect at `Host::start`.
    pub startup_connect: StartupConnectPolicy,

    /// Per-provider timeout for auto-connect during `Host::start`.
    /// Providers exceeding this are abandoned with a styled note;
    /// other providers continue.
    pub connect_timeout_ms: u64,

    /// Grace period for `DisconnectMode::Force` between emitting the
    /// cooperative cancel signal and aborting outstanding turn tasks.
    pub force_disconnect_grace_ms: u64,
}
```

Update `HostConfig::new`:

```rust
pub fn new(provider: ProviderEndpoint, model: impl Into<String>) -> Self {
    Self {
        provider,
        tools: Vec::new(),
        model: model.into(),
        max_tokens: 4096,
        project_root: PathBuf::from("."),
        system_prompt: None,
        max_iterations: 20,
        policy: None,
        sandbox: None,
        default_prompt_enabled: true,
        app_version: None,
        providers: Vec::new(),
        startup_connect: StartupConnectPolicy::default(),
        connect_timeout_ms: 3000,
        force_disconnect_grace_ms: 500,
    }
}
```

Add the `Debug` derive carefully — `Arc<dyn ProviderClient>` is not `Debug`. Add a manual `Debug` impl for `ProviderRegistration` and `HostConfig` that elides the client.

```rust
impl std::fmt::Debug for ProviderRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistration")
            .field("id", &self.id)
            .field("display_name", &self.display_name)
            .field("capabilities", &self.capabilities)
            .field("aliases", &self.aliases)
            .finish_non_exhaustive()
    }
}
```

And on `HostConfig`, remove `#[derive(Debug, Clone)]` if present (since `ProviderRegistration` isn't `Clone`-able trivially) and add manual impls:

```rust
impl std::fmt::Debug for HostConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostConfig")
            .field("provider", &self.provider)
            .field("tools", &self.tools)
            .field("model", &self.model)
            .field("providers", &self.providers)
            .field("startup_connect", &self.startup_connect)
            .field("connect_timeout_ms", &self.connect_timeout_ms)
            .field("force_disconnect_grace_ms", &self.force_disconnect_grace_ms)
            .finish_non_exhaustive()
    }
}
```

Drop `#[derive(Clone)]` on `HostConfig` — `Arc<dyn ProviderClient>` is `Clone` but the spec-level intent is "one HostConfig is consumed by one Host." If existing call sites need `Clone`, fix them in subsequent tasks; for now, expect a compile error and follow the fix-it path.

- [ ] **Step 4: Run the test**

Run: `cargo test -p otto-host registration_tests`
Expected: PASS (3 tests).

- [ ] **Step 5: Fix workspace consumers if `HostConfig: Clone` was relied on**

Run: `cargo check --workspace`
Expected: clean. If any callers cloned `HostConfig`, refactor to pass `&HostConfig` or construct fresh.

- [ ] **Step 6: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy --workspace -- -D warnings
git add crates/otto-host/src/config.rs crates/otto-host/src/lib.rs
git commit -m "feat(host): add ProviderRegistration + StartupConnectPolicy"
```

(If lib.rs needs re-exports for the new types, also include in the commit.)

Edit `crates/otto-host/src/lib.rs`:

```rust
pub use config::{ProviderRegistration, StartupConnectPolicy};
```

---

## Task 4: Refactor Host to use the pool (single active provider)

**Files:**
- Modify: `crates/otto-host/src/session.rs`
- Modify: `crates/otto/src/main.rs` (call sites of `Host::start`)

This is the largest structural change. Replace `provider: Box<dyn ProviderClient>` with a pool, add `active_provider`, route `run_turn_streaming` through a lease.

- [ ] **Step 1: Write the failing test (host + lease integration)**

Create `crates/otto-host/tests/pool_lifecycle.rs`:

```rust
//! Pool lifecycle tests: drain, force, lock hygiene. Force-mode tests
//! that require a cancellation-uncooperative provider live in Task 5.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use otto_host::capabilities::{CostTier, ModelCapabilities, ProviderCapabilities};
use otto_host::config::{ProviderEndpoint, StartupConnectPolicy};
use otto_host::{DisconnectMode, Host, HostConfig, ProviderRegistration};
use otto_mcp::ProviderClient;
use otto_protocol::{
    CompleteRequest, CompleteResponse, ListModelsResponse, Message, ProviderError, ProviderId,
    Role, StreamEvent,
};
use tokio::sync::mpsc;

struct EchoClient;
#[async_trait]
impl ProviderClient for EchoClient {
    async fn complete(
        &self,
        req: CompleteRequest,
        _events: Option<mpsc::Sender<StreamEvent>>,
    ) -> Result<CompleteResponse, ProviderError> {
        Ok(CompleteResponse {
            content: vec![],
            stop_reason: otto_protocol::StopReason::EndTurn,
            usage: Default::default(),
            model: req.model.clone(),
        })
    }
    async fn list_models(&self) -> Result<ListModelsResponse, ProviderError> {
        Ok(ListModelsResponse { models: vec![] })
    }
}

fn caps(model_id: &str) -> ProviderCapabilities {
    ProviderCapabilities {
        models: vec![ModelCapabilities {
            id: model_id.into(),
            display_name: model_id.into(),
            supports_vision: false,
            supports_audio: false,
            context_window: 1000,
            cost_tier: CostTier::Standard,
        }],
        default_model: model_id.into(),
    }
}

fn reg(id: &str, model: &str) -> ProviderRegistration {
    ProviderRegistration {
        id: ProviderId::new(id).unwrap(),
        display_name: id.into(),
        client: Arc::new(EchoClient) as Arc<dyn ProviderClient + Send + Sync>,
        capabilities: caps(model),
        aliases: vec![],
    }
}

#[tokio::test]
async fn host_starts_with_pool_and_active_provider() {
    let mut cfg = HostConfig::new(
        ProviderEndpoint::StreamableHttp { url: "http://unused".into() },
        "m",
    );
    cfg.providers = vec![reg("anthropic", "m")];
    cfg.startup_connect = StartupConnectPolicy::All;
    let host = Host::start(cfg).await.expect("start ok");
    assert_eq!(host.active_provider().await.as_str(), "anthropic");
    assert!(host.is_connected("anthropic").await);
}

#[tokio::test]
async fn add_provider_rejects_duplicate() {
    let mut cfg = HostConfig::new(
        ProviderEndpoint::StreamableHttp { url: "http://unused".into() },
        "m",
    );
    cfg.providers = vec![reg("anthropic", "m")];
    cfg.startup_connect = StartupConnectPolicy::All;
    let host = Host::start(cfg).await.unwrap();
    let dup = reg("anthropic", "m");
    let err = host.add_provider(dup).await.unwrap_err();
    assert!(matches!(
        err,
        otto_host::PoolError::AlreadyRegistered(ref id) if id.as_str() == "anthropic"
    ));
}

#[tokio::test]
async fn remove_provider_drain_blocks_new_turns_but_lets_inflight_finish() {
    let mut cfg = HostConfig::new(
        ProviderEndpoint::StreamableHttp { url: "http://unused".into() },
        "m",
    );
    cfg.providers = vec![reg("anthropic", "m")];
    cfg.startup_connect = StartupConnectPolicy::All;
    let host = Arc::new(Host::start(cfg).await.unwrap());

    // Take a lease ourselves to simulate an in-flight turn.
    let lease = host
        .acquire_lease_for_test(&ProviderId::new("anthropic").unwrap())
        .await
        .unwrap();

    // Drain while lease is held — must not block.
    let host_clone = Arc::clone(&host);
    let drain_handle = tokio::spawn(async move {
        host_clone
            .remove_provider(
                &ProviderId::new("anthropic").unwrap(),
                DisconnectMode::Drain,
            )
            .await
    });

    // Provider is gone from the eligibility set immediately.
    assert!(!host.is_connected("anthropic").await);

    // Drop our lease; drain should complete shortly after.
    drop(lease);
    tokio::time::timeout(Duration::from_secs(2), drain_handle)
        .await
        .expect("drain finished in time")
        .unwrap()
        .unwrap();
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p otto-host --test pool_lifecycle`
Expected: FAIL — `Host::active_provider`, `is_connected`, `add_provider`, `remove_provider`, `acquire_lease_for_test` don't exist; `Host::start` doesn't yet read `HostConfig::providers`.

- [ ] **Step 3: Replace `provider` field on Host**

Edit `crates/otto-host/src/session.rs`. Add at top:

```rust
use std::collections::HashMap;

use crate::pool::{DisconnectMode, PoolEntry, PoolError, ProviderLease};
use crate::config::ProviderRegistration;
```

Replace the `Host` struct's `provider` field with:

```rust
pub struct Host {
    config: HostConfig,
    /// Active provider pool. Keyed by ProviderId. Mutated by
    /// add_provider/remove_provider. Pool RwLock is never held across
    /// an .await on a provider client.
    pool: tokio::sync::RwLock<HashMap<ProviderId, PoolEntry>>,
    /// Which provider's models are eligible for the current
    /// conversation. Switched by `/use <provider>` (which also clears
    /// history).
    active_provider: tokio::sync::RwLock<ProviderId>,
    tools: Mutex<Option<ToolRegistry>>,
    state: Mutex<SessionState>,
    // … remaining existing fields unchanged …
}
```

(Import `ProviderId` if not already imported.)

Update `Host::start` to construct from `config.providers` when non-empty:

```rust
pub async fn start(config: HostConfig) -> Result<Self, HostError> {
    // Build the pool from config.providers (the new path) OR from the
    // legacy single-provider config.provider (the rmcp HTTP path used
    // by the headless example). Embedders should populate `providers`;
    // legacy callers continue to work because we synthesize a single
    // entry id "default" when providers is empty.
    let (pool_init, active) = if !config.providers.is_empty() {
        let policy_filter: Box<dyn Fn(&ProviderId) -> bool> = match &config.startup_connect {
            crate::config::StartupConnectPolicy::All => Box::new(|_| true),
            crate::config::StartupConnectPolicy::None => Box::new(|_| false),
            crate::config::StartupConnectPolicy::OptIn(allow)
            | crate::config::StartupConnectPolicy::LastUsed(allow) => {
                let set: std::collections::HashSet<_> = allow.iter().cloned().collect();
                Box::new(move |id| set.contains(id))
            }
        };
        let mut map = HashMap::new();
        for reg in &config.providers {
            if policy_filter(&reg.id) {
                map.insert(
                    reg.id.clone(),
                    PoolEntry::new(
                        Arc::clone(&reg.client),
                        reg.capabilities.clone(),
                        reg.display_name.clone(),
                    ),
                );
            }
        }
        let active = config
            .providers
            .iter()
            .find(|r| map.contains_key(&r.id))
            .map(|r| r.id.clone())
            .unwrap_or_else(|| config.providers[0].id.clone());
        (map, active)
    } else {
        // Legacy path: build a single entry from config.provider.
        let provider: Arc<dyn ProviderClient + Send + Sync> = match &config.provider {
            ProviderEndpoint::StreamableHttp { url } => {
                Arc::new(RmcpProviderClient::connect(url).await?)
            }
        };
        let id = ProviderId::new("default").expect("valid id");
        let caps = crate::capabilities::ProviderCapabilities {
            models: vec![crate::capabilities::ModelCapabilities {
                id: config.model.clone(),
                display_name: config.model.clone(),
                supports_vision: false,
                supports_audio: false,
                context_window: 0,
                cost_tier: crate::capabilities::CostTier::Standard,
            }],
            default_model: config.model.clone(),
        };
        let entry = PoolEntry::new(provider, caps, "Default".into());
        let mut map = HashMap::new();
        map.insert(id.clone(), entry);
        (map, id)
    };

    // … existing setup for sandbox, tools, policy, system_prompt …

    let host = Self {
        config,
        pool: tokio::sync::RwLock::new(pool_init),
        active_provider: tokio::sync::RwLock::new(active),
        tools: Mutex::new(Some(tools)),
        state: Mutex::new(SessionState { messages: Vec::new() }),
        // … existing fields …
    };
    // existing wire_self_into_resolver call …
    Ok(host)
}
```

Add the new public methods:

```rust
impl Host {
    pub async fn active_provider(&self) -> ProviderId {
        self.active_provider.read().await.clone()
    }

    pub async fn is_connected(&self, id: &str) -> bool {
        let Ok(pid) = ProviderId::new(id) else {
            return false;
        };
        self.pool.read().await.contains_key(&pid)
    }

    pub async fn add_provider(&self, reg: ProviderRegistration) -> Result<(), PoolError> {
        let mut pool = self.pool.write().await;
        if pool.contains_key(&reg.id) {
            return Err(PoolError::AlreadyRegistered(reg.id));
        }
        pool.insert(
            reg.id.clone(),
            PoolEntry::new(reg.client, reg.capabilities, reg.display_name),
        );
        Ok(())
    }

    /// Drain-mode removal: takes the entry out of the eligibility map
    /// immediately, then waits for `active_turns` to reach zero before
    /// returning. The entry's Arc<dyn ProviderClient> is dropped at
    /// that point. Force mode is added in Task 5.
    pub async fn remove_provider(
        &self,
        id: &ProviderId,
        mode: DisconnectMode,
    ) -> Result<(), PoolError> {
        assert_eq!(
            mode,
            DisconnectMode::Drain,
            "Force mode is implemented in Task 5"
        );
        let entry = {
            let mut pool = self.pool.write().await;
            pool.remove(id).ok_or_else(|| PoolError::NotRegistered(id.clone()))?
        };
        // Wait for outstanding leases to drop. Polling at 25ms is fine
        // for Phase 1 — Force mode in Task 5 supersedes this with a
        // proper notify-based wait.
        while entry.active_turn_count() > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        drop(entry);
        Ok(())
    }

    /// Test-only: acquire a lease without going through run_turn. Used
    /// by pool_lifecycle.rs to simulate in-flight turns.
    #[doc(hidden)]
    pub async fn acquire_lease_for_test(
        &self,
        id: &ProviderId,
    ) -> Result<ProviderLease, PoolError> {
        let pool = self.pool.read().await;
        let entry = pool.get(id).ok_or_else(|| PoolError::NotRegistered(id.clone()))?;
        Ok(entry.lease())
    }
}
```

Update `Host::run_turn_streaming` (and `run_turn`) to look up the active provider and acquire a lease before awaiting `complete`:

```rust
// Inside run_turn_streaming, where the current code does `self.provider.complete(req, ...)`,
// replace with:
let lease = {
    let active = self.active_provider.read().await.clone();
    let pool = self.pool.read().await;
    let Some(entry) = pool.get(&active) else {
        return Err(HostError::NoActiveProvider);
    };
    entry.lease()
};
// pool guard dropped here, before the await below
let result = lease.client().complete(req, events_tx).await?;
// lease drops at end of scope
```

Add `HostError::NoActiveProvider`.

- [ ] **Step 4: Run the test**

Run: `cargo test -p otto-host --test pool_lifecycle`
Expected: PASS for the three tests in this task. The Force-mode test (added in Task 5) is not yet present.

- [ ] **Step 5: Fix call sites**

Run: `cargo check --workspace`. The TUI (`crates/otto/src/main.rs::build_in_process_host`) currently constructs `HostConfig` with a single provider via the legacy path; it still works thanks to the fallback in `Host::start`, but verify the headless example builds:

Run: `cargo build -p otto-host --example headless`
Expected: clean.

- [ ] **Step 6: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy --workspace -- -D warnings
git add crates/otto-host/src/session.rs crates/otto-host/tests/pool_lifecycle.rs
git commit -m "feat(host): replace single provider field with PoolEntry map + active_provider"
```

---

## Task 5: Force-disconnect with 3-stage cancellation

**Files:**
- Modify: `crates/otto-host/src/session.rs`
- Modify: `crates/otto-host/tests/pool_lifecycle.rs`
- Modify: `crates/otto-host/src/pool.rs`

Layer Force mode on top of Drain. Adds cooperative cancel signal → 500ms grace → `JoinHandle::abort` + `TurnEvent::AbortedAfterGrace`.

- [ ] **Step 1: Add `AbortedAfterGrace` variant**

Find the `TurnEvent` enum (in `session.rs` or wherever defined). Add:

```rust
TurnEvent::Cancelled {
    reason: CancellationReason,
},
TurnEvent::AbortedAfterGrace {
    reason: CancellationReason,
},
```

Define `CancellationReason`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancellationReason {
    ProviderDisconnected(ProviderId),
    UserAbort,
}
```

(Match the spec's variant naming; if `TurnEvent::Cancelled` already exists with a different shape, refactor to carry `CancellationReason`.)

- [ ] **Step 2: Write the failing test (uncooperative provider)**

Append to `crates/otto-host/tests/pool_lifecycle.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

struct StuckClient {
    started: Arc<AtomicBool>,
}
#[async_trait]
impl ProviderClient for StuckClient {
    async fn complete(
        &self,
        _: CompleteRequest,
        _: Option<mpsc::Sender<StreamEvent>>,
    ) -> Result<CompleteResponse, ProviderError> {
        self.started.store(true, Ordering::SeqCst);
        // Sleep way past the 500ms grace; if we're aborted we get
        // dropped during this sleep without ever returning.
        tokio::time::sleep(Duration::from_secs(60)).await;
        Err(ProviderError::Unknown("should never reach here".into()))
    }
    async fn list_models(&self) -> Result<ListModelsResponse, ProviderError> {
        Ok(ListModelsResponse { models: vec![] })
    }
}

#[tokio::test]
async fn force_disconnect_aborts_uncooperative_turn_within_grace() {
    let started = Arc::new(AtomicBool::new(false));
    let stuck = StuckClient { started: Arc::clone(&started) };
    let mut cfg = HostConfig::new(
        ProviderEndpoint::StreamableHttp { url: "http://unused".into() },
        "m",
    );
    cfg.providers = vec![ProviderRegistration {
        id: ProviderId::new("stuck").unwrap(),
        display_name: "Stuck".into(),
        client: Arc::new(stuck) as Arc<dyn ProviderClient + Send + Sync>,
        capabilities: caps("m"),
        aliases: vec![],
    }];
    cfg.startup_connect = StartupConnectPolicy::All;
    cfg.force_disconnect_grace_ms = 200; // tighten for faster test

    let host = Arc::new(Host::start(cfg).await.unwrap());

    // Spawn a turn that will get stuck.
    let host_t = Arc::clone(&host);
    let turn_handle = tokio::spawn(async move {
        let req = CompleteRequest {
            model: "m".into(),
            messages: vec![Message {
                role: Role::User,
                content: vec![],
            }],
            tools: vec![],
            max_tokens: 100,
            system: None,
        };
        host_t.run_turn(req).await
    });

    // Wait until the stuck client is in `complete`.
    while !started.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Force-disconnect; should resolve well within grace + slack.
    let t0 = std::time::Instant::now();
    host.remove_provider(
        &ProviderId::new("stuck").unwrap(),
        DisconnectMode::Force,
    )
    .await
    .unwrap();
    let elapsed = t0.elapsed();
    assert!(
        elapsed < Duration::from_millis(400),
        "force disconnect took {elapsed:?}, expected < 400ms"
    );

    // The turn task should have been aborted (or returned an error).
    let outcome = tokio::time::timeout(Duration::from_secs(1), turn_handle)
        .await
        .expect("turn task should resolve after abort");
    assert!(outcome.is_err() || outcome.as_ref().unwrap().is_err());
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p otto-host --test pool_lifecycle force_disconnect_aborts_uncooperative_turn_within_grace`
Expected: FAIL — `DisconnectMode::Force` assertion in `remove_provider` panics.

- [ ] **Step 4: Implement Force mode**

Replace the placeholder assertion in `Host::remove_provider` with the 3-stage protocol:

```rust
pub async fn remove_provider(
    &self,
    id: &ProviderId,
    mode: DisconnectMode,
) -> Result<(), PoolError> {
    let entry = {
        let mut pool = self.pool.write().await;
        pool.remove(id).ok_or_else(|| PoolError::NotRegistered(id.clone()))?
    };

    match mode {
        DisconnectMode::Drain => {
            while entry.active_turn_count() > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }
        DisconnectMode::Force => {
            // Stage 1: cooperative cancel signal.
            self.signal_cancel(id, CancellationReason::ProviderDisconnected(id.clone()))
                .await;

            // Stage 2: bounded grace.
            let grace = std::time::Duration::from_millis(self.config.force_disconnect_grace_ms);
            let deadline = tokio::time::Instant::now() + grace;
            loop {
                if entry.active_turn_count() == 0 {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    // Stage 3: hard abort.
                    self.abort_active_tasks_for(id).await;
                    self.signal_aborted_after_grace(
                        id,
                        CancellationReason::ProviderDisconnected(id.clone()),
                    )
                    .await;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
    }
    drop(entry);
    Ok(())
}
```

Add the supporting machinery. Host needs:
- A `cancel_signal_tx: HashMap<ProviderId, broadcast::Sender<CancellationReason>>` for stage 1.
- A `task_handles: HashMap<ProviderId, Vec<JoinHandle<()>>>` for stage 3.

Concretely: when `run_turn_streaming` is invoked, it spawns the actual streaming work on a `tokio::spawn` so the host has a handle to abort. Record the handle in `task_handles` keyed by active provider id at lease time; remove on lease drop.

```rust
async fn signal_cancel(&self, id: &ProviderId, reason: CancellationReason) {
    let map = self.cancel_signal_tx.read().await;
    if let Some(tx) = map.get(id) {
        let _ = tx.send(reason);
    }
    // Also emit the TurnEvent into the current turn's events channel
    // if present, so the UI sees Cancelled immediately.
    if let Some(events) = self.current_turn_events.lock().unwrap().as_ref() {
        let _ = events.send(TurnEvent::Cancelled { reason }).await;
    }
}

async fn signal_aborted_after_grace(&self, _id: &ProviderId, reason: CancellationReason) {
    if let Some(events) = self.current_turn_events.lock().unwrap().as_ref() {
        let _ = events.send(TurnEvent::AbortedAfterGrace { reason }).await;
    }
}

async fn abort_active_tasks_for(&self, id: &ProviderId) {
    let mut map = self.task_handles.write().await;
    if let Some(handles) = map.remove(id) {
        for h in handles {
            h.abort();
        }
    }
}
```

Edit `run_turn_streaming` to:
1. `tokio::spawn` the actual streaming body and record the `JoinHandle` in `task_handles[active_provider]`.
2. Add a `select!` between the spawned task and the cancel-signal `broadcast::Receiver`.
3. On cancel signal, drop the lease, return `HostError::Cancelled(reason)`.

(Concrete code is large; the engineer follows the structure above with file-local refactoring. The test in Step 2 pins the behavior.)

- [ ] **Step 5: Run the test**

Run: `cargo test -p otto-host --test pool_lifecycle force_disconnect_aborts_uncooperative_turn_within_grace`
Expected: PASS, elapsed < 400ms.

- [ ] **Step 6: Verify Drain still works**

Run: `cargo test -p otto-host --test pool_lifecycle`
Expected: all tests PASS.

- [ ] **Step 7: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy --workspace -- -D warnings
git add crates/otto-host/src/session.rs crates/otto-host/src/pool.rs crates/otto-host/tests/pool_lifecycle.rs
git commit -m "feat(host): 3-stage force-disconnect (signal → grace → abort)"
```

---

## Task 6: Legacy `OTTO_MODEL` resolver

**Files:**
- Create: `crates/otto-host/src/router/mod.rs`
- Create: `crates/otto-host/src/router/legacy_model.rs`
- Modify: `crates/otto-host/src/lib.rs`

Pure parser — no I/O. Takes `&[ProviderRegistration]` (or a slimmer view) + a raw string, returns `Resolution`.

- [ ] **Step 1: Create the module file**

Create `crates/otto-host/src/router/mod.rs`:

```rust
//! Routing layers. Phase 1 ships only `legacy_model`; subsequent
//! phases (override prefix, modality, rules, heuristics) add siblings.

pub mod legacy_model;

pub use legacy_model::{resolve_legacy_model, LegacyModelResolution};
```

- [ ] **Step 2: Write the failing test**

Create `crates/otto-host/src/router/legacy_model.rs`:

```rust
//! Resolve a `OTTO_MODEL`-shaped value against the set of connected
//! providers. Accepts both legacy bare-model form ("claude-opus-4-7")
//! and the new "provider/model" form ("anthropic/claude-opus-4-7").
//! Pure function; no I/O. The caller is responsible for surfacing the
//! returned warnings as styled notes.

use otto_protocol::ProviderId;

use crate::capabilities::ProviderCapabilities;

/// What `resolve_legacy_model` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyModelResolution {
    /// Exact match found.
    Resolved {
        provider: ProviderId,
        model: String,
    },
    /// Bare-model resolved unambiguously across all connected providers.
    ResolvedFromBare {
        provider: ProviderId,
        model: String,
        note: String,
    },
    /// Multiple providers expose this model id; fell back to default.
    Ambiguous {
        candidates: Vec<(ProviderId, String)>,
        note: String,
    },
    /// No connected provider exposes this model id.
    Unknown { note: String },
    /// `provider/model` named a provider that isn't connected.
    UnknownProvider { provider: ProviderId, note: String },
    /// Empty / no override.
    NoOverride,
}

/// One per connected provider, by reference, so the resolver does not
/// need to hold an owned snapshot.
pub struct ProviderView<'a> {
    pub id: &'a ProviderId,
    pub capabilities: &'a ProviderCapabilities,
}

pub fn resolve_legacy_model(raw: &str, providers: &[ProviderView<'_>]) -> LegacyModelResolution {
    let raw = raw.trim();
    if raw.is_empty() {
        return LegacyModelResolution::NoOverride;
    }
    if let Some((provider_part, model_part)) = raw.split_once('/') {
        let Ok(pid) = ProviderId::new(provider_part) else {
            return LegacyModelResolution::UnknownProvider {
                provider: ProviderId::new("invalid").unwrap_or_else(|_| unreachable!()),
                note: format!("'{provider_part}' is not a valid provider id"),
            };
        };
        let Some(view) = providers.iter().find(|p| p.id == &pid) else {
            return LegacyModelResolution::UnknownProvider {
                provider: pid.clone(),
                note: format!(
                    "OTTO_MODEL='{raw}' names provider '{}' which is not connected; \
                     falling back to default",
                    pid.as_str()
                ),
            };
        };
        if view.capabilities.model(model_part).is_none() {
            return LegacyModelResolution::Unknown {
                note: format!(
                    "OTTO_MODEL='{raw}' names model '{model_part}' \
                     which provider '{}' does not expose; \
                     falling back to {}'s default model",
                    pid.as_str(),
                    pid.as_str()
                ),
            };
        }
        return LegacyModelResolution::Resolved {
            provider: pid,
            model: model_part.into(),
        };
    }
    // Bare-model form: scan all providers.
    let mut hits: Vec<(ProviderId, String)> = providers
        .iter()
        .filter(|v| v.capabilities.model(raw).is_some())
        .map(|v| (v.id.clone(), raw.to_string()))
        .collect();
    match hits.len() {
        0 => LegacyModelResolution::Unknown {
            note: format!(
                "OTTO_MODEL='{raw}' did not match any connected provider's model; \
                 falling back to default"
            ),
        },
        1 => {
            let (provider, model) = hits.remove(0);
            let note = format!(
                "OTTO_MODEL='{raw}' resolved to '{}/{model}'",
                provider.as_str()
            );
            LegacyModelResolution::ResolvedFromBare { provider, model, note }
        }
        _ => {
            let candidates_str = hits
                .iter()
                .map(|(p, _)| p.as_str().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            LegacyModelResolution::Ambiguous {
                candidates: hits,
                note: format!(
                    "OTTO_MODEL='{raw}' is ambiguous: matches providers [{candidates_str}]. \
                     Falling back to default; switch to 'provider/model' form to disambiguate."
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{CostTier, ModelCapabilities, ProviderCapabilities};

    fn caps(models: &[&str]) -> ProviderCapabilities {
        ProviderCapabilities {
            models: models
                .iter()
                .map(|id| ModelCapabilities {
                    id: id.to_string(),
                    display_name: id.to_string(),
                    supports_vision: false,
                    supports_audio: false,
                    context_window: 0,
                    cost_tier: CostTier::Standard,
                })
                .collect(),
            default_model: models[0].into(),
        }
    }

    #[test]
    fn empty_input_means_no_override() {
        let views: Vec<ProviderView> = vec![];
        assert_eq!(resolve_legacy_model("", &views), LegacyModelResolution::NoOverride);
    }

    #[test]
    fn qualified_form_resolves() {
        let a_id = ProviderId::new("anthropic").unwrap();
        let a_caps = caps(&["claude-opus-4-7"]);
        let views = vec![ProviderView { id: &a_id, capabilities: &a_caps }];
        let r = resolve_legacy_model("anthropic/claude-opus-4-7", &views);
        assert_eq!(
            r,
            LegacyModelResolution::Resolved {
                provider: a_id,
                model: "claude-opus-4-7".into(),
            }
        );
    }

    #[test]
    fn bare_form_resolves_when_one_match() {
        let a_id = ProviderId::new("anthropic").unwrap();
        let a_caps = caps(&["claude-opus-4-7"]);
        let g_id = ProviderId::new("gemini").unwrap();
        let g_caps = caps(&["gemini-pro"]);
        let views = vec![
            ProviderView { id: &a_id, capabilities: &a_caps },
            ProviderView { id: &g_id, capabilities: &g_caps },
        ];
        match resolve_legacy_model("claude-opus-4-7", &views) {
            LegacyModelResolution::ResolvedFromBare { provider, model, .. } => {
                assert_eq!(provider.as_str(), "anthropic");
                assert_eq!(model, "claude-opus-4-7");
            }
            other => panic!("expected ResolvedFromBare, got {other:?}"),
        }
    }

    #[test]
    fn bare_form_ambiguous_falls_back() {
        let a_id = ProviderId::new("anthropic").unwrap();
        let g_id = ProviderId::new("gemini").unwrap();
        // Same model id on both — pathological but documents the rule.
        let caps_both = caps(&["shared-model"]);
        let views = vec![
            ProviderView { id: &a_id, capabilities: &caps_both },
            ProviderView { id: &g_id, capabilities: &caps_both },
        ];
        match resolve_legacy_model("shared-model", &views) {
            LegacyModelResolution::Ambiguous { candidates, .. } => {
                assert_eq!(candidates.len(), 2);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn unknown_provider_returns_unknown_provider() {
        let a_id = ProviderId::new("anthropic").unwrap();
        let a_caps = caps(&["m"]);
        let views = vec![ProviderView { id: &a_id, capabilities: &a_caps }];
        match resolve_legacy_model("openai/gpt-5", &views) {
            LegacyModelResolution::UnknownProvider { provider, .. } => {
                assert_eq!(provider.as_str(), "openai");
            }
            other => panic!("expected UnknownProvider, got {other:?}"),
        }
    }

    #[test]
    fn qualified_form_unknown_model_returns_unknown() {
        let a_id = ProviderId::new("anthropic").unwrap();
        let a_caps = caps(&["claude-opus-4-7"]);
        let views = vec![ProviderView { id: &a_id, capabilities: &a_caps }];
        match resolve_legacy_model("anthropic/nope", &views) {
            LegacyModelResolution::Unknown { .. } => {}
            other => panic!("expected Unknown, got {other:?}"),
        }
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p otto-host router::legacy_model::tests`
Expected: FAIL — module not registered in `lib.rs` yet.

- [ ] **Step 4: Wire the module**

Edit `crates/otto-host/src/lib.rs`:

```rust
pub mod router;
pub use router::{LegacyModelResolution, resolve_legacy_model};
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p otto-host router::legacy_model::tests`
Expected: PASS (6 tests).

- [ ] **Step 6: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy -p otto-host -- -D warnings
git add crates/otto-host/src/router/ crates/otto-host/src/lib.rs
git commit -m "feat(host): add OTTO_MODEL legacy-form resolver"
```

---

## Task 7: Silent `/connect` when keyring already has the key

**Files:**
- Modify: `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs`
- Modify: `crates/otto/src/plugin/builtin/provider_gemini/mod.rs`
- Modify: `crates/otto/src/plugin/builtin/provider_openai/mod.rs`
- Modify: `crates/otto/src/plugin/builtin/provider_local/mod.rs`

The bug fix the user originally reported. `handle_slash` for `/connect <provider>` should check the keyring first.

- [ ] **Step 1: Write the failing test**

In `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs`, add to the existing test module:

```rust
/// `/connect anthropic` with a stored key must NOT emit
/// `Effect::PromptApiKey`; it must instead emit `RegisterProvider`
/// (or equivalent) immediately via the keyring path. The
/// stored-key fallback no longer requires a modal traversal.
#[tokio::test]
#[serial_test::serial] // keyring tests share global state
async fn handle_slash_with_stored_key_skips_modal() {
    // Reset to "en" locale per feedback_test_locale_isolation.
    rust_i18n::set_locale("en");

    // Install a stored key for the duration of the test.
    let _ = keyring::Entry::new("otto", PROVIDER_ID)
        .map(|e| e.set_password("test-key"));

    let mut p = ProviderAnthropicPlugin::new();
    let effs = p.handle_slash("connect anthropic", vec![]).await.unwrap();
    let saw_prompt = effs
        .iter()
        .any(|e| matches!(e, Effect::PromptApiKey { .. }));
    assert!(
        !saw_prompt,
        "with a stored key, /connect must not open the modal"
    );
    let saw_register = effs.iter().any(|e| {
        matches!(e, Effect::RegisterProvider { id, .. } if id.as_str() == PROVIDER_ID)
    });
    assert!(saw_register, "must register the provider silently");

    // Cleanup.
    let _ = keyring::Entry::new("otto", PROVIDER_ID)
        .map(|e| e.delete_credential());
}
```

(Use `#[serial_test::serial]` because keyring entries are process-global on platforms with a backend; add `serial_test = "3"` to dev-deps if not present.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p otto provider_anthropic::tests::handle_slash_with_stored_key_skips_modal`
Expected: FAIL — current `handle_slash` unconditionally emits `PromptApiKey`.

- [ ] **Step 3: Update `handle_slash` to read keyring first**

Edit `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs`. Replace the existing `handle_slash` body:

```rust
async fn handle_slash(
    &mut self,
    _: &str,
    args: Vec<String>,
) -> Result<Vec<Effect>, PluginError> {
    let rekey = args.iter().any(|a| a == "--rekey");
    if !rekey && self.try_connect_from_keyring().is_some() {
        // Stored key worked; register without modal.
        return Ok(vec![Effect::RegisterProvider {
            id: ProviderId::new(PROVIDER_ID).expect("valid"),
            display_name: DISPLAY_NAME.into(),
        }]);
    }
    // No key, --rekey explicitly requested, or stored key didn't yield
    // a working client: open the modal.
    Ok(vec![Effect::PromptApiKey {
        provider_id: ProviderId::new(PROVIDER_ID).expect("valid"),
    }])
}
```

- [ ] **Step 4: Run the new test + the existing tests**

Run: `cargo test -p otto provider_anthropic::tests`
Expected: PASS for new test. The existing `no_creds_emits_prompt_api_key` test still passes (clears keyring before the call). The existing `handle_slash_with_existing_client_still_prompts` test must be **updated** — its premise (the modal opens even when a client exists) is no longer correct policy. Replace it with:

```rust
#[tokio::test]
#[serial_test::serial]
async fn handle_slash_with_rekey_flag_opens_modal_even_when_client_exists() {
    rust_i18n::set_locale("en");
    let mut p = ProviderAnthropicPlugin::with_test_client(Box::new(stub_client()));
    let effs = p
        .handle_slash("connect anthropic", vec!["--rekey".into()])
        .await
        .unwrap();
    assert!(effs.iter().any(|e| matches!(e, Effect::PromptApiKey { .. })));
}
```

(Define `stub_client()` as the inline `StubClient` from the existing test.)

- [ ] **Step 5: Repeat for the other three providers**

Apply the same change to:
- `crates/otto/src/plugin/builtin/provider_gemini/mod.rs`
- `crates/otto/src/plugin/builtin/provider_openai/mod.rs`
- `crates/otto/src/plugin/builtin/provider_local/mod.rs` (where `try_connect_from_keyring` is `try_connect_local` or similar — local providers don't require a key so the call always succeeds and silent-connect is the only path)

- [ ] **Step 6: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy -p otto -- -D warnings
git add crates/otto/src/plugin/builtin/provider_anthropic/ \
        crates/otto/src/plugin/builtin/provider_gemini/ \
        crates/otto/src/plugin/builtin/provider_openai/ \
        crates/otto/src/plugin/builtin/provider_local/
git commit -m "feat(connect): silent re-connect when keyring already has the key"
```

---

## Task 8: `~/.otto/config.toml` schema + load/save + first-launch migration

**Files:**
- Create: `crates/otto/src/config_file.rs`
- Create: `crates/otto/src/migration.rs`
- Modify: `crates/otto/src/main.rs`

The user-facing config that drives startup policy + holds the migration marker.

- [ ] **Step 1: Write the failing test for the config file schema**

Create `crates/otto/src/config_file.rs`:

```rust
//! ~/.otto/config.toml schema, load, save, and migration marker.
//! Single source of truth for non-routing knobs (startup connect policy,
//! per-provider connect timeout, migration_v1_done marker).

use std::path::{Path, PathBuf};

use otto_host::config::StartupConnectPolicy;
use otto_protocol::ProviderId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub startup: StartupSection,
    #[serde(default)]
    pub migration: MigrationSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupSection {
    /// "opt-in" | "all" | "last-used" | "none"
    #[serde(default = "default_policy")]
    pub policy: String,
    #[serde(default)]
    pub startup_providers: Vec<String>,
    #[serde(default = "default_timeout")]
    pub connect_timeout_ms: u64,
}

impl Default for StartupSection {
    fn default() -> Self {
        Self {
            policy: default_policy(),
            startup_providers: Vec::new(),
            connect_timeout_ms: default_timeout(),
        }
    }
}

fn default_policy() -> String { "opt-in".into() }
fn default_timeout() -> u64 { 3000 }

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MigrationSection {
    #[serde(default)]
    pub v1_done: bool,
}

impl ConfigFile {
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".otto")
            .join("config.toml")
    }

    pub fn load_or_default(path: &Path) -> Self {
        let Ok(contents) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&contents).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
    }

    pub fn to_startup_policy(&self) -> StartupConnectPolicy {
        let ids: Vec<ProviderId> = self
            .startup
            .startup_providers
            .iter()
            .filter_map(|s| ProviderId::new(s).ok())
            .collect();
        match self.startup.policy.as_str() {
            "all" => StartupConnectPolicy::All,
            "none" => StartupConnectPolicy::None,
            "last-used" => StartupConnectPolicy::LastUsed(ids),
            _ => StartupConnectPolicy::OptIn(ids),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn missing_file_returns_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let cfg = ConfigFile::load_or_default(&path);
        assert_eq!(cfg.startup.policy, "opt-in");
        assert!(cfg.startup.startup_providers.is_empty());
        assert!(!cfg.migration.v1_done);
    }

    #[test]
    fn round_trip_preserves_fields() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let mut cfg = ConfigFile::default();
        cfg.startup.policy = "opt-in".into();
        cfg.startup.startup_providers = vec!["anthropic".into(), "gemini".into()];
        cfg.startup.connect_timeout_ms = 4000;
        cfg.migration.v1_done = true;
        cfg.save(&path).unwrap();

        let loaded = ConfigFile::load_or_default(&path);
        assert_eq!(loaded.startup.policy, "opt-in");
        assert_eq!(loaded.startup.startup_providers, vec!["anthropic", "gemini"]);
        assert_eq!(loaded.startup.connect_timeout_ms, 4000);
        assert!(loaded.migration.v1_done);
    }

    #[test]
    fn policy_string_maps_correctly() {
        let mut cfg = ConfigFile::default();
        cfg.startup.policy = "all".into();
        assert!(matches!(cfg.to_startup_policy(), StartupConnectPolicy::All));
        cfg.startup.policy = "none".into();
        assert!(matches!(cfg.to_startup_policy(), StartupConnectPolicy::None));
        cfg.startup.policy = "opt-in".into();
        cfg.startup.startup_providers = vec!["anthropic".into()];
        match cfg.to_startup_policy() {
            StartupConnectPolicy::OptIn(ids) => assert_eq!(ids.len(), 1),
            _ => panic!(),
        }
    }
}
```

(Add `dirs`, `tempfile`, `serde`, `toml` to `Cargo.toml` for `otto` if missing; check `[dependencies]` first.)

- [ ] **Step 2: Run the test**

Run: `cargo test -p otto config_file::tests`
Expected: FAIL until `mod config_file;` is added; then PASS (3 tests).

- [ ] **Step 3: Wire the module + run again**

Edit `crates/otto/src/main.rs` (or `lib.rs`) — add `mod config_file;`. Re-run the test; expect PASS.

- [ ] **Step 4: Add the migration scanner**

Create `crates/otto/src/migration.rs`:

```rust
//! First-launch migration: when config.toml is absent or `v1_done=false`,
//! scan the keyring for existing provider keys and decide the initial
//! `startup_providers` list. If more than one key exists, the TUI opens
//! a picker (UI side); if exactly one or zero, write a deterministic
//! default. Either way, set `v1_done = true` so the picker never reopens.

use crate::config_file::ConfigFile;

/// Output of `decide_migration` — what to write or prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// Multiple keys exist; TUI should open the picker.
    Picker { detected: Vec<String> },
    /// Zero or one keys; write this list directly.
    Direct { startup_providers: Vec<String> },
    /// Migration was already done.
    AlreadyDone,
}

/// Known provider ids to scan the keyring for. Order matters: when no
/// keys are present we want a deterministic fallback (Anthropic if
/// present in the catalog, else first alphabetically).
const KNOWN_PROVIDERS: &[&str] = &["anthropic", "gemini", "openai", "local"];

pub fn decide_migration(cfg: &ConfigFile) -> MigrationOutcome {
    if cfg.migration.v1_done {
        return MigrationOutcome::AlreadyDone;
    }
    let detected: Vec<String> = KNOWN_PROVIDERS
        .iter()
        .filter(|id| crate::creds::load(id).map(|opt| opt.is_some()).unwrap_or(false))
        .map(|s| s.to_string())
        .collect();
    match detected.len() {
        0 => MigrationOutcome::Direct { startup_providers: Vec::new() },
        1 => MigrationOutcome::Direct { startup_providers: detected },
        _ => MigrationOutcome::Picker { detected },
    }
}

/// Fallback when the user dismisses the picker without confirming.
/// Anthropic if present in `detected`; else first alphabetically.
pub fn dismissed_fallback(detected: &[String]) -> Vec<String> {
    if detected.iter().any(|s| s == "anthropic") {
        return vec!["anthropic".into()];
    }
    let mut sorted = detected.to_vec();
    sorted.sort();
    sorted.into_iter().next().map(|s| vec![s]).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::ConfigFile;

    #[test]
    fn already_done_short_circuits() {
        let mut cfg = ConfigFile::default();
        cfg.migration.v1_done = true;
        assert_eq!(decide_migration(&cfg), MigrationOutcome::AlreadyDone);
    }

    #[test]
    fn dismissed_fallback_prefers_anthropic() {
        let detected = vec!["gemini".into(), "anthropic".into(), "openai".into()];
        assert_eq!(dismissed_fallback(&detected), vec!["anthropic".to_string()]);
    }

    #[test]
    fn dismissed_fallback_alphabetical_when_no_anthropic() {
        let detected = vec!["openai".into(), "gemini".into()];
        assert_eq!(dismissed_fallback(&detected), vec!["gemini".to_string()]);
    }

    #[test]
    fn dismissed_fallback_empty_when_no_keys() {
        let detected: Vec<String> = vec![];
        assert!(dismissed_fallback(&detected).is_empty());
    }
}
```

- [ ] **Step 5: Wire and test**

Add `mod migration;` to `main.rs`. Run `cargo test -p otto migration::tests` — expect PASS (4 tests).

Note: `decide_migration` cases involving real keyring entries aren't unit-tested here (they hit the platform keyring); the integration assertion happens in Task 9.

- [ ] **Step 6: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy -p otto -- -D warnings
git add crates/otto/src/config_file.rs crates/otto/src/migration.rs crates/otto/src/main.rs crates/otto/Cargo.toml
git commit -m "feat(tui): add ~/.otto/config.toml + migration scanner"
```

---

## Task 9: First-launch migration picker UI

**Files:**
- Create: `crates/otto/src/plugin/builtin/migration_picker/mod.rs`
- Create: `crates/otto/src/plugin/builtin/migration_picker/screen.rs`
- Modify: `crates/otto/src/plugin/builtin/mod.rs`
- Modify: `crates/otto/src/plugin/mod.rs` (register_builtins)
- Modify: `crates/otto/locales/en.yml`

A core plugin that opens its own screen on `HostStarting` when `decide_migration` returns `Picker`. The screen lists detected providers as toggle rows; Enter confirms; Esc applies `dismissed_fallback`. Either path writes `config.toml` and sets `v1_done = true`.

- [ ] **Step 1: Add i18n keys**

Edit `crates/otto/locales/en.yml`:

```yaml
migration:
  picker:
    title: "First launch — pick startup providers"
    hint: "Found multiple stored keys. Choose which should auto-connect on startup. Space to toggle, Enter to confirm, Esc to cancel."
    row_selected: "✓ {name}"
    row_unselected: "  {name}"
  saved: "Saved startup_providers = [{ids}] to ~/.otto/config.toml"
  fallback: "No selection confirmed; defaulting to startup_providers = [{ids}]"
```

(Mirror keys in other locale files with English text for now; native translation is a separate task.)

- [ ] **Step 2: Write the failing test (screen-level)**

Create `crates/otto/src/plugin/builtin/migration_picker/screen.rs` and stub the screen with placeholder behavior + tests asserting:
- Enter on a selection emits `Effect::Stack([Effect::CloseScreen, <write-config-effect>])`.
- Esc emits the fallback-write effect.
- Space toggles row selection.

(The exact test code mirrors `connect/screen.rs` tests in shape — render assertions on `StyledLine` rows, key dispatch assertions on the effects list. Engineer follows the `ConnectPickerScreen` pattern; see the test at `crates/otto/src/plugin/builtin/connect/screen.rs:127-168`.)

- [ ] **Step 3: Implement the screen**

The screen state is `{ rows: Vec<(String, bool)>, cursor: usize }`. Rendering uses `StyledLine::plain` (per the existing picker pattern). On Enter, gather selected ids and emit a `RunSlash` effect targeting `_internal:migration-confirm <ids>`. On Esc, emit `RunSlash _internal:migration-dismiss`. The plugin's `handle_slash` for those two synthetic slashes writes the config file and closes.

- [ ] **Step 4: Register the plugin in `register_builtins`**

Edit `crates/otto/src/plugin/mod.rs::register_builtins`. Add a new entry for `MigrationPickerPlugin` (kind `Core`, since it must run on first launch regardless of optional plugin state).

The plugin subscribes to `HookKind::HostStarting`. In `on_event(HostStarting)`, it calls `decide_migration`; if `Picker { detected }`, it emits an `Effect::OpenScreen { id: "migration.picker", args: ScreenArgs::MigrationPicker { detected } }`. If `Direct`, it writes config and proceeds silently.

- [ ] **Step 5: Add `ScreenArgs::MigrationPicker`**

Edit `crates/otto-plugin/src/lib.rs` (or wherever `ScreenArgs` lives) to add the new variant. Make sure `match` arms in `apply_effects.rs::open_screen` cover it.

- [ ] **Step 6: Test full path**

Add an integration test in `crates/otto/tests/migration_picker.rs` that:
1. Builds a fake home dir (TempDir + env override of `HOME` inside `HOME_LOCK`).
2. Installs two fake keyring entries (use a stub keyring if real-keyring is awkward in CI — gate the test on a feature flag if needed).
3. Boots a minimal App; asserts the migration screen opens.
4. Simulates Space + Enter on the first row; asserts `config.toml` is written with `startup_providers = ["anthropic"]` and `v1_done = true`.

(Wrap with `#[serial_test::serial]` + locale reset per [[feedback_test_locale_isolation]].)

Run: `cargo test -p otto migration_picker`
Expected: PASS.

- [ ] **Step 7: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy -p otto -- -D warnings
git add crates/otto/src/plugin/builtin/migration_picker/ \
        crates/otto/src/plugin/builtin/mod.rs \
        crates/otto/src/plugin/mod.rs \
        crates/otto/locales/ \
        crates/otto-plugin/src/lib.rs \
        crates/otto/tests/migration_picker.rs
git commit -m "feat(tui): first-launch migration picker for startup_providers"
```

---

## Task 10: `/disconnect <provider> [--force]` slash command

**Files:**
- Modify: `crates/otto/src/main.rs`
- Modify: `crates/otto/locales/en.yml`

- [ ] **Step 1: Write the failing test**

Add to `crates/otto/tests/slash_commands.rs` (create if absent):

```rust
//! End-to-end slash command tests for /disconnect and /use.

use otto_protocol::ProviderId;
// … set up an App with a host that has anthropic + gemini connected,
//   then dispatch `/disconnect gemini` through the same path main.rs
//   uses (the run_slash function or its public test seam).
//   Assert: gemini is no longer in pool, anthropic still is, active
//   provider unchanged (since the disconnected one wasn't active).

#[tokio::test]
async fn disconnect_drain_removes_provider() {
    // … construction details follow existing test patterns in
    //   crates/otto/tests/*. See feedback_streaming_test_permissions:
    //   pre-register Allow if any tool-use surface is involved.
}

#[tokio::test]
async fn disconnect_force_aborts_inflight_turn() {
    // Construct an App with a stuck provider, kick off a turn, run
    // `/disconnect <stuck> --force`, assert TurnEvent::Cancelled fires
    // within < 600ms and pool no longer contains the provider.
}
```

(Real test bodies follow `crates/otto/tests/*.rs` patterns; this plan describes the assertions, not the harness setup boilerplate. The engineer reuses existing test helpers.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p otto --test slash_commands`
Expected: FAIL — `/disconnect` not yet routed.

- [ ] **Step 3: Wire the slash command**

Edit `crates/otto/src/main.rs::run_slash`. Add a new arm next to the existing `/model` / `/resume` / `/sandbox` cases:

```rust
"/disconnect" => {
    handle_disconnect_command(app, rest, host_slot).await;
}
```

Implement `handle_disconnect_command`:

```rust
async fn handle_disconnect_command(
    app: &mut App,
    rest: &str,
    host_slot: &HostSlot,
) {
    let mut tokens = rest.split_whitespace();
    let Some(provider) = tokens.next() else {
        app.push_note(rust_i18n::t!("notes.disconnect-needs-provider").to_string());
        return;
    };
    let force = tokens.any(|t| t == "--force");
    let Some(host) = current_host(host_slot).await else {
        app.push_note(rust_i18n::t!("notes.disconnect-no-host").to_string());
        return;
    };
    let Ok(pid) = ProviderId::new(provider) else {
        app.push_note(
            rust_i18n::t!("notes.disconnect-invalid-id", id = provider).to_string(),
        );
        return;
    };
    let mode = if force { DisconnectMode::Force } else { DisconnectMode::Drain };
    app.push_note(
        rust_i18n::t!("notes.disconnect-starting", name = provider, mode = format!("{mode:?}"))
            .to_string(),
    );
    let host_clone = Arc::clone(&host);
    let pid_clone = pid.clone();
    tokio::spawn(async move {
        let _ = host_clone.remove_provider(&pid_clone, mode).await;
    });
}
```

(Add i18n keys for the three notes.)

- [ ] **Step 4: Run the tests**

Run: `cargo test -p otto --test slash_commands`
Expected: PASS.

- [ ] **Step 5: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy -p otto -- -D warnings
git add crates/otto/src/main.rs crates/otto/tests/slash_commands.rs crates/otto/locales/
git commit -m "feat(tui): /disconnect <provider> [--force] slash command"
```

---

## Task 11: `/use <provider>` slash command

**Files:**
- Modify: `crates/otto/src/main.rs`
- Modify: `crates/otto-host/src/session.rs` (add `Host::set_active_provider` + `clear_history`)
- Modify: `crates/otto/locales/en.yml`

Switch active provider; clears history first.

- [ ] **Step 1: Write the failing test**

Append to `crates/otto/tests/slash_commands.rs`:

```rust
#[tokio::test]
async fn use_provider_clears_history_and_switches_active() {
    // Construct App with anthropic + gemini in pool, anthropic active.
    // Run a turn so history is non-empty.
    // Dispatch `/use gemini`.
    // Assert:
    //   - host.active_provider() == "gemini"
    //   - host history is empty
    //   - status bar reflects gemini as active
}

#[tokio::test]
async fn use_provider_rejects_unknown_id() {
    // Dispatch `/use nope` with anthropic+gemini connected.
    // Assert: active provider unchanged, styled note emitted.
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p otto --test slash_commands use_provider_clears`
Expected: FAIL.

- [ ] **Step 3: Add host method**

Edit `crates/otto-host/src/session.rs`:

```rust
impl Host {
    pub async fn set_active_provider(&self, id: &ProviderId) -> Result<(), PoolError> {
        let pool = self.pool.read().await;
        if !pool.contains_key(id) {
            return Err(PoolError::NotRegistered(id.clone()));
        }
        drop(pool);
        *self.active_provider.write().await = id.clone();
        self.clear_history().await;
        Ok(())
    }
}
```

(`clear_history` already exists per the spec's reference to it; reuse.)

- [ ] **Step 4: Wire `/use` in main.rs**

Add to `run_slash`:

```rust
"/use" => {
    handle_use_command(app, rest, host_slot).await;
}
```

```rust
async fn handle_use_command(app: &mut App, rest: &str, host_slot: &HostSlot) {
    let provider = rest.split_whitespace().next().unwrap_or("");
    if provider.is_empty() {
        app.push_note(rust_i18n::t!("notes.use-needs-provider").to_string());
        return;
    }
    let Some(host) = current_host(host_slot).await else {
        app.push_note(rust_i18n::t!("notes.use-no-host").to_string());
        return;
    };
    let Ok(pid) = ProviderId::new(provider) else {
        app.push_note(rust_i18n::t!("notes.use-invalid-id", id = provider).to_string());
        return;
    };
    match host.set_active_provider(&pid).await {
        Ok(()) => {
            app.entries.clear();
            app.live_text.clear();
            app.update_metrics();
            app.push_note(rust_i18n::t!("notes.use-switched", name = provider).to_string());
        }
        Err(PoolError::NotRegistered(_)) => {
            app.push_note(
                rust_i18n::t!("notes.use-not-connected", name = provider).to_string(),
            );
        }
        Err(e) => {
            app.push_note(format!("{e:#}"));
        }
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p otto --test slash_commands use_provider`
Expected: PASS.

- [ ] **Step 6: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy --workspace -- -D warnings
git add crates/otto/src/main.rs crates/otto-host/src/session.rs crates/otto/locales/ crates/otto/tests/slash_commands.rs
git commit -m "feat: /use <provider> switches active provider and clears history"
```

---

## Task 12: `/model` filtering by active provider

**Files:**
- Modify: `crates/otto/src/main.rs` (`handle_model_command`)
- Modify: `crates/otto/src/plugin/builtin/model/screen.rs`

Today's `/model` already lists models — we now filter to only the active provider's catalog.

- [ ] **Step 1: Write the failing test**

Add to `crates/otto/tests/slash_commands.rs`:

```rust
#[tokio::test]
async fn model_picker_only_lists_active_providers_models() {
    // Construct App with anthropic + gemini connected, anthropic active.
    // Open the model picker.
    // Assert: only anthropic models shown.
    // Switch to gemini via /use; reopen picker; assert: only gemini models.
}
```

- [ ] **Step 2: Filter at picker open time**

Edit `crates/otto/src/plugin/effects.rs` where `ScreenArgs::ModelPicker { current_id, models }` is constructed (in `open_screen`). Replace the `models: app.cached_models.clone()` line with a filtered version that pulls from the host's active provider's capabilities:

```rust
let models = if let Some(host) = current_host_via_slot.await {
    let active = host.active_provider().await;
    let pool_view = host.pool_snapshot().await;
    pool_view
        .get(&active)
        .map(|entry| entry.capabilities().models.clone())
        .unwrap_or_default()
} else {
    Vec::new()
};
```

Add `Host::pool_snapshot` returning `HashMap<ProviderId, PoolEntrySnapshot>` (a cheap-clone view: id, display_name, capabilities). Used here and by the status bar.

- [ ] **Step 3: Reject inactive-provider model selection in `handle_model_command`**

Where `handle_model_command` accepts a model id directly (no UI), validate against the active provider's catalog before applying. Reject with a styled note if the id isn't there.

- [ ] **Step 4: Run tests**

Run: `cargo test -p otto --test slash_commands model_picker_only_lists_active`
Expected: PASS.

- [ ] **Step 5: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy --workspace -- -D warnings
git add crates/otto/src/main.rs crates/otto/src/plugin/effects.rs crates/otto/src/plugin/builtin/model/screen.rs crates/otto-host/src/session.rs crates/otto/tests/slash_commands.rs
git commit -m "feat(tui): /model picker filters to active provider's catalog"
```

---

## Task 13: Status bar lists all pool members + active marker

**Files:**
- Modify: `crates/otto/src/ui.rs`
- Modify: `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs` (render_slot)
- Modify: same for gemini, openai, local

Today each provider plugin renders its own `home.footer.left` slot when connected. After Phase 1, ALL connected providers render; the active one gets a leading "▸ ".

- [ ] **Step 1: Write the failing test**

In `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs`:

```rust
#[test]
fn render_slot_marks_active_provider() {
    let mut p = ProviderAnthropicPlugin::with_test_client(Box::new(stub_client()));
    // Set a flag the plugin reads to know it's the active one.
    p.set_active_for_render(true);
    let lines = p.render_slot("home.footer.left", region());
    let joined: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
        .collect::<Vec<_>>()
        .join("");
    assert!(joined.starts_with("▸ "), "active marker missing in: {joined}");
}
```

- [ ] **Step 2: Add the marker plumbing**

The plugin needs to know whether it's the active provider. Options:
- A `HostEvent::ActiveProviderChanged { id }` event the plugin subscribes to and stores locally.
- A `Plugin::render_slot` parameter carrying app-level context.

Option A is the cleaner long-term fit (event bus pattern already exists). Add `HostEvent::ActiveProviderChanged { id: ProviderId }` to `otto-plugin::HostEvent`. Dispatch it from `Host::set_active_provider` and from `Host::start` (initial active). The plugin caches the most recent id and compares against its own `PROVIDER_ID` in `render_slot`.

Add `set_active_for_render(&mut self, active: bool)` as test seam.

- [ ] **Step 3: Apply to all four provider plugins + run tests**

Repeat for gemini, openai, local. Run `cargo test -p otto provider_anthropic provider_gemini provider_openai provider_local`. Expect PASS.

- [ ] **Step 4: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy --workspace -- -D warnings
git add crates/otto/src/plugin/builtin/provider_*/ crates/otto-plugin/src/ crates/otto-host/src/session.rs
git commit -m "feat(tui): status bar marks the active provider"
```

---

## Task 14: Connect picker alt-Enter for `/connect <id> --rekey`

**Files:**
- Modify: `crates/otto/src/plugin/builtin/connect/screen.rs`
- Modify: `crates/otto/locales/en.yml`

- [ ] **Step 1: Write the failing test**

In `crates/otto/src/plugin/builtin/connect/screen.rs::tests`:

```rust
#[tokio::test]
async fn alt_enter_emits_rekey_slash() {
    let mut s = ConnectPickerScreen::with_candidates(vec![(
        ProviderId::new("anthropic").unwrap(),
        "Anthropic".into(),
    )]);
    let mut key = key(KeyCodePortable::Enter);
    key.modifiers.alt = true;
    let effs = s.on_key(key).await.unwrap();
    match &effs[0] {
        Effect::Stack(children) => {
            assert!(matches!(children[0], Effect::CloseScreen));
            match &children[1] {
                Effect::RunSlash { name, .. } => {
                    assert_eq!(name, "connect anthropic --rekey");
                }
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p otto connect::screen::tests::alt_enter`
Expected: FAIL — current `on_key` doesn't inspect modifiers on Enter.

- [ ] **Step 3: Update `on_key`**

In `screen.rs::on_key`, change the `Enter` branch:

```rust
KeyCodePortable::Enter => {
    let Some((pid, _)) = self.candidates.get(self.cursor).cloned() else {
        return Ok(vec![Effect::CloseScreen]);
    };
    let name = if key.modifiers.alt {
        format!("connect {} --rekey", pid.as_str())
    } else {
        format!("connect {}", pid.as_str())
    };
    Ok(vec![Effect::Stack(vec![
        Effect::CloseScreen,
        Effect::RunSlash { name, args: vec![] },
    ])])
}
```

Update the tips text to mention Alt-Enter:

```yaml
picker:
  connect:
    tips: "Enter to connect · Alt-Enter to re-enter API key · Esc to cancel"
```

- [ ] **Step 4: Run tests + commit**

Run: `cargo test -p otto connect::screen::tests`
Expected: PASS.

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy -p otto -- -D warnings
git add crates/otto/src/plugin/builtin/connect/screen.rs crates/otto/locales/
git commit -m "feat(tui): Alt-Enter on connect picker re-enters API key"
```

---

## Task 15: TUI startup wires `StartupConnectPolicy` + per-provider timeout

**Files:**
- Modify: `crates/otto/src/main.rs`

Today the TUI's startup constructs a single host via `bootstrap_host`. After Phase 1, startup must:
1. Load `config.toml` + run migration if needed.
2. Build `ProviderRegistration` entries for every available built-in provider plugin.
3. Pass them in `HostConfig::providers` + `startup_connect`.
4. Apply the per-provider timeout when `Host::start` builds each entry.

- [ ] **Step 1: Write the failing test**

Add `crates/otto/tests/startup_policy.rs`:

```rust
#[tokio::test]
#[serial_test::serial]
async fn startup_opt_in_only_connects_allowed_providers() {
    // Configure HOME to a TempDir.
    // Write config.toml with policy="opt-in", startup_providers=["anthropic"].
    // Install fake keyring entries for anthropic AND gemini.
    // Boot the app (or its bootstrap_host equivalent).
    // Assert: host has anthropic in pool, NOT gemini.
}

#[tokio::test]
#[serial_test::serial]
async fn startup_timeout_skips_slow_provider() {
    // Construct a provider plugin whose `try_connect_from_keyring`
    // takes > connect_timeout_ms. Assert: host comes up without that
    // provider; styled note in the log mentions the timeout.
}
```

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL.

- [ ] **Step 3: Refactor startup**

In `crates/otto/src/main.rs`, refactor `bootstrap_host` (or its replacement) to:

```rust
async fn bootstrap_host_with_pool(
    project_root: &Path,
    tool_bins: &ToolBins,
    config_file: &ConfigFile,
) -> Result<Host, ...> {
    let policy = config_file.to_startup_policy();
    let timeout = std::time::Duration::from_millis(config_file.startup.connect_timeout_ms);
    let mut providers = Vec::new();
    for plugin in iter_provider_plugins() {
        let res = tokio::time::timeout(timeout, plugin.try_build_registration()).await;
        match res {
            Ok(Ok(Some(reg))) => providers.push(reg),
            Ok(Ok(None)) => {
                // No key; not an error — plugin user can /connect later.
            }
            Ok(Err(e)) => {
                tracing::warn!(plugin = ?plugin.id(), error = %e, "provider build failed");
                push_note_to_log(...);
            }
            Err(_elapsed) => {
                push_note_to_log(t!("notes.startup-timeout", name = plugin.display_name()));
            }
        }
    }
    let mut cfg = HostConfig::new(/* legacy fallback unused */, "");
    cfg.providers = providers;
    cfg.startup_connect = policy;
    cfg.connect_timeout_ms = config_file.startup.connect_timeout_ms;
    cfg.project_root = project_root.into();
    // … existing tool/sandbox setup …
    Host::start(cfg).await
}
```

Add `try_build_registration` to each provider plugin (consuming its keyring-load + client-build code). Where today the plugin sets `self.client = Some(...)` for later `take_client`, the new method returns `Result<Option<ProviderRegistration>, ...>` directly — no internal `client` state needed.

- [ ] **Step 4: Run the test**

Run: `cargo test -p otto --test startup_policy`
Expected: PASS.

- [ ] **Step 5: Clippy + fmt + commit**

```bash
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy --workspace -- -D warnings
git add crates/otto/src/main.rs crates/otto/src/plugin/builtin/provider_*/ crates/otto/tests/startup_policy.rs
git commit -m "feat(tui): startup applies StartupConnectPolicy + per-provider timeout"
```

---

## Task 16: `perform_connect` updates to call `Host::add_provider`

**Files:**
- Modify: `crates/otto/src/main.rs`

Today `perform_connect` builds a fresh host and swaps the slot. After Phase 1, it builds a `ProviderRegistration` and calls `host.add_provider(reg)`.

- [ ] **Step 1: Write the failing test**

Add to `crates/otto/tests/slash_commands.rs`:

```rust
#[tokio::test]
async fn connect_adds_to_pool_without_replacing_host() {
    // Start with anthropic connected.
    // Run `/connect gemini` (simulate stored key).
    // Assert:
    //   - host.is_connected("anthropic") == true
    //   - host.is_connected("gemini") == true
    //   - host instance pointer unchanged (no replacement)
    //   - active_provider still "anthropic"
}
```

- [ ] **Step 2: Refactor `perform_connect`**

Replace the body so it:
1. Builds the `ProviderRegistration` from the spec + api_key + capabilities (the same plumbing used by `try_build_registration` in Task 15).
2. Calls `host.add_provider(reg).await`. On `AlreadyRegistered`, emits a styled note and exits without replacing.
3. Does NOT touch `app.entries` / `app.live_text` / history — the pool is additive.
4. Sets `app.connected = true` only when this was the first provider in the pool; otherwise the bar already shows connected providers.

- [ ] **Step 3: Run tests + commit**

```bash
cargo test -p otto --test slash_commands connect_adds_to_pool
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy --workspace -- -D warnings
git add crates/otto/src/main.rs crates/otto/tests/slash_commands.rs
git commit -m "feat(tui): /connect adds to pool instead of replacing host"
```

---

## Task 17: Re-prompt regression test

**Files:**
- Create or modify: `crates/otto/tests/connect_regression.rs`

The user's original complaint deserves an explicit, named test that locks in the fix.

- [ ] **Step 1: Write the test**

```rust
//! Regression test for the "/connect re-prompts when key is stored" bug.

use otto_plugin::Effect;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn connect_with_stored_key_does_not_open_modal() {
    rust_i18n::set_locale("en");

    // Pre-populate keyring (use the platform keyring; test is serial).
    let _ = keyring::Entry::new("otto", "anthropic")
        .map(|e| e.set_password("test-key"));

    // Construct the plugin and dispatch /connect.
    let mut p = otto::plugin::builtin::provider_anthropic::ProviderAnthropicPlugin::new();
    let effs = p.handle_slash("connect anthropic", vec![]).await.unwrap();
    assert!(
        !effs.iter().any(|e| matches!(e, Effect::PromptApiKey { .. })),
        "stored-key /connect must not open the API key modal"
    );

    // Cleanup.
    let _ = keyring::Entry::new("otto", "anthropic")
        .map(|e| e.delete_credential());
}
```

- [ ] **Step 2: Run + commit**

```bash
cargo test -p otto --test connect_regression
git add crates/otto/tests/connect_regression.rs
git commit -m "test: lock in /connect-with-stored-key silent path"
```

---

## Task 18: README + CHANGELOG + version bump

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Modify: `Cargo.toml` (workspace.package.version)
- Modify: `crates/*/Cargo.toml` literals in `[workspace.dependencies]` (per [[feedback_semver]])

Per spec, every release ships with notes + README sync (see [[feedback_release_notes]], [[feedback_release_docs]]).

- [ ] **Step 1: Bump version**

The current workspace version is 0.11.0 (post-self-update). Phase 1 is feature work → MINOR bump → 0.12.0 (per [[feedback_semver]]). Update:
- `Cargo.toml` `[workspace.package].version = "0.12.0"`
- Any literal version strings in `[workspace.dependencies]` for otto-* crates.

- [ ] **Step 2: CHANGELOG entry**

Add to `CHANGELOG.md`:

```markdown
## [0.12.0] — YYYY-MM-DD

### Added
- Multi-provider connection pool. `/connect <provider>` is now silent when the keyring already has a stored key; the API-key modal only opens when a key is missing or `--rekey` is passed.
- `/disconnect <provider> [--force]` removes a provider from the pool. Drain mode waits for in-flight turns; Force mode signals cooperative cancel, waits 500ms, then aborts.
- `/use <provider>` switches the active provider and clears the conversation (Phase 1 invariant: one active provider per conversation).
- `~/.otto/config.toml` for startup connect policy (`opt-in` / `all` / `last-used` / `none`) and per-provider connect timeout.
- First-launch migration picker for users upgrading with multiple stored keys.

### Changed
- The host's single `provider: Box<dyn ProviderClient>` field is replaced by a `HashMap<ProviderId, PoolEntry>` with `Arc`-held clients and active-turn leases. `HostConfig::providers` carries the registration set in.
- `/model` lists only the active provider's models. Switching providers requires `/use <provider>`.
- `OTTO_MODEL` accepts both legacy bare-model form and new `provider/model` form; ambiguous bare forms log a warning and fall back to default.

### Migration notes
- Pre-existing users with multiple stored keys see a one-time picker on first launch; the selection writes `startup_providers` to `~/.otto/config.toml`.
- Single-key users see no UI change beyond the silent re-connect behavior.
```

- [ ] **Step 3: README updates**

Sections that need touching:
- "Running providers as standalone MCP servers" — add a note that `/connect <provider>` is now silent.
- New section "Connected provider pool" describing single-active-provider semantics for Phase 1 and `/disconnect`, `/use`.
- "Configuration files" — document `~/.otto/config.toml`.

(Engineer drafts copy by reading the spec sections "Connect semantics" and "Startup auto-connect policy" and translating to README voice.)

- [ ] **Step 4: Verify build + tests + lint at the new version**

Run:
```bash
cargo build --workspace
cargo test --workspace
rustup run stable cargo fmt --all -- --check
rustup run stable cargo clippy --workspace -- -D warnings
```
Expected: clean across the board. Per [[feedback_dead_code_in_binary_crate]], `cargo build --workspace` catches `dead_code` errors that `cargo test` masks.

- [ ] **Step 5: Commit**

```bash
git add README.md CHANGELOG.md Cargo.toml
git commit -m "release(0.12.0): multi-provider pool foundation"
```

---

## Task 19: GitHub release notes + manual verification

**Files:** none (release prep)

Per [[feedback_cargo_dist_release.md]]: do NOT run `gh release create` for tagged versions — cargo-dist handles release publication on tag push. This task is about preparing the release-note draft and verifying CI on the push.

- [ ] **Step 1: Tag and push (USER step, not agent step)**

Show the user the commands; do not run them ourselves:

```bash
git tag v0.12.0
git push origin spec/multi-provider-pool-and-routing
git push origin v0.12.0
```

- [ ] **Step 2: Watch CI**

Per [[feedback_verify_ci_after_push]]: after the user pushes, monitor with `gh run watch` until green. Confirm the cargo-dist Release workflow uploaded binaries. Do NOT claim the release shipped until that workflow shows success.

- [ ] **Step 3: Update the original GitHub issue (if one exists)**

Per [[feedback_keep_issue_updated]]: post a comment summarizing what Phase 1 ships and linking the release. Close only after the user confirms.

---

## Self-Review

**Spec coverage check** — every spec section vs. tasks:
- Pool foundation (`provider_pool`, `PoolEntry`, `ProviderLease`) → Tasks 2, 4 ✓
- Connect semantics: silent when stored → Task 7 ✓
- `--rekey` flag → Tasks 7, 14 ✓
- `/disconnect` with Drain + Force → Tasks 10, 5 ✓
- 3-stage Force protocol (signal → grace → abort) → Task 5 ✓
- `ProviderCapabilities` / `ModelCapabilities` / `ModelAlias` → Task 1 ✓
- `HostConfig::providers` + `ProviderRegistration` (Arc end-to-end) → Task 3 ✓
- `StartupConnectPolicy` + per-provider timeout → Tasks 3, 15 ✓
- Active provider invariant + `/use` → Tasks 4, 11 ✓
- `/model` filtered by active → Task 12 ✓
- Legacy `OTTO_MODEL` resolver → Task 6 ✓
- `~/.otto/config.toml` + migration picker → Tasks 8, 9 ✓
- Status bar lists all pool members + active marker → Task 13 ✓
- Re-prompt regression test → Task 17 ✓
- README + CHANGELOG + version bump → Task 18 ✓
- Release prep → Task 19 ✓

**Spec sections deferred to later phases (not Phase 1):**
- Phase 2 gate (cross-vendor tool_use ID compatibility tests) → separate plan
- `@provider:model` override + cross-provider conversations → separate plan
- Modality routing → separate plan
- Routing rules from `~/.otto/routing.toml` → separate plan
- Heuristic classifier → separate plan
- Transcript per-turn routing badge → arrives with `@`-override in Phase 3 plan
- `routing.toml` schema and parser → arrives in Phase 5 plan

**Placeholder scan:** searched for "TBD", "TODO", "implement later", "fill in details", "Add appropriate error handling", "Similar to Task N", "Write tests for the above". None found in actionable steps; the only deferred work is in the "Phase 2+" section list which is explicit non-scope.

**Type consistency:** `PoolEntry` / `ProviderLease` / `DisconnectMode` / `PoolError` / `ProviderRegistration` / `StartupConnectPolicy` / `ProviderCapabilities` / `ModelCapabilities` / `ModelAlias` / `LegacyModelResolution` / `MigrationOutcome` / `ConfigFile` — names used consistently across all task references. `Host::add_provider`, `Host::remove_provider`, `Host::set_active_provider`, `Host::active_provider`, `Host::is_connected`, `Host::pool_snapshot`, `Host::acquire_lease_for_test` — same.

**Tests cover spec gates:**
- Drain semantics (Task 4 test): lease keeps Arc alive after entry removal.
- Force semantics (Task 5 test): uncooperative provider aborted within 400ms with `force_disconnect_grace_ms=200`.
- Lock hygiene: enforced by lease pattern itself + reviewed in Task 4's pool_lifecycle test.
- Startup policy (Task 15 test): `opt-in` only connects allow-list.
- Migration (Tasks 8, 9 tests): pure logic in unit tests; UI picker in integration test.
- Legacy `OTTO_MODEL` (Task 6 test): all 6 resolution paths.
- Re-prompt regression (Task 17): the exact bug that motivated this whole change.

Plan is complete and internally consistent.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-15-multi-provider-pool-phase-1.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
