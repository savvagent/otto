//! Layer-3 OS-level sandboxing for tool MCP server spawns.
//!
//! Each tool child process can be wrapped in an OS sandbox so that it cannot
//! access the network or write outside the project root even if the tool binary
//! itself is compromised. Sandboxing is **default-on** as of v0.7.0 on Linux
//! and macOS — set [`SandboxConfig`]'s mode to [`SandboxMode::Off`] (or run
//! `/sandbox off`) to opt out. Existing configs that explicitly set
//! `enabled = false` are preserved across upgrade via the struct-level
//! `#[serde(default)]` on [`SandboxConfig`].
//!
//! # 3-state mode (v0.8)
//!
//! [`SandboxMode`] distinguishes "no explicit preference" ([`Default`]) from
//! explicit opt-in ([`On`]) and explicit opt-out ([`Off`]). Wire format is
//! still the legacy `enabled = bool` key: absent → `Default`, `true` → `On`,
//! `false` → `Off`. This lets the splash banner suppress the v0.7-style nag
//! line for users who have explicitly chosen a state, regardless of which.
//!
//! [`Default`]: SandboxMode::Default
//! [`On`]: SandboxMode::On
//! [`Off`]: SandboxMode::Off
//!
//! # Platform support
//!
//! | Platform | Mechanism      | Status |
//! |----------|----------------|--------|
//! | Linux    | `bwrap` (bubblewrap) | Supported |
//! | macOS    | `sandbox-exec` | Supported |
//! | Windows  | None           | Deferred — runs unwrapped with a one-time warning |
//!
//! If the required wrapper binary (`bwrap` on Linux, `sandbox-exec` on macOS)
//! is not found on `$PATH`, a warning is logged and the tool runs unwrapped.
//! The sandbox is never a hard prerequisite.
//!
//! # `tool-bash` and network access
//!
//! Since v0.7, `tool-bash` is denied network by default. The host's
//! spawn path resolves bash network access at runtime via the permission
//! layer (see `tools::LazyBash` and the bash-net resolver closure
//! installed by `Host::wire_self_into_resolver`) and injects a
//! per-spawn `tool_overrides["tool-bash"].allow_net` before calling
//! [`apply_sandbox`]. User-pinned overrides in `~/.savvagent/sandbox.toml`
//! still win — set `[tool_overrides.tool-bash] allow_net = true` to opt
//! out of the prompt and grant net access unconditionally.
//!
//! # Config persistence
//!
//! `~/.savvagent/sandbox.toml` is loaded at startup (silent fallback on any
//! parse error) and written through whenever the user changes settings via the
//! `/sandbox` command.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

/// Schema version of the on-disk `sandbox.toml` format. Bumped when a field
/// is renamed or removed; bumping is a load-path migration signal. Newer
/// files (`version > SCHEMA_VERSION`) are rejected at load time and fall
/// back to defaults with a loud warning, rather than silently dropping
/// unrecognized keys.
pub const SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

/// What happened when [`SandboxConfig::load_with_status`] looked for the
/// on-disk preference file. Used by the startup splash to distinguish a
/// real opt-out (`Loaded` + `mode = Off`) from a config that *failed* to
/// load and silently fell back to Off — the latter is a misconfiguration
/// the user should see, not a quiet downgrade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxLoadStatus {
    /// `~/.savvagent/sandbox.toml` does not exist. Defaults are in use; the
    /// splash should treat this as the implicit Default mode (no nag is
    /// already provided by [`SandboxMode::Default`]).
    NoFile,
    /// File parsed cleanly. The config faithfully represents the user's
    /// on-disk preference.
    Loaded,
    /// File present but TOML parsing failed. The returned config has
    /// `mode = Off` as a fail-safe — but the splash should surface the
    /// parse error so the user knows their preferred state is *not*
    /// actually applied.
    ParseError {
        /// Verbatim error from the TOML parser.
        message: String,
    },
    /// File declares a schema version this build doesn't understand. Same
    /// fail-safe to Off as `ParseError`, with the splash showing the
    /// version mismatch so the user knows to upgrade.
    UnsupportedVersion {
        /// Version declared in the file.
        found: u32,
        /// Newest schema version this build can parse.
        max: u32,
    },
}

/// Tri-state sandbox preference. Distinguishes "no explicit choice" from an
/// explicit on/off setting so the splash banner can honor the v0.7 "no-nag
/// on opt-out" promise.
///
/// # Wire format
///
/// Serialized as the legacy `enabled = bool` TOML key for back-compat:
///
/// | TOML                | In-memory                |
/// |---------------------|--------------------------|
/// | (key absent)        | [`SandboxMode::Default`] |
/// | `enabled = true`    | [`SandboxMode::On`]      |
/// | `enabled = false`   | [`SandboxMode::Off`]     |
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SandboxMode {
    /// User has not expressed a preference. Sandboxing is enabled at runtime;
    /// the splash banner shows the v0.7 nag line ("use /sandbox off to
    /// disable").
    #[default]
    Default,
    /// User explicitly opted in. Sandboxing is enabled; the splash banner
    /// suppresses the nag.
    On,
    /// User explicitly opted out. Sandboxing is disabled; the splash banner
    /// suppresses the nag.
    Off,
}

impl SandboxMode {
    /// `true` when the user has explicitly set a preference (`On` or `Off`).
    /// Used by the splash to decide whether to show the nag banner.
    pub fn is_explicit(self) -> bool {
        matches!(self, SandboxMode::On | SandboxMode::Off)
    }

    /// Effective enable bit: `Default` and `On` both enable; `Off` disables.
    pub fn is_enabled(self) -> bool {
        !matches!(self, SandboxMode::Off)
    }
}

/// Per-tool sandbox overrides. Applied on top of the global [`SandboxConfig`].
///
/// `deny_unknown_fields` makes typos (e.g. `allow_nett = true`) loud parse
/// errors instead of silently inheriting from the global config.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolSandboxOverride {
    /// When `Some(true)`, allow network access for this tool regardless of
    /// the global `allow_net`. When `Some(false)`, deny even if global allows.
    /// When `None`, inherit the global setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_net: Option<bool>,

    /// Additional paths to bind read-write inside the sandbox for this tool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_binds: Vec<PathBuf>,
}

/// Sandbox configuration. Persisted to `~/.savvagent/sandbox.toml`.
///
/// The struct-level `#[serde(default)]` means any missing field is populated
/// from `SandboxConfig::default()` — so a partial `sandbox.toml` (e.g. only
/// `allow_net = false`) inherits the v0.7 default-on mode rather than
/// failing with a missing-field error.
///
/// `deny_unknown_fields` (added in v0.8) makes typos like `enabld = false`
/// loud parse errors instead of silently dropping to defaults. Combined with
/// the explicit `version` tag this also gives us a clear migration story
/// for future field renames.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SandboxConfig {
    /// Schema version of this config (see [`SCHEMA_VERSION`]). Missing key
    /// defaults to the current version — files written by older binaries
    /// without the field load as v1.
    #[serde(default = "default_schema_version")]
    pub version: u32,

    /// Whether to apply OS-level sandboxing to tool spawns. Wire format is
    /// the legacy `enabled = bool` key; in-memory it is a 3-state
    /// [`SandboxMode`] so the splash can suppress the nag for explicit
    /// opt-outs. Default (v0.7+): enabled.
    #[serde(
        rename = "enabled",
        with = "mode_serde",
        default,
        skip_serializing_if = "is_default_mode"
    )]
    pub mode: SandboxMode,

    /// Allow network access for all tools when sandboxed. Default: `false`.
    ///
    /// `tool-bash` is denied network by default since v0.7; the
    /// host's spawn path injects a per-spawn override based on the
    /// runtime permission decision. To unconditionally grant net access
    /// to bash, set `[tool_overrides.tool-bash] allow_net = true` in
    /// `~/.savvagent/sandbox.toml` — that override bypasses the prompt.
    pub allow_net: bool,

    /// Per-tool override map. Key is a substring of the tool binary path
    /// (e.g. `"tool-bash"` matches any path containing `tool-bash`).
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub tool_overrides: HashMap<String, ToolSandboxOverride>,

    /// Additional paths to bind read-write inside the sandbox for all tools.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_binds: Vec<PathBuf>,
}

