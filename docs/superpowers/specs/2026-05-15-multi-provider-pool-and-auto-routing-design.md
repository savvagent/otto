# Multi-provider pool and auto-routing — design

Date: 2026-05-15
Status: pending review
Roadmap: post-v0.11.0; introduces a new major capability — version TBD when slicing into PRs
Supersedes: nothing
Related: project_connect_command (in-process providers, `/connect`), project_host_design (single-provider Host)

## Problem

Two complaints, one root cause.

1. **`/connect <provider>` always re-prompts for the API key**, even after the user has already entered it. The keyring entry is intact and startup auto-connects fine, but `handle_slash` in every provider plugin
   (`crates/otto/src/plugin/builtin/provider_anthropic/mod.rs:134-145`, mirrored in `provider_gemini` and `provider_openai`) unconditionally emits `Effect::PromptApiKey`. The user is expected to press Enter on the empty modal to "use stored key" (`crates/otto/src/main.rs:2059-2083`), which is undiscoverable and feels like the key was forgotten.

2. **Only one provider can be connected at a time.** `host_slot: Arc<RwLock<Option<Arc<Host>>>>` (`crates/otto/src/main.rs:75`) holds a single Host; each Host owns a single `Box<dyn ProviderClient>` (`crates/otto-host/src/session.rs:208-243`). `perform_connect` swaps the host out, shuts down the old one, and clears history (`main.rs:1341-1357`). There is no way to keep Anthropic and Gemini connected at the same time, let alone route different turns to different models within one conversation.

Both complaints dissolve if connected providers persist as a **pool** and the host gains a small **router** that picks which provider serves each user turn. After that change, `/connect` becomes additive ("add this provider to the pool") and re-prompting is no longer needed — silent connect from the keyring is the default.

## Goals

- Multiple providers connected concurrently (Anthropic + Gemini + OpenAI all live at once).
- `/connect <provider>` is silent when a key is stored; only prompts when missing. Explicit `--rekey` for the re-enter case.
- Per-user-turn routing: image attached → vision-capable model; user-defined rules from a config file; `@provider:model` prefix as an override that always wins.
- Shared, host-owned conversation history that survives provider switches within one conversation.
- Routing decisions visible in the transcript (small per-turn badge) so the user can always answer "why did it pick that model?"
- Tools (`ToolRegistry`) stay shared and provider-agnostic. No fork.

## Non-goals

- An intent classifier that learns from usage. The v1 heuristic classifier is static keyword/length rules; an ML-based router is not in scope.
- Cross-provider streaming of a single turn (e.g., model A drafts, model B refines). Routing boundary is the user turn; once a provider takes a turn, it runs it end-to-end including all tool-use iterations.
- A `/route` editor inside the TUI. Rules are edited in a file; the TUI offers `/route reload` and `/route show` only.
- Persisted pool state as a separate "connected providers" file. The pool is rebuilt at startup according to the explicit policy described in "Startup auto-connect policy" — by default, only the providers the user opts into are auto-connected, not every keyring entry. State of "what was connected last session" lives in `~/.otto/state.toml` only when the user picks the `last-used` policy.
- Changes to the SPP wire format. The `Message`/`ContentBlock` shapes in `crates/otto-protocol` are unchanged; routing operates above SPP.

## Approach

The Host gains a `provider_pool: HashMap<ProviderId, PoolEntry>` in place of its single `provider` field, plus a `Router` that, given a `CompleteRequest` and pool state, returns `(ProviderId, ModelId)`. The TUI continues to see one `Arc<Host>` — no change to the `Arc<RwLock<Option<Arc<Host>>>>` pattern that protects against awaits-while-locked (see project_tui_design). What changes is that the host is no longer destroyed when the user "connects to a different provider"; instead, `Host::add_provider` and `Host::remove_provider` mutate the pool in place under the same lifecycle rules used by `ToolRegistry`.

`PoolEntry` holds the provider client behind an `Arc<dyn ProviderClient + Send + Sync>` (not `Box`) plus its `ProviderCapabilities` and active-turn counter. The `Arc` is what makes turn leasing safe: see "Pool lifecycle and turn leases" below.

Capabilities cross the crate boundary via `HostConfig`, not by reaching back into plugin manifests. The TUI is responsible for collecting each plugin's `ProviderCapabilities` (already declared in its manifest) and packing it into a `ProviderRegistration` before calling `Host::add_provider`. The host depends only on `otto-protocol` and `otto-mcp`; it does not know about `Plugin` or `Manifest`. See "Crate boundary and capability flow" below.

Routing layers run in priority order, first match wins:

1. **`@provider:model` prefix** in the user input (stripped before submission). Always wins.
2. **Modality match.** If the input contains an `Image` content block and the current default provider/model has `supports_vision = false`, route to the highest-priority connected model that does. Same logic applies to PDF and audio if/when those modalities land.
3. **User rules** from `~/.otto/routing.toml`. Top-to-bottom; first match wins.
4. **Heuristic classifier** (opt-in). Short factoid → cheap fast model; keyword "refactor"/"implement"/"debug" → coding-strong model; else default. Off by default in v1; user enables via routing.toml.
5. **Default model.** The current `/model` selection, generalized to identify both a provider and a model. Mirrors today's `OTTO_MODEL` precedence (env → `~/.otto/models.toml` → provider's `default_model`).

