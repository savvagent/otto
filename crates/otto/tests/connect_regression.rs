//! Regression anchor for the "/connect re-prompts when key is stored" bug.
//!
//! See `docs/superpowers/specs/2026-05-15-multi-provider-pool-and-auto-routing-design.md`.
//!
//! ## Background
//!
//! Before Phase 1, every `/connect <provider>` invocation opened the API-key
//! modal even when the keyring already held a valid key. The user had to know
//! to press Enter on the empty modal to "use stored key." After Phase 1's
//! silent-connect path (Task 7), `/connect` reads the keyring first and opens
//! the modal only when no key is stored or `--rekey` is explicitly passed.
//!
//! ## Why this file is a stub
//!
//! `otto` is a binary-only crate: it has no `lib.rs`, so items under
//! `src/plugin/builtin/` are not reachable from `tests/` (integration tests
//! can only import `pub` items from a crate's library root). Adding a `lib.rs`
//! would require meaningful Cargo + module restructuring that would risk
//! breaking the TUI build mid-branch; that cost outweighs the cosmetic benefit
//! of moving the assertions here.
//!
//! ## Where the real tests live
//!
//! The four scenarios that lock in this regression are tested as `#[serial]`
//! Tokio unit tests inside the per-plugin `mod tests` blocks (Task 7):
//!
//! | Test name | Location |
//! |---|---|
//! | `handle_slash_with_stored_key_skips_modal` (anthropic) | `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs` |
//! | `handle_slash_with_stored_key_skips_modal` (gemini) | `crates/otto/src/plugin/builtin/provider_gemini/mod.rs` |
//! | `handle_slash_with_stored_key_skips_modal` (openai) | `crates/otto/src/plugin/builtin/provider_openai/mod.rs` |
//! | `handle_slash_with_rekey_flag_opens_modal_even_when_client_exists` | `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs` |
//!
//! Run them with:
//!
//! ```text
//! cargo test -p otto -- handle_slash_with_stored_key_skips_modal
//! cargo test -p otto -- handle_slash_with_rekey_flag_opens_modal_even_when_client_exists
//! ```
//!
//! `git grep handle_slash_with_stored_key_skips_modal` will find all three
//! provider sites at once.
//!
//! ## If you are refactoring the silent-connect path
//!
//! You are about to touch code guarded by the tests listed above. Before
//! merging, verify that **all three providers** still emit `RegisterProvider`
//! (not `PromptApiKey`) when a keyring entry is present, and that `--rekey`
//! still forces the modal regardless of keyring state.