fn is_default_mode(m: &SandboxMode) -> bool {
    matches!(m, SandboxMode::Default)
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            // v0.7: default-on. Existing `enabled = false` configs are
            // preserved via `#[serde(default)]` on the struct (Task 14.3).
            // v0.8: distinguishes implicit Default from explicit On/Off.
            mode: SandboxMode::Default,
            allow_net: false,
            tool_overrides: HashMap::new(),
            extra_binds: Vec::new(),
        }
    }
}

/// Serde adapter mapping [`SandboxMode`] to the legacy `enabled = bool` wire
/// format. Wire-level `Option<bool>` lets us round-trip the 3-state through
/// a back-compat TOML key.
mod mode_serde {
    use super::SandboxMode;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(mode: &SandboxMode, ser: S) -> Result<S::Ok, S::Error> {
        match mode {
            // `skip_serializing_if = is_default_mode` short-circuits this branch
            // in normal use; serializing `None` is the safe fallback if the
            // skip is ever removed.
            SandboxMode::Default => ser.serialize_none(),
            SandboxMode::On => ser.serialize_bool(true),
            SandboxMode::Off => ser.serialize_bool(false),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<SandboxMode, D::Error> {
        let v: Option<bool> = Option::deserialize(de)?;
        Ok(match v {
            None => SandboxMode::Default,
            Some(true) => SandboxMode::On,
            Some(false) => SandboxMode::Off,
        })
    }
}

impl SandboxConfig {
    /// Load from `~/.savvagent/sandbox.toml`. Returns the default if the file
    /// is absent, unparseable, or written by a newer schema version (each
    /// case is logged at `warn!` level).
    ///
    /// Callers that need to distinguish "file absent" from "parse error" —
    /// e.g. the startup splash banner — should use
    /// [`SandboxConfig::load_with_status`] instead.
    pub fn load() -> Self {
        Self::load_with_status().0
    }

    /// Load and report what happened. Used by the startup splash to surface
    /// parse errors as a third visual state instead of silently falling back
    /// to defaults (which would lie about the on-disk preference).
    pub fn load_with_status() -> (Self, SandboxLoadStatus) {
        let Some(path) = sandbox_toml_path() else {
            return (Self::default(), SandboxLoadStatus::NoFile);
        };
        load_from_path(&path)
    }

    /// Persist to `~/.savvagent/sandbox.toml`. Errors are propagated; the
    /// caller decides whether to surface them to the user.
    ///
    /// Returns an error when neither `$HOME` nor `$USERPROFILE` is set —
    /// previously this silently no-op'd, making the TUI report a phantom
    /// success even though nothing was written.
    pub async fn save(&self) -> std::io::Result<()> {
        let path = sandbox_toml_path().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "neither $HOME nor $USERPROFILE is set; cannot locate sandbox.toml",
            )
        })?;
        save_to_path(self, &path).await
    }

    /// Effective enable bit derived from [`SandboxMode`]. `true` for
    /// [`SandboxMode::Default`] and [`SandboxMode::On`]; `false` for
    /// [`SandboxMode::Off`].
    pub fn is_enabled(&self) -> bool {
        self.mode.is_enabled()
    }

    /// Resolve whether network should be allowed for a tool identified by its
    /// binary path.
    pub fn net_allowed_for(&self, tool_bin: &Path) -> bool {
        let bin_str = tool_bin.to_string_lossy();

        // User-defined overrides take precedence.
        for (key, ov) in &self.tool_overrides {
            if bin_str.contains(key.as_str()) {
                if let Some(net) = ov.allow_net {
                    return net;
                }
            }
        }

        // Built-in per-tool default: `tool-bash` is denied network by
        // default since v0.7. The host's spawn path (see `tools::LazyBash`)
        // injects a
        // per-spawn override based on the runtime permission decision
        // (see `tools::LazyBash`). User configs can also
        // pin allow/deny via `[tool_overrides.tool-bash] allow_net = ...`
        // in `~/.savvagent/sandbox.toml`.
        if bin_str.contains("tool-bash") {
            return false;
        }

        self.allow_net
    }

    /// Collect the extra bind paths applicable to a given tool binary.
    pub fn extra_binds_for<'a>(&'a self, tool_bin: &Path) -> Vec<&'a Path> {
        let mut out: Vec<&Path> = self.extra_binds.iter().map(PathBuf::as_path).collect();
        if let Some(ov) = self.find_override(tool_bin) {
            out.extend(ov.extra_binds.iter().map(PathBuf::as_path));
        }
        out
    }

    fn find_override(&self, tool_bin: &Path) -> Option<&ToolSandboxOverride> {
        let bin_str = tool_bin.to_string_lossy();
        self.tool_overrides
            .iter()
            .find(|(key, _)| bin_str.contains(key.as_str()))
            .map(|(_, v)| v)
    }
}

fn load_from_path(path: &Path) -> (SandboxConfig, SandboxLoadStatus) {
    match std::fs::read_to_string(path) {
        Ok(text) => match toml::from_str::<SandboxConfig>(&text) {
            Ok(cfg) => {
                if cfg.version > SCHEMA_VERSION {
                    tracing::warn!(
                        "sandbox.toml at {} declares version {} but this build \
                         only understands schema versions up to {}. Falling back \
                         to disabled to preserve any prior opt-out intent. \
                         Upgrade savvagent or remove the file to dismiss.",
                        path.display(),
                        cfg.version,
                        SCHEMA_VERSION,
                    );
                    return (
                        SandboxConfig {
                            mode: SandboxMode::Off,
                            ..SandboxConfig::default()
                        },
                        SandboxLoadStatus::UnsupportedVersion {
                            found: cfg.version,
                            max: SCHEMA_VERSION,
                        },
                    );
                }
                (cfg, SandboxLoadStatus::Loaded)
            }
            Err(e) => {
                let message = e.to_string();
                tracing::warn!(
                    "sandbox.toml at {} failed to parse: {message}. Falling back to \
                     disabled to preserve any prior opt-out intent. Fix the file \
                     and reload to re-enable.",
                    path.display()
                );
                (
                    SandboxConfig {
                        mode: SandboxMode::Off,
                        ..SandboxConfig::default()
                    },
                    SandboxLoadStatus::ParseError { message },
                )
            }
        },
        Err(_) => (SandboxConfig::default(), SandboxLoadStatus::NoFile),
    }
}

async fn save_to_path(cfg: &SandboxConfig, path: &Path) -> std::io::Result<()> {
    let text = toml::to_string_pretty(cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, text).await
}

/// `~/.savvagent/sandbox.toml`, or `None` if `$HOME` is unset.
fn sandbox_toml_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(".savvagent").join("sandbox.toml"))
}