The router emits a `RoutingDecision { provider_id, model_id, reason }` that the host pins for the duration of the user turn. The reason is one of the layer names (Override / Modality / Rule(name) / Heuristic / Default) and gets surfaced in the transcript.

Conversation history is owned by the host as a single canonical `Vec<Message>` (already SPP/vendor-neutral). Each `ProviderClient::complete` call adapts SPP → vendor format on its own — that already works today, since the in-process clients wrap vendor SDKs. The only new wrinkle is that `tool_use` IDs minted by one provider must round-trip through another provider's history serializer without collision. The host namespaces tool_use IDs with the issuing provider's id at insertion time (`<provider-id>:<original-id>`); each provider adapter strips its own prefix on the way out and chooses one of two strategies for foreign IDs: pass-through (treat as opaque string) or hash-substitution (rewrite to a short opaque token before sending, restore on return). Pass-through is preferred and simpler, but its viability per vendor is the subject of **"Phase 2 gate: cross-vendor tool_use ID compatibility"** below — that gate must pass with concrete tests before any phase that lets the user switch providers mid-conversation can ship.

Connect semantics flip from "replace the active host" to "add to the pool." `/connect <provider>` becomes:

- Provider already in the pool → push a "already connected" note and exit.
- Keyring has a key → silent connect (build `ProviderClient`, insert into pool, emit `ProviderRegistered` + `Connect` events, status bar reflects).
- Keyring is empty → open the existing `Effect::PromptApiKey` modal. On submit, save to keyring and connect.
- `--rekey` flag → always open the modal, regardless of stored key. (Same modal flow as today, just gated on the flag instead of unconditional.)

The picker (`ConnectPickerScreen`) gains a "Re-enter key" sub-option per provider for the same `--rekey` path, so the keyboard-only path doesn't require typing a flag.

## Modules

```
crates/otto-host/src/
├── session.rs            // Host: provider field → provider_pool: HashMap<ProviderId, PoolEntry>
│                         //       + per-entry turn-lease counter + disconnect modes
├── pool.rs               // PoolEntry, disconnect modes (Drain/Force), lease guards
├── config.rs             // HostConfig adds providers: Vec<ProviderRegistration>;
│                         //   startup_connect: StartupConnectPolicy
├── capabilities.rs       // ProviderCapabilities + ModelCapabilities + ModelAlias;
│                         //   crate-internal types passed in via HostConfig
├── router/
│   ├── mod.rs            // Router struct, RoutingDecision, layered dispatch
│   ├── modality.rs       // detect_required_modality(req) -> RequiredModalities
│   ├── rules.rs          // RoutingRules parsing/eval (TOML)
│   ├── heuristics.rs     // opt-in keyword/length classifier
│   ├── prefix.rs         // parse_at_prefix("@anthropic:opus-4.7 …") -> (override, rest);
│   │                     //   handles @@-escape, unknown-token fallthrough
│   └── legacy_model.rs   // OTTO_MODEL parser + ambiguity resolver
└── lib.rs                // re-export Router, RoutingDecision, ProviderRegistration,
                          //   StartupConnectPolicy, DisconnectMode

crates/otto/src/
├── main.rs               // host_slot stays Arc<RwLock<Option<Arc<Host>>>>; perform_connect → add_provider
├── plugin/builtin/
│   ├── provider_anthropic/mod.rs   // handle_slash: silent if key stored; --rekey flag opens modal
│   ├── provider_gemini/mod.rs      // same pattern
│   ├── provider_openai/mod.rs      // same pattern
│   ├── provider_local/mod.rs       // same pattern (no key required → silent always)
│   └── connect/screen.rs           // picker gains "Re-enter key" alt-Enter binding per row
└── ui.rs                 // status bar lists all pool members; transcript shows per-turn routing badge
```

The router lives in `otto-host` (not `otto`) because it operates on `CompleteRequest` (SPP) and provider capability metadata, neither of which the TUI crate should know about. The TUI only invokes `host.run_turn_streaming(req)` as today; the router runs inside that call.

`ProviderCapabilities` is a new type carried by each `ProviderSpec` already declared in plugin manifests. It lists the models the provider exposes plus per-model flags (`supports_vision`, `supports_audio`, `context_window`, `cost_tier`). The Anthropic/Gemini/OpenAI plugins each populate this from their hardcoded model lists today (no new network calls).

## Data flow

```
User submits input (possibly with @prefix or attached image)
         │
         ▼
TUI builds CompleteRequest (SPP) → host.run_turn_streaming(req)
         │
         ▼
Router::pick(&req, &pool, &rules)
  ├─ Layer 1: parse @prefix from first user message → Override
  ├─ Layer 2: scan req for Image/PDF/Audio content blocks
  │              → if default lacks modality, pick first connected model that has it
  ├─ Layer 3: evaluate user rules top-to-bottom against req
  ├─ Layer 4: heuristic classifier (opt-in)
  └─ Layer 5: default
         │
         ▼
RoutingDecision { provider_id, model_id, reason }
         │
         ▼
emit TurnEvent::RouteSelected { decision } → TUI renders badge in transcript
         │
         ▼
Router::pick acquires ProviderLease (clones the entry's Arc, ++active_turns).
Pool RwLock is released before the .await below — never held across await.
         │
         ▼
lease.client().complete(req, …)  ← runs the entire tool_use loop on this provider;
                                    /disconnect Drain cannot drop this client out
                                    from under the in-flight loop because the
                                    lease holds the Arc.
         │
         ▼
StreamEvent stream → TUI, identical to today
         │
         ▼
On TurnComplete: append assistant message(s) to host history.
                 tool_use blocks get their IDs namespaced (provider_id:original_id)
                 before being stored, so the next turn's history is unambiguous
                 even if it runs on a different provider.
         │
         ▼
ProviderLease drops (--active_turns). If a Drain disconnect is waiting on this
provider and active_turns reaches zero, the PoolEntry's Arc reaches strong
count zero and the underlying ProviderClient is dropped.
```

