# Restore self-update detection and install flow — design

Date: 2026-09-07
> **Status:** pending review
Related: `savvagent/otto#65`

## Problem

Otto's self-update path currently has two correctness gaps that combine into the issue #65 failure mode:

- The startup check in `crates/otto/src/plugin/builtin/self_update/mod.rs:382-454` trusts any fresh `~/.otto/update-check.json` entry whose cached tag is not *ahead* of the running version. When that cached tag equals the running version, `run_check_once` short-circuits to `UpToDate` without hitting GitHub (`mod.rs:585-626`, `cache.rs:59-100`). If a new release was published after the cache entry was written but before its 24h TTL expires, startup incorrectly suppresses the update banner/auto-install until the later periodic tick.
- `/update` in `mod.rs:324-380` is state-driven only. It retries the installer only when the plugin is already in `Available` or `InstallFailed`; for `Unknown`, `UpToDate`, and `CheckFailed` it emits a note and never performs a live release check. That means the stale-cache `UpToDate` state above also makes `/update` no-op instead of discovering and installing the newer release.

Issue #65's expected behavior is stricter than the current cache optimization: when Otto is older than the latest release, the user should be notified promptly, and `/update` should actively fetch/apply the newest release or tell the user exactly why it could not.

## Goal & Success Criteria

Make self-update correctness win over the optimistic cache so an out-of-date Otto promptly discovers new releases at startup, and make `/update` perform an authoritative live check before deciding whether to install.

Success criteria:
- A fresh cache entry that only proves "this version was current sometime in the last 24h" no longer suppresses a startup check when Otto may now be behind the latest release.
- Startup still uses cached data when it already proves a newer release is available, so users get a prompt/banner immediately even before the network round-trip completes.
- `/update` performs a live GitHub release check when invoked from `Unknown`, `UpToDate`, `CheckFailed`, `Available`, or `InstallFailed`, bypassing the optimistic startup cache and installing when a newer tag exists.
- Failure cases remain actionable: disabled/dev-build flows still explain why no update will run, and live-check or install failures surface explicit notes/banners instead of silent no-ops.
- Targeted self-update tests and workspace validation pass.

## Approach

1. **Tighten first-tick cache trust.** Keep the existing cache file and TTL, but change the first-tick decision in `crates/otto/src/plugin/builtin/self_update/mod.rs` so cached data is only authoritative when it indicates the running binary is *behind* the cached tag. A cached `Equal` result becomes a "must revalidate now" case instead of a startup skip, because it cannot prove that GitHub has not published a newer release since the cache write.
2. **Split cached classification from live classification.** Add a small helper in `self_update/mod.rs` to classify cached tags and decide whether to trust them. This keeps the policy local to the plugin rather than overloading `cache.rs` (which should stay a format/TTL helper) or `check.rs` (which should stay a pure GitHub-fetch + semver module).
3. **Make `/update` authoritative.** Refactor the slash handler in `self_update/mod.rs` so `/update` can perform a live `check_for_update` call itself, independent of the background task's current state. The command should still respect `Disabled` and dev-build short-circuits, but otherwise it should fetch the latest tag now, update plugin state with the result, and run `run_install` when the live result is `Available`.
4. **Preserve the existing plugin boundaries.** The fix stays entirely inside `crates/otto`'s self-update plugin and README text. No host turn-loop changes, no tool transport changes, no provider pool changes, and no host-swap lock changes are needed.
5. **Document the corrected behavior.** Update the README's `/update` description to say it performs a live re-check before install rather than merely "re-run the latest-release install," because the command semantics are part of the documented slash-command surface.

## Scope

**In:**
- `crates/otto/src/plugin/builtin/self_update/mod.rs`
- Self-update tests in that same module
- `README.md` documentation for `/update` and startup checking behavior

**Out:**
- Replacing the on-disk cache format or TTL
- Changing install-asset selection, release packaging, or cargo-dist configuration
- Altering slash-command names, config schema, env vars, transcript format, or keyring behavior
- Any host/provider/tool architecture changes outside the self-update plugin

## Public-interface changes

No public interface shape changes are intended. The `/update` slash command name, `OTTO_NO_UPDATE_CHECK`, `--no-update-check`, the update cache file format, and `~/.otto/config.toml`'s `[update]` section all stay the same.

There is a user-visible **behavior correction** to the documented slash-command surface: `/update` will now perform a fresh release check before deciding whether there is anything to install, rather than relying on whatever state the background checker last published. That is additive/corrective behavior, not a breaking interface change.

## Premise corrections

1. **The banner path is not entirely absent.** `render_slot` already renders `Available`, `Installing`, `InstallFailed`, `Updated`, and `CheckFailed` states via the `home.banner` slot (`mod.rs:457-495`, `ui.rs:268-322`). The notification bug is that startup can get stuck in an incorrect `UpToDate` state due to stale optimistic cache reuse, not that the UI has no banner surface.
2. **`/update` is not an unconditional installer today.** The current implementation is effectively a retry/status command because it only installs from pre-existing `Available` or `InstallFailed` state (`mod.rs:333-378`). Fixing issue #65 requires changing that behavior, not merely fixing the installer subprocess.

## Assumptions

- **The 24h cache should remain as an optimization, not be deleted outright.** Rationale: the issue asks for correctness, not for removing all startup caching, and the existing cache still provides value when it already proves a newer release exists.
- **A cached `UpToDate` result is not trustworthy enough for startup suppression.** Rationale: it only proves there was no newer release at cache-write time, not at current launch time.
- **`/update` should bypass the startup cache entirely.** Rationale: a user invoking the command is explicitly asking for an authoritative check-and-install now.
- **Disabled and dev-build short-circuits remain unchanged.** Rationale: issue #65 targets broken installed-release behavior, not local-dev ergonomics or the opt-out contract.
- **Existing installer execution and error propagation are sufficient unless live-check testing exposes otherwise.** Rationale: the issue's observed no-op is already explained by state/cache logic, so avoid widening scope into packaging until verification proves a second defect.

## Error Handling & Edge Cases

- If GitHub lookup fails during `/update`, set/preserve `UpdateState::CheckFailed` and return an explicit note; do not silently leave the prior optimistic `UpToDate` state in place.
- If the startup live revalidation fails after an `Equal` cached tag, publish `CheckFailed` so the banner can explain the check failure rather than silently implying no update exists.
- If cached data shows a newer release (`Behind`), keep the fast path: publish/install from cache immediately rather than delaying the user-visible signal on network latency.
- `/update` invoked while an install is already running should keep the existing in-progress note instead of starting a second installer.
- `InstallFailed` with the same tag should still preserve the failure context for the background periodic path; `/update` should be allowed to retry that same tag intentionally because the user asked for it.

## Risks & Open Questions

- **Risk: more startup GitHub requests when the cache previously said `UpToDate`.** This is intentional; correctness on startup now outweighs the stale-cache optimization. The cache still suppresses some requests when it already proves a newer release exists.
- **Risk: `/update` state transitions now come from two call sites (background loop and slash handler).** Tests should cover the new slash flow to ensure state publication and install invocation stay serialized through the existing `Installing` guard.
- **Open question deferred unless validation disproves the main hypothesis:** whether unsupported install locations outside the cargo-dist receipt path need separate detection and messaging. Current scope assumes issue #65 is fully explained by stale cache + non-authoritative `/update` behavior.