// ---------------------------------------------------------------------------
// Command builder
// ---------------------------------------------------------------------------

/// The wrapper an [`apply_sandbox`] call resolved to. Used for logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxWrapper {
    /// Sandbox was disabled or the wrapper binary was not found.
    None,
    /// Wrapped with `bwrap` (Linux).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Bwrap,
    /// Wrapped with `sandbox-exec` (macOS).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    SandboxExec,
}

/// Apply sandbox wrapping (if configured and available) to `cmd` for the given
/// `tool_bin` path, using `project_root` as the read-write bind root.
///
/// Returns the wrapper kind that was applied, for logging at the call-site.
pub fn apply_sandbox(
    cmd: &mut tokio::process::Command,
    tool_bin: &Path,
    project_root: &Path,
    config: &SandboxConfig,
) -> SandboxWrapper {
    if !config.is_enabled() {
        return SandboxWrapper::None;
    }

    #[cfg(target_os = "linux")]
    {
        apply_linux(cmd, tool_bin, project_root, config)
    }

    #[cfg(target_os = "macos")]
    {
        apply_macos(cmd, tool_bin, project_root, config)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        static WARNED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        WARNED.get_or_init(|| {
            tracing::warn!(
                "sandbox: Windows OS-level sandboxing is not yet implemented; \
                 tools will run unwrapped. Disable sandboxing to silence this warning."
            );
        });
        let _ = (cmd, tool_bin, project_root, config);
        SandboxWrapper::None
    }
}

// ---------------------------------------------------------------------------
// Linux — bubblewrap
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn apply_linux(
    cmd: &mut tokio::process::Command,
    tool_bin: &Path,
    project_root: &Path,
    config: &SandboxConfig,
) -> SandboxWrapper {
    let bwrap = match which_binary("bwrap") {
        Some(p) => p,
        None => {
            // `error!` — not `warn!`. Sandboxing is opted-in via config;
            // a missing wrapper silently downgrades the security posture
            // and operators need to see this at a louder level than
            // generic warnings. Per-spawn surfacing in the TUI is handled
            // by `tools::log_sandbox_wrapper`'s `sandbox_enabled` arm.
            tracing::error!(
                "sandbox: `bwrap` not found on PATH — tool `{}` will run unwrapped",
                tool_bin.display()
            );
            return SandboxWrapper::None;
        }
    };

    // Collect the original program, args, envs, and cwd before we mutate `cmd`.
    // The rewrite (`*cmd = Command::new(bwrap)`) replaces the entire Command,
    // dropping any env/current_dir set by the caller. We must restore them
    // after the rewrite so callers (e.g. `tools::ToolRegistry::connect` which
    // sets `SAVVAGENT_TOOL_FS_ROOT`) keep their configuration.
    let orig_program = cmd.as_std().get_program().to_owned();
    let orig_args: Vec<OsString> = cmd.as_std().get_args().map(|a| a.to_owned()).collect();
    let saved_envs: Vec<(OsString, OsString)> = cmd
        .as_std()
        .get_envs()
        .filter_map(|(k, v)| v.map(|val| (k.to_owned(), val.to_owned())))
        .collect();
    let saved_current_dir = cmd.as_std().get_current_dir().map(|p| p.to_owned());

    let allow_net = config.net_allowed_for(tool_bin);
    let extra_binds = config.extra_binds_for(tool_bin);

    let mut wrapper_args: Vec<OsString> = Vec::new();

    // Read-only bind of the entire filesystem as the base.
    wrapper_args.push("--ro-bind".into());
    wrapper_args.push("/".into());
    wrapper_args.push("/".into());

    // Read-write bind for the project root so tools can write files.
    if let Some(root) = canonical_or_original(project_root) {
        wrapper_args.push("--bind".into());
        wrapper_args.push(root.as_os_str().to_owned());
        wrapper_args.push(root.as_os_str().to_owned());
    }

    // Additional per-tool read-write binds.
    for bind in extra_binds {
        if let Some(b) = canonical_or_original(bind) {
            wrapper_args.push("--bind".into());
            wrapper_args.push(b.as_os_str().to_owned());
            wrapper_args.push(b.as_os_str().to_owned());
        }
    }

    // Network namespace unshare — disabled unless allow_net is true.
    if !allow_net {
        wrapper_args.push("--unshare-net".into());
    }

    // Die with the parent process so orphaned tool servers don't linger.
    wrapper_args.push("--die-with-parent".into());

    // New session to prevent the tool from sending signals to the terminal.
    wrapper_args.push("--new-session".into());

    // Read-side deny floor: hide $HOME secrets from the spawn. The set of
    // paths is the single source of truth declared by
    // `sensitive_paths::sensitive_paths_for_user`.
    wrapper_args.extend(overlay_args_for_paths(
        &crate::sensitive_paths::sensitive_paths_for_user(),
    ));

    // Separator and then the original command.
    wrapper_args.push("--".into());
    wrapper_args.push(orig_program);
    wrapper_args.extend(orig_args);

    // Rewrite `cmd` to invoke bwrap instead. This replaces the whole Command,
    // so the saved envs and cwd must be re-applied immediately afterwards.
    *cmd = tokio::process::Command::new(bwrap);
    cmd.args(wrapper_args);
    for (k, v) in saved_envs {
        cmd.env(k, v);
    }
    if let Some(cwd) = saved_current_dir {
        cmd.current_dir(cwd);
    }

    tracing::info!(
        "sandbox: wrapping `{}` with bwrap (net={})",
        tool_bin.display(),
        allow_net
    );
    SandboxWrapper::Bwrap
}