## `/connect` semantics — before vs after

| State | Today | After |
|---|---|---|
| Keyring empty, plugin disconnected | Prompt opens; submit saves + connects | Prompt opens; submit saves + connects (unchanged) |
| Keyring has key, plugin disconnected | Prompt opens; user presses Enter to use stored key | **Silent connect** — no modal |
| Plugin already connected | Prompt opens; submit re-keys via stored-key fallback | "already connected; use --rekey to re-enter" note, no modal |
| User wants to re-enter the key | Open `/connect`, press Enter on empty modal | `/connect <provider> --rekey` (or alt-Enter on picker row) |

## Routing config — `~/.otto/routing.toml`

```toml
# Default model (provider/model). Overridable by OTTO_MODEL env var,
# matching the precedence the host already uses for /model selection.
default = "anthropic/claude-opus-4-7"

# Opt in to the built-in keyword/length classifier as a fallback layer.
heuristics = false

[[rule]]
name = "vision-for-images"
match = { has_image = true }
use = "gemini/gemini-2.0-flash-vision"

[[rule]]
name = "haiku-for-shortform"
match = { max_input_chars = 400 }
use = "anthropic/claude-haiku-4-5"

[[rule]]
name = "deep-reasoning"
match = { keywords = ["refactor", "design", "architect", "investigate"] }
use = "anthropic/claude-opus-4-7"
```

`match` predicates compose with AND. `has_image` / `has_pdf` / `has_audio` are derived from the request's content blocks; `keywords` is case-insensitive substring match against the latest user message; `max_input_chars` and `min_input_chars` bound the latest user message length. Future predicates land here additively.

The router parses this file at startup and on `/route reload`. Parse errors fall back to "no user rules" + a styled note in the log, so a typo doesn't strand the user without routing.

`/route show` dumps the active rule list and the last `RoutingDecision` (provider, model, reason) so debugging "why did it pick X" is one slash command away.

## `@provider:model` override syntax

Parsed at the very start of the first user message in a turn. Recognized forms:

- `@anthropic:opus-4.7 <rest>` — explicit provider and model
- `@opus <rest>` — model alias resolves to `(provider, model)` via the host's model registry; ambiguous aliases (same model name on multiple providers) get a styled note + fall back to default, **without consuming the prefix** (the user sees their message go through verbatim so they can fix it)
- `@gemini <rest>` — bare provider, picks the provider's default model

The prefix is stripped before the request is sent to the provider, and `RoutingDecision.reason = Override` is recorded for the transcript badge.

**Escape and fallthrough rules** (so the parser cannot accidentally steal legitimate user text):

- `@@<rest>` at the start of a message means "literal `@`." Exactly one leading `@` is stripped; the message body becomes `@<rest>` and no override is applied. This is the documented way to start a prompt with `@mention …` or `@ai-search …` style text.
- An `@token` at the start that does not match any registered provider, alias, or `@@` escape is **not** consumed. The whole message is sent as-is and no override applies. The router records `reason = Default` and the transcript badge shows the default model, so the user can see that their `@token` wasn't recognized rather than silently swallowed.
- Slash commands take precedence: a line beginning with `/` is consumed by the slash router before the `@`-parser ever sees it. `@`-overrides do not apply to slash command arguments.
- The override applies only to the first user message of the turn (the one the user just typed). It does not re-parse historical messages on every turn.

## History and tool_use ID namespacing

The host owns one `Vec<Message>` per conversation. When a provider returns assistant content containing `ContentBlock::ToolUse { id, name, input }`, the host rewrites `id` to `<provider-id>:<id>` before appending. The matching `ContentBlock::ToolResult { tool_use_id, … }` it appends in the next turn carries the same namespaced ID.

Each `ProviderClient::complete` adapter receives the canonical history with namespaced IDs. On the way out, it:

- If the namespaced prefix matches its own provider id, strips the prefix and emits the vendor-shape with the original ID.
- Otherwise, treats the whole namespaced ID as an opaque string. Anthropic/Gemini/OpenAI all accept arbitrary string IDs in prior-turn `tool_use`/`tool_result` blocks as long as they round-trip within a single request, so a turn handled by Anthropic sees a Gemini-tool-use with id `gemini:abc-123` as just another tool call in history.

Tool definitions (`tools` field on the outgoing request) are filtered per provider when a provider's vendor format disallows certain tool shapes — but in practice all three current vendors accept the SPP tool shape, so v1 does no filtering and registers all configured tools with every provider.

## UI changes

- **Status bar** (today: `home.footer.left` slot per-provider): each connected provider's slot renders. The provider that handled the most recent turn gets a leading "▸ " marker. Disconnected providers render nothing (today's behavior, unchanged).
- **Transcript per-turn badge:** each assistant turn entry includes a small `[provider/model — reason]` line above the response text. `reason` is one of `Override`, `Modality(image)`, `Rule(name)`, `Heuristic(short)`, `Default`. Plain text, themed via existing `ThemeColor::Muted`.
- **Connect picker:** Enter still emits `/connect <id>`. Alt-Enter (or `r` on the focused row) emits `/connect <id> --rekey` so the keyboard-only path covers re-keying. Footer hint updates accordingly.
- **`/disconnect <provider>`** new slash command. Default mode is **drain**: the provider is removed from new-turn eligibility immediately, but in-flight turns leasing it run to completion before the underlying `ProviderClient` is dropped. A small note ("Anthropic disconnecting; finishing 1 turn") appears for the duration. `/disconnect <provider> --force` triggers an explicit cancellation: the in-flight turn aborts with `TurnEvent::Cancelled { reason: ProviderDisconnected }`, the user sees a clear "turn cancelled because provider was force-disconnected" line, and the provider is dropped immediately. See "Pool lifecycle and turn leases" for the contract. If the disconnected provider was the default, the next turn falls back to whatever's still in the pool with a note; if the pool is empty, the host behaves as today's "not connected" state.

## Pool lifecycle and turn leases

The pool needs explicit rules for what happens when the user mutates it while a turn is in flight. Without those rules, `/disconnect` mid-stream is a use-after-free in slow motion.

**Lease model.** Each `PoolEntry` carries an `Arc<dyn ProviderClient + Send + Sync>` and an `active_turns: AtomicUsize` counter. `Router::pick` returns a `(ProviderId, ModelId, ProviderLease)` triple where `ProviderLease` is a small RAII guard that:

- holds a cloned `Arc<dyn ProviderClient>` (so the underlying client stays alive until every lease is dropped, even if the pool entry is removed in between),
- increments `active_turns` at construction and decrements on drop,
- exposes the cloned `Arc` via `.client()` so the streaming turn loop can call `complete` on it without re-entering the pool lock.

The turn loop holds its `ProviderLease` for the entire user-turn duration (including all tool-use iterations). The pool's `RwLock` is acquired only by `Router::pick` (read) and by `add_provider`/`remove_provider` (write); it is never held across an `.await` on `complete`. This mirrors the `Arc<RwLock<Option<Arc<Host>>>>` pattern the TUI already uses for host swaps and obeys the existing "clone Arc under brief read lock, drop guard before any .await" rule (project_tui_design).

**Disconnect modes.** `Host::remove_provider(id, mode)` takes a `DisconnectMode`:

- `DisconnectMode::Drain` (default; what `/disconnect <provider>` issues). Removes the entry from new-turn eligibility immediately. Existing `ProviderLease`s keep the `Arc<dyn ProviderClient>` alive until they drop. When `active_turns` reaches zero, the `Arc`'s strong count hits zero and the client is dropped (its `Drop` impl handles HTTP connection teardown, etc.). The TUI surfaces a transient note ("Anthropic disconnecting; finishing N turns") that clears when the counter reaches zero.
- `DisconnectMode::Force` (`/disconnect <provider> --force`). Removes from eligibility AND signals every active turn on the provider to cancel. Cancellation has three stages, in order:
  1. **Cooperative cancel.** `TurnEvent::Cancelled { reason: ProviderDisconnected(provider_id) }` is emitted immediately so the UI labels the cancellation correctly. The turn loop's `select!` between provider stream and cancel signal observes the signal and unwinds, dropping the `ProviderLease`. This handles the well-behaved case where the in-flight `ProviderClient::complete` future is cancellation-cooperative (true for the rmcp StreamableHttp transport and for the in-process SDK paths that don't park threads).
  2. **Bounded grace period.** Host waits **500 ms** (`force_disconnect_grace_ms`, configurable in `HostConfig`) for `active_turns` to reach zero. This window absorbs the time the SDK needs to land its terminal poll, drop the connection, and let its drop glue run.
  3. **Hard abort.** If `active_turns` is still non-zero after the grace window, the host calls `JoinHandle::abort()` on each lingering turn task and force-drops the `PoolEntry`'s `Arc`. The user-visible `TurnEvent::Cancelled` from stage 1 is followed by a `TurnEvent::AbortedAfterGrace { reason: ProviderDisconnected }` so the transcript shows clearly that the SDK was not cancellation-cooperative and was forcibly stopped. Any `Arc` clones still held outside the host (defensive code paths, leak bugs) become orphans the host explicitly disowns; their owners are responsible for cleanup. This mirrors the `rmcp` `ProgressDispatcher` pattern already in the codebase (project_rmcp_progress_gotcha) — abort the task or the channel deadlocks.

The point of the three-stage protocol: the user always gets a deterministic resolution within 500 ms of issuing `/disconnect --force`. There is no path where the TUI hangs waiting for a stuck SDK call.

**Capacity guard.** `add_provider` rejects a registration whose `ProviderId` is already present with `PoolError::AlreadyRegistered`. To re-register (e.g., to swap in a new API key without losing in-flight turns), the TUI does `remove_provider(id, Drain)` then `add_provider(...)` once the previous entry drains — or accepts the disruption and issues `Force` first.