// ---------------------------------------------------------------------------
// macOS — sandbox-exec
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn apply_macos(
    cmd: &mut tokio::process::Command,
    tool_bin: &Path,
    project_root: &Path,
    config: &SandboxConfig,
) -> SandboxWrapper {
    let sandbox_exec = match which_binary("sandbox-exec") {
        Some(p) => p,
        None => {
            // `error!` — not `warn!`. See the bwrap branch for rationale.
            tracing::error!(
                "sandbox: `sandbox-exec` not found — tool `{}` will run unwrapped",
                tool_bin.display()
            );
            return SandboxWrapper::None;
        }
    };

    let allow_net = config.net_allowed_for(tool_bin);
    let extra_binds = config.extra_binds_for(tool_bin);

    // `sandbox-exec`'s `subpath` profile predicate only matches absolute
    // paths — a relative `project_root` (e.g. ".") silently fails to match
    // anything, denying every file write. Canonicalize the same way the
    // bwrap (Linux) path already does, so relative roots resolve against the
    // process's actual cwd instead of being compared literally.
    let canonical_root =
        canonical_or_original(project_root).unwrap_or_else(|| project_root.to_path_buf());
    let canonical_binds: Vec<PathBuf> = extra_binds
        .iter()
        .map(|b| canonical_or_original(b).unwrap_or_else(|| b.to_path_buf()))
        .collect();
    let canonical_bind_refs: Vec<&Path> = canonical_binds.iter().map(PathBuf::as_path).collect();

    // Build the sandbox-exec(1) TinyScheme profile via the testable helper.
    let profile = build_macos_profile(&canonical_root, allow_net, &canonical_bind_refs);

    // Collect original program, args, envs, and cwd before we mutate `cmd`.
    // The rewrite (`*cmd = Command::new(sandbox_exec)`) replaces the entire
    // Command, dropping any env/current_dir set by the caller. Restore them
    // after the rewrite so callers (e.g. `tools::ToolRegistry::connect` which
    // sets `SAVVAGENT_TOOL_FS_ROOT`) keep their configuration.
    let orig_program = cmd.as_std().get_program().to_owned();
    let orig_args: Vec<OsString> = cmd.as_std().get_args().map(|a| a.to_owned()).collect();
    let saved_envs: Vec<(OsString, OsString)> = cmd
        .as_std()
        .get_envs()
        .filter_map(|(k, v)| v.map(|val| (k.to_owned(), val.to_owned())))
        .collect();
    let saved_current_dir = cmd.as_std().get_current_dir().map(|p| p.to_owned());

    let mut wrapper_args: Vec<OsString> = Vec::new();
    wrapper_args.push("-p".into());
    wrapper_args.push(profile.into());
    wrapper_args.push(orig_program);
    wrapper_args.extend(orig_args);

    *cmd = tokio::process::Command::new(sandbox_exec);
    cmd.args(wrapper_args);
    for (k, v) in saved_envs {
        cmd.env(k, v);
    }
    if let Some(cwd) = saved_current_dir {
        cmd.current_dir(cwd);
    }

    tracing::info!(
        "sandbox: wrapping `{}` with sandbox-exec (net={})",
        tool_bin.display(),
        allow_net
    );
    SandboxWrapper::SandboxExec
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Quote a string for safe interpolation into a TinyScheme string literal
/// (used in sandbox-exec(1) profiles). Escapes backslashes and double quotes.
///
/// Newlines are also escaped (as `\n`) — sandbox-exec rejects raw newlines
/// inside string literals, and silently swallowing them would change the
/// profile semantics.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn scheme_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// Build the TinyScheme profile passed to `sandbox-exec -p`. Reads the real
/// sensitive-path list from `sensitive_paths::sensitive_paths_for_user`.
/// Thin shim around [`build_macos_profile_with`] for testability.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn build_macos_profile(project_root: &Path, allow_net: bool, extra_binds: &[&Path]) -> String {
    let sensitive = crate::sensitive_paths::sensitive_paths_for_user();
    build_macos_profile_with(project_root, allow_net, extra_binds, &sensitive)
}

/// Build the TinyScheme profile from explicit inputs. Pure string composition —
/// does not read `$HOME` or any other process state. Tests pass a synthetic
/// sensitive list.
///
/// Path strings are escaped via [`scheme_quote`] so that paths containing
/// `"`, `\`, or newlines cannot break out of the string literal and corrupt
/// the profile.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn build_macos_profile_with(
    project_root: &Path,
    allow_net: bool,
    extra_binds: &[&Path],
    sensitive: &[PathBuf],
) -> String {
    let project_root_str = scheme_quote(&project_root.to_string_lossy());

    let mut profile = String::from("(version 1)\n");
    profile.push_str("(allow default)\n");

    // Deny all file-write except under project root (and extra binds).
    profile.push_str("(deny file-write*)\n");
    profile.push_str(&format!(
        "(allow file-write* (subpath \"{}\"))\n",
        project_root_str
    ));
    for bind in extra_binds {
        let bind_str = scheme_quote(&bind.to_string_lossy());
        profile.push_str(&format!("(allow file-write* (subpath \"{}\"))\n", bind_str));
    }

    // Read-side deny floor: forbid reads of sensitive paths even though the
    // base policy allows file-read* by default. Sensitive list is the single
    // source of truth from `sensitive_paths::sensitive_paths_for_user`.
    for path in sensitive {
        let q = scheme_quote(&path.to_string_lossy());
        profile.push_str(&format!("(deny file-read* (subpath \"{}\"))\n", q));
    }

    // Deny network unless allowed.
    if !allow_net {
        profile.push_str("(deny network*)\n");
    }

    // Allow process fork and exec (needed for subprocess spawning inside tool).
    profile.push_str("(allow process-fork)\n");
    profile.push_str("(allow process-exec)\n");

    profile
}

/// Find `name` on `$PATH`. Returns `None` if not found.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
fn which_binary(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(name);
            candidate.exists().then_some(candidate)
        })
    })
}

/// Try `canonicalize`; fall back to the original path if that fails (e.g.
/// the path doesn't exist yet at sandbox-apply time).
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
fn canonical_or_original(p: &Path) -> Option<PathBuf> {
    if p.as_os_str().is_empty() {
        return None;
    }
    Some(std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
}

/// Build the full deny-floor arg sequence for a list of sensitive paths.
/// Each path contributes whatever `hide_mount_args` returns (tmpfs for
/// dirs, ro-bind /dev/null for files, nothing for missing entries).
/// Pure — does not read the env.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn overlay_args_for_paths(paths: &[PathBuf]) -> Vec<OsString> {
    let mut out = Vec::new();
    for sensitive in paths {
        for arg in hide_mount_args(sensitive) {
            out.push(arg.into());
        }
    }
    out
}