**Tool registry invariants.** `ToolRegistry` is unchanged: it is owned by the `Host` directly, not per-provider, and stays alive as long as the `Host` does. Tools never lease providers; providers lease nothing from tools. The two systems are decoupled today and stay decoupled.

## Crate boundary and capability flow

The router lives in `otto-host`, which depends on `otto-protocol` and `otto-mcp` but **not** on `otto` (the TUI crate). Plugin manifests live in `otto`. That dependency direction is fixed by the workspace topology and must not invert.

Capabilities therefore flow *into* the host at construction time:

```rust
// In otto-host::config
pub struct ProviderRegistration {
    pub id: ProviderId,
    pub display_name: String,
    // Arc — not Box — so the same handle the pool stores is the same handle
    // ProviderLease clones. There is exactly one ownership conversion point
    // in the system: the plugin wraps its concrete client in Arc::new(...)
    // once, then hands ownership through. Host::add_provider stores the Arc
    // directly into the new PoolEntry; no Box→Arc round-trip exists.
    pub client: Arc<dyn ProviderClient + Send + Sync>,
    pub capabilities: ProviderCapabilities,
    pub aliases: Vec<ModelAlias>,
}

pub struct HostConfig {
    // … existing fields …
    pub providers: Vec<ProviderRegistration>,
    pub startup_connect: StartupConnectPolicy,
    pub routing_rules_path: Option<PathBuf>, // ~/.otto/routing.toml by default
}

pub struct ProviderCapabilities {
    pub models: Vec<ModelCapabilities>,
    pub default_model: ModelId,
}

pub struct ModelCapabilities {
    pub id: ModelId,
    pub display_name: String,
    pub supports_vision: bool,
    pub supports_audio: bool,
    pub context_window: usize,
    pub cost_tier: CostTier, // Free / Cheap / Standard / Premium
}

pub struct ModelAlias {
    pub alias: String,         // "opus", "haiku", etc.
    pub provider: ProviderId,
    pub model: ModelId,
}
```

Mirror methods on `Host` for runtime updates:

- `Host::add_provider(reg: ProviderRegistration) -> Result<(), PoolError>` — used by `/connect`. Takes the same shape as a `HostConfig::providers` entry, so the TUI has one code path that builds the registration regardless of whether it's startup or a slash command.
- `Host::update_capabilities(id: &ProviderId, caps: ProviderCapabilities)` — used when a provider plugin refreshes its model list at runtime (rare). Does not affect the active turn lease.
- `Host::reload_routing_rules()` — re-reads `routing.toml`; errors fall back to "no user rules" + a styled note in the log.

The TUI's provider plugins continue to declare their `ProviderSpec` in their `Manifest` as today; the TUI's startup wiring (in `crates/otto/src/main.rs`) reads those specs to construct each `ProviderRegistration`. The host never sees `Manifest`, `Plugin`, or any other plugin-runtime type.

## Phase 2 gate: cross-vendor tool_use ID compatibility

Phase 3 (`@provider:model` override) introduces the first scenario where one user's conversation can have turn N served by provider A and turn N+1 served by provider B with shared history. That requires provider B's vendor SDK to accept history blocks that contain `tool_use_id`s minted by provider A. If a vendor rejects unrecognized ID shapes, every cross-provider turn fails with an opaque vendor error.

Phase 2 of the phasing plan is dedicated to standing up the CI compatibility matrix that proves (or, where needed, fixes) this assumption *before* any user-facing routing ships. This is a **release blocker for Phase 3 and every subsequent phase**. The gate:

1. **Per-vendor compatibility test.** New `crates/otto-host/tests/cross_vendor_history.rs` builds a synthetic `Vec<Message>` containing a `ContentBlock::ToolUse { id: "foreign-prefix:abc-123", … }` followed by a `ContentBlock::ToolResult { tool_use_id: "foreign-prefix:abc-123", … }`. For each `(sender_provider, receiver_provider)` pair across the three vendors, the test submits a `CompleteRequest` with that history and asserts the call succeeds (no 4xx, no `ProviderError::BadRequest`). Tests run against the vendor SDKs in-process; they hit the real vendor APIs only in nightly CI gated behind credentials, and run against vendor mocks/replays in PR CI.
2. **Vendor-specific fallback strategy** for any failing pair. If e.g. Gemini rejects Anthropic-style IDs, the Gemini adapter rewrites foreign IDs to short opaque hashes (`sha256(provider_id || ":" || original_id)` truncated to 8 chars, namespaced to avoid collision with Gemini's own ID format) before serialization, and restores them on the way back. The opaque token is recorded in a per-turn map kept in `SessionState` so the next turn's history serializer can map them back to canonical namespaced IDs.
3. **Default-fallthrough behavior.** Until the gate passes for a vendor, that vendor is excluded from cross-provider routing in release builds: the `@`-override and modality routing refuse to switch *to* it from a different provider mid-conversation, and the user sees a styled note ("Anthropic → Gemini routing is unavailable in this build; ID compatibility test not yet green"). Single-provider conversations are unaffected.

Phase 3 cannot ship to master until every vendor pair in the current shipping set passes (or has its fallback implemented and tested). The gate is checked in CI: a `release-gate` test target that fails when the compatibility matrix has gaps. Phase 2's own deliverable is the test target itself plus any fallback adapters needed to make the matrix green.

## Startup auto-connect policy

The host accepts `StartupConnectPolicy` in `HostConfig`. The TUI reads the user's choice from `~/.otto/config.toml` (a new file; `routing.toml` would be the wrong home for an account/connectivity knob) and constructs the policy at launch:

```toml
# ~/.otto/config.toml
[startup]
# "opt-in" (default): only connect providers in startup_providers
# "all":              today's behavior — every provider with a keyring entry
# "last-used":        only the provider(s) used in the previous session
# "none":             never auto-connect; require explicit /connect
policy = "opt-in"
startup_providers = ["anthropic"]

# Per-provider startup timeout. After this elapses, auto-connect for that
# provider is abandoned with a styled note; other providers continue.
# Default: 3000ms. The TUI never blocks user input on startup connects.
connect_timeout_ms = 3000
```

**Why opt-in by default.** A user could plausibly have keyring entries for Anthropic, Gemini, OpenAI, and a local provider all from past experiments. Auto-connecting all four on launch creates network traffic and (for paid providers) the perception of cost exposure they did not consent to in the current session. Defaulting to the single most-recently-installed provider — or specifically Anthropic on first launch — keeps the "it just works" UX while not surprising the user with cross-vendor activity. Users who want the old "connect everything" behavior set `policy = "all"`.

**Failure UX.** Each startup connect runs in parallel up to `connect_timeout_ms`. Failure modes the user sees in the log:

- Timeout: "Anthropic auto-connect timed out after 3s; run `/connect anthropic` to retry."
- Keyring read error: "Anthropic auto-connect failed: keyring unavailable; run `/connect anthropic` to enter a key."
- Provider client build error (TLS init, bad proxy env, etc.): "Anthropic auto-connect failed: <error>; run `/connect anthropic` to retry."

None of these block the TUI from coming up; the user can always finish startup with zero providers and connect interactively.

**Migration.** Users upgrading from a pre-pool release already have keyring entries. Writing every detected key into `startup_providers` would silently restore the "auto-connect everything" behavior we just made opt-in — defeating the purpose of the new default. Instead:

- On first launch of the new version (gated on a `~/.otto/state.toml` "migration_v1_done" marker), if more than one keyring entry exists, the TUI opens a one-time **startup-providers picker** modal: "We found stored keys for Anthropic, Gemini, OpenAI. Which should auto-connect when otto starts? [space to toggle, Enter to confirm]." The user's selection writes `config.toml` with `policy = "opt-in"` and `startup_providers = [their selection]`. The marker is set regardless of choice so the picker never re-runs.
- If the user dismisses the modal (Esc), the TUI falls back to a deterministic single default: Anthropic if a key exists, else the alphabetically first detected provider id. `config.toml` is written with that one provider in `startup_providers`. The user can edit later.
- If exactly one keyring entry exists, no modal — `startup_providers` is written with just that provider (same effective behavior as today, no surprise).
- If zero keyring entries exist, `config.toml` is written with `startup_providers = []` and the user goes through normal `/connect` flow.

The migration is one-time per user (the marker prevents re-prompts) and the modal text is i18n'd so it lands properly under the locale loaded at startup.

## Legacy environment compatibility

`OTTO_MODEL` exists today as a bare model name (e.g., `claude-opus-4-7`). With multi-provider, the canonical form is `provider/model` (e.g., `anthropic/claude-opus-4-7`). The router's `legacy_model.rs` resolver handles both shapes with this precedence:

1. **`provider/model` form.** Split on the first `/`. Validate provider is in the pool. If provider exists but the model is unknown to that provider, log a warning and fall back to the provider's default model (not to a different provider). If provider is not in the pool, log a warning and fall back to the default model from `routing.toml`.
2. **Bare-model form** (no `/`). Scan all *connected* providers' `ModelCapabilities` lists for a model with a matching `id` or `alias`. Resolution rules:
   - Exactly one match → use it. Log at `info` ("`OTTO_MODEL=claude-opus-4-7` resolved to `anthropic/claude-opus-4-7`") so the resolution is auditable.
   - Multiple matches → ambiguous. Log a warning naming every match, ignore `OTTO_MODEL`, fall back to the default. The user fixes by switching to `provider/model` form.
   - Zero matches → log a warning, ignore, fall back to default.
3. **`~/.otto/models.toml`** legacy entries follow the same parser. The model field there has always been a bare string; entries that resolve ambiguously get the same warning + fallback. The user can edit the file to the new `provider/model` form to silence the warning.

The parser is pure (no I/O once `ProviderCapabilities` is in hand), tested in isolation in `crates/otto-host/src/router/legacy_model.rs`, and runs on every startup and on every `/model` invocation. Warnings surface as styled notes in the TUI log, not just `tracing::warn!`, so users see them when they happen.

## Phasing

Shipping all four routing signals + the pool refactor as one PR is too big. Proposed slicing — each phase is independently shippable and observable:

1. **Pool foundation.** Host gains `provider_pool` with `PoolEntry`/`ProviderLease`; `Host::add_provider`/`remove_provider` land with both `Drain` and `Force` disconnect modes; `HostConfig::providers` and `StartupConnectPolicy` land; `/connect` becomes additive and silent-when-stored; `--rekey` flag implemented; `~/.otto/config.toml` migration runs; status bar lists all pool members. The host carries an **`active_provider: ProviderId`** field. **All turns in Phase 1 route to the active provider; `/model` in Phase 1 only lists models from the active provider's `ProviderCapabilities`.** Switching to a different provider's model requires explicit `/use <provider>` (a temporary Phase 1 slash command) which clears the conversation (same as today's provider swap) and updates `active_provider`. This deliberately defers cross-provider history paths until the Phase 2 gate is green — Phase 1's user-visible multi-provider behavior is "multiple connected, one active per conversation." The new `legacy_model.rs` resolver lands in this phase to handle bare-model `OTTO_MODEL` against the active provider's catalog. **This phase alone closes the re-prompt complaint, and ships with the lifecycle/lease contract so later phases inherit safe semantics.**
2. **Phase 2 gate: cross-vendor tool_use ID compatibility.** Not a user-visible feature — this is a CI-only deliverable that establishes the `release-gate` test target described in "Phase 2 gate." No code other than tests + per-vendor fallback adapters ships. **No subsequent phase merges to master until this gate is green.** Until then, Phase 1's "one active provider per conversation" constraint is the active safety invariant.
3. **`@provider:model` override + cross-provider conversations.** Removes Phase 1's "one active per conversation" constraint. Adds the `@`-prefix parser (with `@@`-escape rules), the `Router` skeleton, `RoutingDecision`, and the transcript badge. This is the first phase where one conversation's history can contain tool_use blocks from multiple providers; it depends on the Phase 2 gate being green for every vendor pair the user has connected. `/use <provider>` from Phase 1 graduates to a normal model picker since cross-provider history is now safe.
4. **Modality routing.** Add `ProviderCapabilities` consumption + per-model `supports_vision` flag; router auto-redirects image-bearing turns. This is the marquee multi-model use case ("Gemini Vision for multimodal tasks").
5. **User rules from `routing.toml`.** Most flexible, fully debuggable since the user owns the policy. Adds `/route reload` and `/route show`.
6. **Heuristic classifier.** Opt-in via `heuristics = true` in routing.toml. Riskiest UX (opaque boundary cases), shipped last so we can see real usage from phases 3-5 before guessing keyword lists.

Each phase gets its own version bump + release notes + README update (per [[feedback_release_notes]] and [[feedback_release_docs]]).

## Testing strategy

- **Pool foundation:** unit tests in `otto-host/src/session.rs` for `add_provider` / `remove_provider` / `PoolError::AlreadyRegistered`. Integration test in `crates/otto/tests/` that connects two providers in sequence via `/connect` and asserts both `ProviderRegistered` events fired and both render in the status bar slot. Re-prompt regression: assert that `/connect anthropic` with a stored key does **not** emit `Effect::PromptApiKey`.
- **Lease and disconnect — cooperative path:** `remove_provider(id, Drain)` while a synthetic streaming turn holds a `ProviderLease` keeps the inner `Arc<dyn ProviderClient>` alive until the lease drops; the provider is gone from new-turn eligibility immediately but the in-flight turn completes successfully. `remove_provider(id, Force)` against a cancellation-cooperative stub provider causes the in-flight turn to emit `TurnEvent::Cancelled { reason: ProviderDisconnected }` and exit within a few ms.
- **Lease and disconnect — uncooperative path:** stub `ProviderClient::complete` that holds for 5s without awaiting any cancel-cooperative point. `remove_provider(id, Force)` emits `Cancelled` immediately, waits 500ms (`force_disconnect_grace_ms`), then aborts the task and emits `AbortedAfterGrace`. Assert the total wall-clock from `/disconnect --force` to `active_turns == 0` is ≤ 600ms (500ms grace + slack for task scheduling). This guards against the project_rmcp_progress_gotcha pattern leaking into the pool.
- **Lock hygiene:** assert no test scenario holds the pool `RwLock` across an `.await` (use `tokio::task::yield_now()` between `Router::pick` and `complete` to expose any accidental guard retention).
- **Phase 1 cross-provider safety:** with Anthropic and Gemini both connected, `/model` only lists Anthropic's models if `active_provider == anthropic`; the list updates after `/use gemini`. Direct attempts to set a model from the inactive provider (e.g. via `OTTO_MODEL`) get a styled note + fall back to the active provider's default. `/use <provider>` clears history before switching `active_provider`. Regression test: assert that after a turn on Anthropic + `/use gemini` + new turn, the second `CompleteRequest` has empty prior-turn history (no `anthropic:`-namespaced tool_use IDs leak across).
- **Startup policy:** with `policy = "opt-in"` and `startup_providers = ["anthropic"]`, only Anthropic is in the pool after `Host::start` even though Gemini and OpenAI both have keyring entries. With `policy = "all"`, all three are connected. With `policy = "none"`, the pool is empty. Connect timeout: a registration whose provider client build takes > `connect_timeout_ms` is abandoned with a styled note; the host comes up regardless.
- **Migration:** pre-existing `~/.otto/state.toml` absent + multiple keyring entries → on first launch, the startup-providers picker opens. Confirming a selection writes `policy = "opt-in"` + `startup_providers = <selection>` and sets the migration marker. Dismissing the picker writes `startup_providers = ["anthropic"]` (or first alphabetically if no anthropic). Exactly one keyring entry → no picker; that one provider is written. Zero entries → empty `startup_providers`. Re-run with marker present → picker never opens again regardless of pool contents.
- **Phase 2 gate (`crates/otto-host/tests/cross_vendor_history.rs`):** for each `(sender, receiver)` pair across Anthropic, Gemini, OpenAI, submit a `CompleteRequest` with prior-turn history containing a `ContentBlock::ToolUse { id: "<sender>:abc-123", … }` and matching `ToolResult`. Assert the call succeeds. PR CI uses recorded vendor replays; nightly CI uses real credentials. Any failing pair must have a fallback adapter that rewrites IDs to short opaque hashes and tests the round trip.
- **Router layered dispatch:** unit tests in `otto-host/src/router/` per layer — override prefix parsing (including `@@`-escape and unknown-token fallthrough), modality detection on synthetic `CompleteRequest`s, rule eval with a fixture `routing.toml`, heuristic classifier on canned inputs, `legacy_model.rs` resolver for bare/qualified `OTTO_MODEL` including ambiguity warnings. End-to-end test: build a request with an image attached, default model lacks vision, router picks the vision-capable provider, `RoutingDecision.reason == Modality(image)`.
- **History with namespaced tool_use IDs:** turn 1 routes to Gemini, returns a tool_use; turn 2 routes to Anthropic; assert Anthropic sees `gemini:<id>` in history and the matching tool_result resolves correctly. (Use the existing `MockProvider` pattern in `provider-anthropic`/`provider-gemini` test modules.)
- **Locale isolation** for any test that reads styled notes: per [[feedback_test_locale_isolation]], reset to "en" inside `HOME_LOCK` so parallel test runs don't poison the mutex.
- **Streaming permissions** for any router tests that exercise tool_use loops: pre-register `Allow` via `host.add_session_rule(...)` per [[feedback_streaming_test_permissions]] so the synthetic turn doesn't hang.

## Open questions / risks

Resolved in this design and no longer open: vendor tool_use ID acceptance (now the "Phase 2 gate" with explicit fallback strategy); concurrent pool mutation safety (now "Pool lifecycle and turn leases" with `Drain`/`Force` + cooperative cancel + bounded grace + abort); startup auto-connect surprise (now "Startup auto-connect policy" with opt-in default and one-time migration picker); host/plugin capability ownership (now "Crate boundary and capability flow" with `Arc`-end-to-end `ProviderRegistration`); prefix override stealing user text (now `@@`-escape + unrecognized-token fallthrough); legacy `OTTO_MODEL` ambiguity (now "Legacy environment compatibility"); cross-provider history paths in Phase 1 (now blocked by the "one active provider per conversation" invariant; lifted in Phase 3 once the gate is green).

Still open:

- **`--rekey` discoverability.** A flag isn't great UX for a re-keying flow. The Alt-Enter row binding in the picker partially covers it, but a `/connect <provider>` invocation where the plugin already has a client could also offer an inline "(press R to re-enter key)" hint. Decide during Phase 1 implementation; this is a one-line change either way.
- **Default model when pool is empty.** Today, "no provider" = "not connected" with a clear UI state. With the pool model, a user could `/disconnect` everything and end up in the same state — make sure the UI doesn't claim "no router decisions yet" when the actual problem is "no providers."
- **Routing visible cost.** Adding the badge to every assistant turn is one extra line per turn. Worth A/B-ing inline (small `▸ anthropic/opus-4.7 — Modality(image)`) vs. on-hover / on-demand (`/route show` only). Phase 3 picks one; the cheap default is inline + muted.
- **Phase 2 gate CI infrastructure.** Recorded vendor replays for the compatibility test (or VCR-style fixture cassettes) need an authoring path that doesn't require running tests against live vendors locally. Decide whether to lean on an existing crate (`vcr-cassette`, `rstest_reuse` + custom JSON fixtures) or hand-roll a thin recorder during Phase 2 implementation. Affects schedule but not design.
- **Fate of `/use <provider>` in Phase 3.** Phase 1 introduces `/use <provider>` as a clear-and-switch action. Phase 3 makes cross-provider switching safe without clearing history, so `/use` may either (a) graduate to a routing-aware "set default provider" command, (b) be retired in favor of `@provider:model` overrides, or (c) coexist as a sticky per-conversation override. Decide during Phase 3 design; documenting one choice now would prejudge UX feedback from Phase 1.
- **Force-grace value.** 500ms is a guess for "long enough that cooperative cancel usually finishes, short enough that the user doesn't notice." First Phase 1 dogfooding will tell us whether it's too tight (cooperative SDK paths get aborted unnecessarily) or too loose (uncooperative SDK paths feel like hangs). The value is a `HostConfig` field so the fix is a config bump, not a code change.

## Out of scope (for tracking; not shipping here)

- Cost-tier routing (model selection driven by `cost_tier` + budget). Possible Phase 6+ once usage data exists.
- Per-conversation default model override (today's `/model` is global; users have asked for per-thread). Orthogonal to multi-provider; can land independently.
- Cross-provider response diff / "ask two models in parallel and show both." Distinct feature, distinct UX, not implied by anything in this design.