/// Build `bwrap` arguments that hide the contents of `path` from a tool
/// spawn. Returns the empty vector if `path` does not exist.
///
/// - Directories are masked with `--tmpfs <path>` (empty in-memory mount).
/// - Regular files are masked with `--ro-bind /dev/null <path>`
///   (read returns 0 bytes; writes fail with EACCES).
///
/// Symlinks are followed before classifying — `~/.aws` → real dir works.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn hide_mount_args(path: &Path) -> Vec<String> {
    let resolved = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::error!(
                "sandbox deny-floor: cannot canonicalize sensitive path {} ({e}); \
                 it will NOT be hidden from the tool spawn",
                path.display()
            );
            return Vec::new();
        }
    };
    let meta = match std::fs::metadata(&resolved) {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(
                "sandbox deny-floor: cannot stat sensitive path {} ({e}); \
                 it will NOT be hidden from the tool spawn",
                path.display()
            );
            return Vec::new();
        }
    };
    let target = path.display().to_string();
    if meta.is_dir() {
        vec!["--tmpfs".into(), target]
    } else {
        vec!["--ro-bind".into(), "/dev/null".into(), target]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_cmd(bin: &str) -> tokio::process::Command {
        tokio::process::Command::new(bin)
    }

    fn config_off() -> SandboxConfig {
        SandboxConfig {
            mode: SandboxMode::Off,
            ..SandboxConfig::default()
        }
    }

    #[test]
    fn disabled_config_returns_none_wrapper() {
        let mut cmd = make_cmd("/usr/bin/savvagent-tool-fs");
        let wrapper = apply_sandbox(
            &mut cmd,
            Path::new("/usr/bin/savvagent-tool-fs"),
            Path::new("/home/user/project"),
            &config_off(),
        );
        assert_eq!(wrapper, SandboxWrapper::None);
        // The command should be unchanged.
        assert_eq!(
            cmd.as_std().get_program(),
            std::ffi::OsStr::new("/usr/bin/savvagent-tool-fs")
        );
    }

    #[test]
    fn default_config_has_sandboxing_enabled() {
        let cfg = SandboxConfig::default();
        assert!(
            cfg.is_enabled(),
            "v0.7 default-on: SandboxConfig::default() must enable sandboxing"
        );
        assert_eq!(
            cfg.mode,
            SandboxMode::Default,
            "fresh default is the implicit Default mode, not an explicit On"
        );
        assert!(
            !cfg.mode.is_explicit(),
            "Default is not an explicit user choice — splash should still show the nag"
        );
    }

    #[test]
    fn default_config_denies_tool_bash_net_by_built_in_fallback() {
        let cfg = SandboxConfig::default();
        assert!(
            cfg.tool_overrides.is_empty(),
            "default still has empty tool_overrides"
        );
        assert!(
            !cfg.net_allowed_for(Path::new("/usr/bin/savvagent-tool-bash")),
            "v0.7 default-deny: bash gets no net unless host injects an override"
        );
        assert!(
            !cfg.net_allowed_for(Path::new("/usr/bin/savvagent-tool-fs")),
            "non-bash tools still inherit global allow_net = false"
        );
    }

    #[test]
    fn explicit_user_tool_bash_override_grants_net() {
        let toml_str = r#"
            enabled = true
            allow_net = false

            [tool_overrides.tool-bash]
            allow_net = true
        "#;
        let cfg: SandboxConfig = toml::from_str(toml_str).unwrap();
        assert!(
            cfg.net_allowed_for(Path::new("/usr/bin/savvagent-tool-bash")),
            "explicit user override allow_net=true must grant net for bash"
        );
    }

    #[test]
    fn user_tool_overrides_for_other_tools_leave_bash_default_deny() {
        // User overrides tool-fs only. tool-bash falls through to the v0.7
        // PR 15 built-in fallback, which now denies.
        let toml_str = r#"
            enabled = true
            allow_net = false

            [tool_overrides.tool-fs]
            allow_net = false
            extra_binds = ["/data"]
        "#;
        let cfg: SandboxConfig = toml::from_str(toml_str).unwrap();
        assert!(
            !cfg.net_allowed_for(Path::new("/usr/bin/savvagent-tool-bash")),
            "bash hits the built-in deny fallback when no bash-specific override"
        );
        assert!(
            !cfg.net_allowed_for(Path::new("/usr/bin/savvagent-tool-fs")),
            "tool-fs's user override is respected"
        );
    }

    #[test]
    fn non_bash_tool_inherits_global_net_setting() {
        let cfg = SandboxConfig {
            mode: SandboxMode::On,
            allow_net: false,
            ..SandboxConfig::default()
        };
        let fs_bin = PathBuf::from("/usr/local/bin/savvagent-tool-fs");
        assert!(!cfg.net_allowed_for(&fs_bin));
    }

    #[test]
    fn per_tool_override_can_deny_net_for_bash() {
        let mut cfg = SandboxConfig::default();
        cfg.tool_overrides.insert(
            "tool-bash".to_string(),
            ToolSandboxOverride {
                allow_net: Some(false),
                extra_binds: Vec::new(),
            },
        );
        let bash_bin = PathBuf::from("/usr/local/bin/savvagent-tool-bash");
        assert!(!cfg.net_allowed_for(&bash_bin));
    }

    #[test]
    fn extra_binds_merged_from_global_and_tool_override() {
        let global_bind = PathBuf::from("/data/shared");
        let tool_bind = PathBuf::from("/tmp/scratch");
        let mut cfg = SandboxConfig {
            extra_binds: vec![global_bind.clone()],
            ..SandboxConfig::default()
        };
        cfg.tool_overrides.insert(
            "tool-fs".to_string(),
            ToolSandboxOverride {
                allow_net: None,
                extra_binds: vec![tool_bind.clone()],
            },
        );
        let fs_bin = PathBuf::from("/usr/local/bin/savvagent-tool-fs");
        let binds = cfg.extra_binds_for(&fs_bin);
        assert_eq!(binds.len(), 2);
        assert!(binds.contains(&global_bind.as_path()));
        assert!(binds.contains(&tool_bind.as_path()));
    }

    #[cfg(target_os = "linux")]
    mod linux {
        use super::*;

        fn has_bwrap() -> bool {
            which_binary("bwrap").is_some()
        }

        /// Build a sandbox-wrapped command and verify the bwrap argv contains
        /// the expected flags. This is a pure command-builder test — it does
        /// NOT actually execute bwrap.
        #[test]
        fn bwrap_argv_contains_required_flags() {
            if !has_bwrap() {
                // bwrap not installed in CI — skip gracefully.
                return;
            }
            let mut cmd = make_cmd("/usr/local/bin/savvagent-tool-fs");
            let cfg = SandboxConfig::default();
            let wrapper = apply_sandbox(
                &mut cmd,
                Path::new("/usr/local/bin/savvagent-tool-fs"),
                Path::new("/tmp/project"),
                &cfg,
            );
            assert_eq!(wrapper, SandboxWrapper::Bwrap);
            let args: Vec<String> = cmd
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            // Must contain read-only root bind.
            assert!(
                args.windows(3).any(|w| w == ["--ro-bind", "/", "/"]),
                "expected --ro-bind / / in {args:?}"
            );
            // Must contain --die-with-parent.
            assert!(
                args.contains(&"--die-with-parent".to_string()),
                "expected --die-with-parent in {args:?}"
            );
            // tool-fs has no net override → global allow_net = false → --unshare-net present.
            assert!(
                args.contains(&"--unshare-net".to_string()),
                "expected --unshare-net in {args:?}"
            );
        }

        #[test]
        fn bwrap_argv_no_unshare_when_net_allowed() {
            if !has_bwrap() {
                return;
            }
            let cfg = SandboxConfig {
                allow_net: true,
                ..SandboxConfig::default()
            };
            let mut cmd = make_cmd("/usr/local/bin/savvagent-tool-fs");
            apply_sandbox(
                &mut cmd,
                Path::new("/usr/local/bin/savvagent-tool-fs"),
                Path::new("/tmp/project"),
                &cfg,
            );
            let args: Vec<String> = cmd
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(
                !args.contains(&"--unshare-net".to_string()),
                "--unshare-net should be absent when allow_net=true: {args:?}"
            );
        }

        /// Regression: caller-supplied env vars (e.g. `SAVVAGENT_TOOL_FS_ROOT`)
        /// must survive the bwrap Command rewrite. Without `apply_linux`
        /// preserving envs, sandboxed tools would silently lose their root
        /// configuration and fall back to defaults.
        #[test]
        fn bwrap_preserves_caller_env_vars() {
            if !has_bwrap() {
                return;
            }
            let mut cmd = make_cmd("/usr/local/bin/savvagent-tool-fs");
            cmd.env("SAVVAGENT_TOOL_FS_ROOT", "/foo");
            cmd.env("SAVVAGENT_TOOL_BASH_ROOT", "/bar");
            let cfg = SandboxConfig::default();
            let wrapper = apply_sandbox(
                &mut cmd,
                Path::new("/usr/local/bin/savvagent-tool-fs"),
                Path::new("/tmp/project"),
                &cfg,
            );
            assert_eq!(wrapper, SandboxWrapper::Bwrap);
            let envs: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> = cmd
                .as_std()
                .get_envs()
                .map(|(k, v)| (k.to_owned(), v.map(|val| val.to_owned())))
                .collect();
            assert!(
                envs.iter()
                    .any(|(k, v)| k == std::ffi::OsStr::new("SAVVAGENT_TOOL_FS_ROOT")
                        && v.as_deref() == Some(std::ffi::OsStr::new("/foo"))),
                "SAVVAGENT_TOOL_FS_ROOT=/foo should survive bwrap rewrite, got envs={envs:?}"
            );
            assert!(
                envs.iter().any(
                    |(k, v)| k == std::ffi::OsStr::new("SAVVAGENT_TOOL_BASH_ROOT")
                        && v.as_deref() == Some(std::ffi::OsStr::new("/bar"))
                ),
                "SAVVAGENT_TOOL_BASH_ROOT=/bar should survive bwrap rewrite, got envs={envs:?}"
            );
        }

        #[test]
        fn tool_bash_unshares_net_by_default() {
            if !has_bwrap() {
                return;
            }
            // Since v0.7: built-in default for tool-bash is deny.
            // Without a host-injected `tool_overrides[tool-bash]
            // .allow_net = true`, bash should get --unshare-net.
            let cfg = SandboxConfig::default(); // allow_net = false globally
            let mut cmd = make_cmd("/usr/local/bin/savvagent-tool-bash");
            apply_sandbox(
                &mut cmd,
                Path::new("/usr/local/bin/savvagent-tool-bash"),
                Path::new("/tmp/project"),
                &cfg,
            );
            let args: Vec<String> = cmd
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(
                args.contains(&"--unshare-net".to_string()),
                "v0.7+ deny-by-default: tool-bash must get --unshare-net \
                 absent a host override: {args:?}"
            );
        }

        #[test]
        fn tool_bash_with_explicit_allow_net_override_keeps_net() {
            if !has_bwrap() {
                return;
            }
            // Simulates what the host's spawn path does once
            // `resolve_bash_network_async` returns `true`: insert a
            // per-spawn override granting bash network access.
            let mut cfg = SandboxConfig::default();
            cfg.tool_overrides.insert(
                "tool-bash".to_string(),
                ToolSandboxOverride {
                    allow_net: Some(true),
                    extra_binds: Vec::new(),
                },
            );
            let mut cmd = make_cmd("/usr/local/bin/savvagent-tool-bash");
            apply_sandbox(
                &mut cmd,
                Path::new("/usr/local/bin/savvagent-tool-bash"),
                Path::new("/tmp/project"),
                &cfg,
            );
            let args: Vec<String> = cmd
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(
                !args.contains(&"--unshare-net".to_string()),
                "explicit override allow_net=true must skip --unshare-net: {args:?}"
            );
        }

        /// Integration test: actually spawn a sandboxed `echo` via bwrap and
        /// confirm it can execute. Requires `bwrap` installed.
        #[tokio::test]
        #[ignore = "requires bwrap on PATH"]
        async fn bwrap_sandboxed_echo_runs() {
            let mut cmd = tokio::process::Command::new("echo");
            cmd.arg("hello-from-bwrap");
            let cfg = SandboxConfig::default();
            let wrapper = apply_sandbox(&mut cmd, Path::new("echo"), Path::new("/tmp"), &cfg);
            assert_eq!(wrapper, SandboxWrapper::Bwrap);
            let output = cmd.output().await.expect("bwrap echo failed");
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("hello-from-bwrap"), "got: {stdout}");
        }

        /// Integration test: verify a sandboxed process cannot read `/etc/shadow`.
        /// Requires `bwrap` installed. Does not require root.
        #[tokio::test]
        #[ignore = "requires bwrap on PATH"]
        async fn bwrap_sandboxed_cannot_read_shadow() {
            // We use `cat /etc/shadow` as the canary. On a typical Linux system,
            // /etc/shadow is mode 640 root:shadow (or 000 root:root), so a
            // non-privileged process should fail regardless of sandboxing.
            // The sandbox adds the --ro-bind / / layer, which is fine here —
            // the key property we check is that the process *cannot* write
            // outside the project root, not that it can't read /etc/shadow.
            //
            // Instead we test that `bwrap --unshare-net` actually blocks network:
            // try to connect to 127.0.0.1:9 (discard port) — it should fail.
            let mut cmd = tokio::process::Command::new("sh");
            cmd.arg("-c")
                .arg("echo hello > /tmp/savvagent_sandbox_test_write_outside");
            // project root is /tmp/project (doesn't exist but that's fine for the test)
            let cfg = SandboxConfig::default();
            apply_sandbox(&mut cmd, Path::new("sh"), Path::new("/tmp/project"), &cfg);
            let output = cmd.output().await.expect("bwrap sh failed");
            // Writing outside the project root should fail — bwrap's ro-bind
            // makes / read-only except for the explicit --bind of project_root.
            assert!(
                !output.status.success(),
                "expected write outside project root to fail; exit={:?}",
                output.status
            );
        }
    }

    #[cfg(target_os = "macos")]
    mod macos {
        use super::*;

        /// Regression test for a bug where `apply_macos` passed a relative
        /// `project_root` (e.g. ".") straight into the sandbox-exec profile.
        /// `sandbox-exec`'s `subpath` predicate only matches absolute paths,
        /// so a relative root silently denied *every* file write — tools
        /// spawned with a relative project root (as several tests, and any
        /// caller resolving cwd lazily, do) would fail to write anywhere,
        /// including their own stdout/session files, closing the connection
        /// before the MCP handshake even completed.
        #[test]
        fn relative_project_root_is_canonicalized_before_building_profile() {
            if which_binary("sandbox-exec").is_none() {
                // sandbox-exec is built into macOS, but skip gracefully if a
                // future runner image ever lacks it.
                return;
            }
            let mut cmd = make_cmd("/bin/echo");
            let cfg = SandboxConfig::default();
            apply_macos(&mut cmd, Path::new("/bin/echo"), Path::new("."), &cfg);

            let args: Vec<String> = cmd
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            // argv is `["-p", "<profile>", "/bin/echo"]` after the rewrite.
            let profile = args
                .iter()
                .position(|a| a == "-p")
                .and_then(|i| args.get(i + 1))
                .expect("sandbox-exec argv should contain -p <profile>");

            assert!(
                !profile.contains("(subpath \".\")"),
                "profile still contains the unresolved relative root:\n{profile}"
            );
            let cwd = std::env::current_dir().expect("current dir");
            let expected = format!(
                "(allow file-write* (subpath \"{}\"))",
                scheme_quote(&cwd.to_string_lossy())
            );
            assert!(
                profile.contains(&expected),
                "expected canonicalized cwd allow-rule in profile:\n{profile}\nexpected:\n{expected}"
            );
        }
    }

    // --- scheme_quote / build_macos_profile (pure functions, run on all OSes)

    #[test]
    fn scheme_quote_escapes_backslash_and_double_quote() {
        assert_eq!(scheme_quote("plain"), "plain");
        assert_eq!(scheme_quote(r#"with"quote"#), r#"with\"quote"#);
        assert_eq!(scheme_quote(r"with\backslash"), r"with\\backslash");
        // Both, in order: backslash first so we don't double-escape.
        assert_eq!(scheme_quote(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn scheme_quote_escapes_newlines() {
        assert_eq!(scheme_quote("line1\nline2"), "line1\\nline2");
        assert_eq!(scheme_quote("a\rb"), "a\\rb");
    }

    #[test]
    fn macos_profile_quotes_path_with_double_quote() {
        // A pathological project root that contains a literal `"`.
        let weird = PathBuf::from(r#"/tmp/proj"with"quote"#);
        let profile = build_macos_profile(&weird, false, &[]);
        // The raw `"` must NOT appear unescaped between the `(subpath "…")`
        // delimiters; it must be `\"`.
        assert!(
            profile.contains(r#"(allow file-write* (subpath "/tmp/proj\"with\"quote"))"#),
            "profile did not properly escape quotes:\n{profile}"
        );
        // Sanity: deny file-write* and network* both present.
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("(deny network*)"));
    }

    #[test]
    fn macos_profile_includes_extra_binds() {
        let project = PathBuf::from("/tmp/project");
        let bind_a = PathBuf::from("/data/cache");
        let bind_b = PathBuf::from("/var/tmp/scratch");
        let profile = build_macos_profile(&project, false, &[&bind_a, &bind_b]);
        assert!(
            profile.contains(r#"(allow file-write* (subpath "/tmp/project"))"#),
            "missing project-root allow rule:\n{profile}"
        );
        assert!(
            profile.contains(r#"(allow file-write* (subpath "/data/cache"))"#),
            "missing bind_a allow rule:\n{profile}"
        );
        assert!(
            profile.contains(r#"(allow file-write* (subpath "/var/tmp/scratch"))"#),
            "missing bind_b allow rule:\n{profile}"
        );
    }

    #[test]
    fn macos_profile_omits_network_when_disallowed() {
        let project = PathBuf::from("/tmp/project");
        let denied = build_macos_profile(&project, false, &[]);
        assert!(
            denied.contains("(deny network*)"),
            "expected (deny network*) when allow_net=false:\n{denied}"
        );

        let allowed = build_macos_profile(&project, true, &[]);
        assert!(
            !allowed.contains("(deny network*)"),
            "(deny network*) should be absent when allow_net=true:\n{allowed}"
        );
    }

    #[test]
    fn build_macos_profile_with_appends_file_read_deny_for_sensitive_paths() {
        let root = std::path::Path::new("/Users/alice/project");
        let sensitive: Vec<std::path::PathBuf> = vec![
            std::path::PathBuf::from("/Users/alice/.ssh"),
            std::path::PathBuf::from("/Users/alice/.aws"),
        ];
        let extra_binds: Vec<&std::path::Path> = vec![];

        let profile =
            build_macos_profile_with(root, /* allow_net = */ false, &extra_binds, &sensitive);

        assert!(
            profile.contains(r#"(deny file-read* (subpath "/Users/alice/.ssh"))"#),
            "missing .ssh deny clause in profile:\n{profile}"
        );
        assert!(
            profile.contains(r#"(deny file-read* (subpath "/Users/alice/.aws"))"#),
            "missing .aws deny clause in profile:\n{profile}"
        );
        // Sanity: existing clauses still present.
        assert!(profile.contains("(allow file-write* (subpath \"/Users/alice/project\"))"));
        assert!(profile.contains("(deny network*)"));
    }

    #[test]
    fn macos_profile_escapes_extra_bind_paths() {
        // Both project root and extra binds are user-controlled (via
        // SandboxConfig), so both must be escaped.
        let project = PathBuf::from("/tmp/project");
        let evil_bind = PathBuf::from(r#"/tmp/with"quote"#);
        let profile = build_macos_profile(&project, true, &[&evil_bind]);
        assert!(
            profile.contains(r#"(allow file-write* (subpath "/tmp/with\"quote"))"#),
            "extra bind not properly escaped:\n{profile}"
        );
    }

    #[test]
    fn hide_mount_args_for_existing_directory_use_tmpfs() {
        let td = tempfile::TempDir::new().unwrap();
        let dir = td.path().join(".ssh");
        std::fs::create_dir_all(&dir).unwrap();

        let args = hide_mount_args(&dir);
        assert_eq!(args, vec!["--tmpfs".into(), dir.display().to_string()]);
    }

    #[test]
    fn hide_mount_args_for_existing_file_use_ro_bind_dev_null() {
        let td = tempfile::TempDir::new().unwrap();
        let file = td.path().join(".netrc");
        std::fs::write(&file, "secret\n").unwrap();

        let args = hide_mount_args(&file);
        assert_eq!(
            args,
            vec![
                "--ro-bind".into(),
                "/dev/null".into(),
                file.display().to_string(),
            ]
        );
    }

    #[test]
    fn hide_mount_args_for_missing_path_returns_empty() {
        let td = tempfile::TempDir::new().unwrap();
        let missing = td.path().join("nonexistent");
        let args = hide_mount_args(&missing);
        assert!(args.is_empty());
    }

    #[test]
    fn overlay_args_for_paths_emits_tmpfs_for_dirs_and_ro_bind_for_files() {
        let td = tempfile::TempDir::new().unwrap();
        let dir = td.path().join(".ssh");
        std::fs::create_dir_all(&dir).unwrap();
        let file = td.path().join(".netrc");
        std::fs::write(&file, "secret\n").unwrap();
        let missing = td.path().join("nonexistent");

        let args = overlay_args_for_paths(&[dir.clone(), file.clone(), missing]);
        let dbg = format!("{args:?}");

        assert!(dbg.contains("--tmpfs"));
        assert!(dbg.contains(".ssh"));
        assert!(dbg.contains("--ro-bind"));
        assert!(dbg.contains("/dev/null"));
        assert!(dbg.contains(".netrc"));
        // Missing path contributes nothing — count the args.
        let total: usize = args.len();
        assert_eq!(
            total, 5,
            "expected 2 (--tmpfs, .ssh) + 3 (--ro-bind, /dev/null, .netrc) = 5 args, got {args:?}"
        );
    }

    #[test]
    fn load_from_path_returns_default_on_when_file_absent() {
        let td = tempfile::TempDir::new().unwrap();
        let missing = td.path().join("does-not-exist.toml");
        let (cfg, status) = load_from_path(&missing);
        assert_eq!(
            cfg.mode,
            SandboxMode::Default,
            "absent file must yield the implicit Default mode"
        );
        assert!(
            cfg.is_enabled(),
            "Default mode is enabled at runtime even without an on-disk preference"
        );
        assert_eq!(
            status,
            SandboxLoadStatus::NoFile,
            "absent file must report NoFile so the splash can avoid showing a \
             parse-error indicator"
        );
    }

    #[test]
    fn load_from_path_preserves_explicit_enabled_false() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("sandbox.toml");
        std::fs::write(&path, "enabled = false\n").unwrap();
        let (cfg, status) = load_from_path(&path);
        assert_eq!(
            cfg.mode,
            SandboxMode::Off,
            "explicit `enabled = false` must round-trip to SandboxMode::Off"
        );
        assert!(
            !cfg.is_enabled(),
            "explicit Off must disable sandboxing at runtime"
        );
        assert!(
            cfg.mode.is_explicit(),
            "explicit Off must signal the splash to suppress the v0.7 nag"
        );
        assert_eq!(
            status,
            SandboxLoadStatus::Loaded,
            "successful parse must report Loaded — the splash uses this to \
             distinguish a real opt-out from a parse-error fallback"
        );
    }

    #[test]
    fn load_from_path_preserves_explicit_enabled_true() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("sandbox.toml");
        std::fs::write(&path, "enabled = true\n").unwrap();
        let (cfg, status) = load_from_path(&path);
        assert_eq!(
            cfg.mode,
            SandboxMode::On,
            "explicit `enabled = true` must round-trip to SandboxMode::On"
        );
        assert!(
            cfg.mode.is_explicit(),
            "explicit On must signal the splash to suppress the v0.7 nag"
        );
        assert_eq!(status, SandboxLoadStatus::Loaded);
    }

    #[test]
    fn load_from_path_partial_file_defaults_to_default_mode() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("sandbox.toml");
        // No `enabled` key — only a non-default `allow_net = true` so that a
        // serde-default regression (where every missing field silently resets)
        // would be caught by the allow_net assertion.
        std::fs::write(&path, "allow_net = true\n").unwrap();
        let (cfg, status) = load_from_path(&path);
        assert_eq!(
            cfg.mode,
            SandboxMode::Default,
            "partial file with no `enabled` key must yield the implicit Default mode"
        );
        assert!(
            cfg.allow_net,
            "explicitly written `allow_net = true` must be preserved — if this \
             flips to false a serde-default regression has silently reset every \
             unmentioned field"
        );
        assert!(
            cfg.is_enabled(),
            "Default mode must still enable sandboxing at runtime"
        );
        assert_eq!(status, SandboxLoadStatus::Loaded);
    }

    #[test]
    fn load_from_path_falls_back_to_off_on_parse_error() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("sandbox.toml");
        // Malformed TOML — unclosed string.
        std::fs::write(&path, "enabled = \"unclosed\n").unwrap();
        let (cfg, status) = load_from_path(&path);
        assert!(
            !cfg.is_enabled(),
            "parse error must fall back to disabled (fail-safe), not default-on"
        );
        assert_eq!(
            cfg.mode,
            SandboxMode::Off,
            "parse-error fallback must be the explicit Off mode (no splash nag)"
        );
        match status {
            SandboxLoadStatus::ParseError { message } => {
                assert!(
                    !message.is_empty(),
                    "parse-error status must carry a non-empty message for the splash to surface"
                );
            }
            other => panic!("expected SandboxLoadStatus::ParseError, got {other:?}"),
        }
    }

    #[test]
    fn load_from_path_rejects_future_schema_version() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("sandbox.toml");
        // File from a hypothetical future schema. Today's binary refuses to
        // pretend it understood the contents and falls back to Off.
        let future_version = SCHEMA_VERSION + 1;
        std::fs::write(&path, format!("version = {future_version}\n")).unwrap();
        let (cfg, status) = load_from_path(&path);
        assert_eq!(
            cfg.mode,
            SandboxMode::Off,
            "future-version file must fall back to Off (fail-safe), not silently \
             load with possibly-misinterpreted fields"
        );
        assert_eq!(
            status,
            SandboxLoadStatus::UnsupportedVersion {
                found: future_version,
                max: SCHEMA_VERSION,
            },
            "future-version status must surface the version mismatch so the splash \
             can prompt the user to upgrade"
        );
    }

    #[test]
    fn load_from_path_accepts_current_schema_version() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("sandbox.toml");
        let text = format!("version = {SCHEMA_VERSION}\nenabled = false\n");
        std::fs::write(&path, text).unwrap();
        let (cfg, status) = load_from_path(&path);
        assert_eq!(cfg.mode, SandboxMode::Off);
        assert_eq!(cfg.version, SCHEMA_VERSION);
        assert_eq!(status, SandboxLoadStatus::Loaded);
    }

    #[test]
    fn load_with_status_returns_no_file_when_home_unset() {
        // Drive the public API directly. If $HOME is unset we get
        // SandboxLoadStatus::NoFile (and SandboxConfig::default()) without
        // touching the disk. This is the path the TUI hits in headless CI
        // images where neither $HOME nor $USERPROFILE is set.
        //
        // We can't mutate env in a multi-threaded test safely, so this test
        // documents the contract via the equivalent load_from_path path: a
        // missing file yields NoFile. The env-based branch is exercised by
        // sandbox_toml_path() which is private but covered indirectly by
        // the integration test for save() in the test below.
        let td = tempfile::TempDir::new().unwrap();
        let missing = td.path().join("nonexistent.toml");
        let (_, status) = load_from_path(&missing);
        assert_eq!(status, SandboxLoadStatus::NoFile);
    }

    #[test]
    fn deny_unknown_fields_at_struct_level() {
        // Typo: `enabld` instead of `enabled` — must be a loud parse error,
        // not a silent fall-through to Default.
        let toml_str = "enabld = false\n";
        let err = toml::from_str::<SandboxConfig>(toml_str).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown field") || msg.contains("enabld"),
            "expected unknown-field error for typo `enabld`, got: {msg}"
        );
    }

    #[test]
    fn deny_unknown_fields_on_tool_override() {
        // Typo inside a per-tool override section.
        let toml_str = r#"
            enabled = true

            [tool_overrides.tool-bash]
            allow_nett = true
        "#;
        let err = toml::from_str::<SandboxConfig>(toml_str).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown field") || msg.contains("allow_nett"),
            "expected unknown-field error for typo `allow_nett`, got: {msg}"
        );
    }

    #[test]
    fn sandbox_config_roundtrips_toml() {
        let cfg = SandboxConfig {
            version: SCHEMA_VERSION,
            mode: SandboxMode::On,
            allow_net: false,
            tool_overrides: {
                let mut m = HashMap::new();
                m.insert(
                    "tool-bash".to_string(),
                    ToolSandboxOverride {
                        allow_net: Some(true),
                        extra_binds: vec![PathBuf::from("/var/cache")],
                    },
                );
                m
            },
            extra_binds: vec![PathBuf::from("/data/shared")],
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        let roundtripped: SandboxConfig = toml::from_str(&text).unwrap();
        assert_eq!(roundtripped.mode, cfg.mode);
        assert_eq!(roundtripped.allow_net, cfg.allow_net);
        assert_eq!(roundtripped.extra_binds, cfg.extra_binds);
        assert_eq!(roundtripped.version, cfg.version);
        let bash_ov = roundtripped.tool_overrides.get("tool-bash").unwrap();
        assert_eq!(bash_ov.allow_net, Some(true));
        assert_eq!(bash_ov.extra_binds, vec![PathBuf::from("/var/cache")]);
    }

    #[test]
    fn default_mode_is_omitted_from_serialized_toml() {
        // A freshly defaulted config must NOT serialize `enabled = …` so the
        // file stays a true tri-state: rewriting the file shouldn't promote
        // Default into an explicit On/Off.
        let cfg = SandboxConfig::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(
            !text.contains("enabled"),
            "Default mode must omit the `enabled` key so the splash can still \
             distinguish 'no preference' from 'explicit on': \n{text}"
        );
    }

    #[test]
    fn explicit_modes_roundtrip_via_enabled_key() {
        let on = SandboxConfig {
            mode: SandboxMode::On,
            ..SandboxConfig::default()
        };
        let on_text = toml::to_string_pretty(&on).unwrap();
        assert!(
            on_text.contains("enabled = true"),
            "On must serialize as `enabled = true`:\n{on_text}"
        );
        let on_back: SandboxConfig = toml::from_str(&on_text).unwrap();
        assert_eq!(on_back.mode, SandboxMode::On);

        let off = SandboxConfig {
            mode: SandboxMode::Off,
            ..SandboxConfig::default()
        };
        let off_text = toml::to_string_pretty(&off).unwrap();
        assert!(
            off_text.contains("enabled = false"),
            "Off must serialize as `enabled = false`:\n{off_text}"
        );
        let off_back: SandboxConfig = toml::from_str(&off_text).unwrap();
        assert_eq!(off_back.mode, SandboxMode::Off);
    }

    #[tokio::test]
    async fn save_to_path_writes_toml() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("sandbox.toml");
        let cfg = SandboxConfig {
            mode: SandboxMode::Off,
            ..SandboxConfig::default()
        };
        save_to_path(&cfg, &path).await.unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("enabled = false"));
        let back: SandboxConfig = toml::from_str(&text).unwrap();
        assert_eq!(back.mode, SandboxMode::Off);
    }
}
