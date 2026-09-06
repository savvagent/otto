//! `internal:home-footer` — sandbox state + turn state + working dir + key reminder.
//!
//! Contributes to slots `home.footer.center` and `home.footer.right`.
//! Subscribes to `TurnStart` / `TurnEnd` for the turn-state span (PR 7
//! wires the host to actually emit those events; until then the plugin
//! shows the idle state) and `ContextSizeChanged` for the `~N ctx`
//! segment on the right slot.

use async_trait::async_trait;
use savvagent_plugin::{
    Contributions, Effect, HookKind, HostEvent, Manifest, Plugin, PluginError, PluginId,
    PluginKind, Region, SlotSpec, StyledLine, StyledSpan, TextMods, ThemeColor,
};

/// TUI home-screen footer plugin.
///
/// Renders sandbox + turn state in `home.footer.center` and the working
/// directory + context-size + cost + version in `home.footer.right`.
/// Tracks the active turn by listening to `TurnStart` / `TurnEnd` host
/// events, and the rough conversation context-size estimate by listening
/// to `ContextSizeChanged`.
pub struct HomeFooterPlugin {
    turn_active: Option<u32>,
    working_dir: String,
    context_tokens: u32,
}

impl HomeFooterPlugin {
    /// Construct a new `HomeFooterPlugin`, snapshotting `$PWD` at creation time.
    ///
    /// Falls back to `"?"` if the working directory cannot be determined.
    pub fn new() -> Self {
        let wd = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "?".into());
        Self {
            turn_active: None,
            working_dir: wd,
            context_tokens: 0,
        }
    }
}

impl Default for HomeFooterPlugin {
    fn default() -> Self {
        Self::new()
    }
}

/// Format the context-size estimate for the footer:
/// - 0 returns `None` so the segment is omitted entirely
/// - `< 1000` renders as `~123 ctx`
/// - `>= 1000` rounds to the nearest thousand and renders as `~12k ctx`
fn format_context_segment(tokens: u32) -> Option<String> {
    if tokens == 0 {
        return None;
    }
    if tokens < 1000 {
        Some(rust_i18n::t!("footer.ctx-tokens", count = tokens).to_string())
    } else {
        // Round-half-up to the nearest thousand.
        let k = (tokens + 500) / 1000;
        Some(rust_i18n::t!("footer.ctx-kilo", k = k).to_string())
    }
}

#[async_trait]
impl Plugin for HomeFooterPlugin {
    fn manifest(&self) -> Manifest {
        // Contributions is #[non_exhaustive] — build via field mutation
        // from default rather than struct-literal FRU syntax.
        let mut contributions = Contributions::default();
        contributions.slots = vec![
            SlotSpec {
                slot_id: "home.footer.center".into(),
                priority: 100,
            },
            SlotSpec {
                slot_id: "home.footer.right".into(),
                priority: 100,
            },
        ];
        contributions.hooks = vec![
            HookKind::TurnStart,
            HookKind::TurnEnd,
            HookKind::ContextSizeChanged,
        ];

        Manifest {
            id: PluginId::new("internal:home-footer").expect("valid built-in id"),
            name: "Home footer".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: rust_i18n::t!("plugin.home-footer-description").to_string(),
            kind: PluginKind::Core,
            contributions,
        }
    }

    fn render_slot(&self, slot_id: &str, _region: Region) -> Vec<StyledLine> {
        match slot_id {
            "home.footer.center" => {
                let turn = match self.turn_active {
                    Some(id) => rust_i18n::t!("footer.turn-working", id = id).to_string(),
                    None => rust_i18n::t!("footer.idle").to_string(),
                };
                vec![StyledLine {
                    spans: vec![StyledSpan {
                        text: turn,
                        fg: Some(ThemeColor::Accent),
                        bg: None,
                        modifiers: TextMods::default(),
                    }],
                }]
            }
            "home.footer.right" => {
                // Layout: working_dir · ~N ctx · $0.00 · vX.Y.Z
                //
                // - working_dir / labels / dividers use ThemeColor::Muted
                // - version uses ThemeColor::Accent
                // - $0.00 is a literal placeholder until real cost tracking
                //   ships.
                // TODO(v0.10): wire real cost via TurnOutcome.usage + per-model pricing table
                let muted = |text: String| StyledSpan {
                    text,
                    fg: Some(ThemeColor::Muted),
                    bg: None,
                    modifiers: TextMods::default(),
                };
                let accent = |text: String| StyledSpan {
                    text,
                    fg: Some(ThemeColor::Accent),
                    bg: None,
                    modifiers: TextMods::default(),
                };

                let mut spans: Vec<StyledSpan> = Vec::with_capacity(7);
                spans.push(muted(self.working_dir.clone()));
                if let Some(ctx_text) = format_context_segment(self.context_tokens) {
                    spans.push(muted(" · ".into()));
                    spans.push(muted(ctx_text));
                }
                spans.push(muted(" · ".into()));
                spans.push(muted(rust_i18n::t!("footer.cost-zero").to_string()));
                spans.push(muted(" · ".into()));
                spans.push(accent(format!("v{}", env!("CARGO_PKG_VERSION"))));

                vec![StyledLine { spans }]
            }
            _ => vec![],
        }
    }

    async fn on_event(&mut self, event: HostEvent) -> Result<Vec<Effect>, PluginError> {
        match event {
            HostEvent::TurnStart { turn_id } => self.turn_active = Some(turn_id),
            HostEvent::TurnEnd { .. } => self.turn_active = None,
            HostEvent::ContextSizeChanged { tokens } => self.context_tokens = tokens,
            _ => {}
        }
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_center_renders_idle() {
        let p = HomeFooterPlugin::new();
        let lines = p.render_slot(
            "home.footer.center",
            Region {
                x: 0,
                y: 0,
                width: 40,
                height: 1,
            },
        );
        assert_eq!(
            lines[0].spans[0].text,
            rust_i18n::t!("footer.idle").as_ref()
        );
    }

    #[tokio::test]
    async fn turn_start_flips_to_working() {
        let mut p = HomeFooterPlugin::new();
        p.on_event(HostEvent::TurnStart { turn_id: 3 })
            .await
            .unwrap();
        let lines = p.render_slot(
            "home.footer.center",
            Region {
                x: 0,
                y: 0,
                width: 40,
                height: 1,
            },
        );
        assert_eq!(
            lines[0].spans[0].text,
            rust_i18n::t!("footer.turn-working", id = 3u32).as_ref()
        );
    }

    #[tokio::test]
    async fn turn_end_returns_to_idle() {
        let mut p = HomeFooterPlugin::new();
        p.on_event(HostEvent::TurnStart { turn_id: 3 })
            .await
            .unwrap();
        p.on_event(HostEvent::TurnEnd {
            turn_id: 3,
            success: true,
        })
        .await
        .unwrap();
        let lines = p.render_slot(
            "home.footer.center",
            Region {
                x: 0,
                y: 0,
                width: 40,
                height: 1,
            },
        );
        assert_eq!(
            lines[0].spans[0].text,
            rust_i18n::t!("footer.idle").as_ref()
        );
    }

    #[test]
    fn right_slot_includes_working_dir_version_and_cost() {
        let p = HomeFooterPlugin::new();
        let lines = p.render_slot(
            "home.footer.right",
            Region {
                x: 0,
                y: 0,
                width: 80,
                height: 1,
            },
        );
        let joined: String = lines[0].spans.iter().map(|s| s.text.clone()).collect();
        assert!(
            joined.contains(&p.working_dir),
            "expected working dir in: {joined}"
        );
        assert!(
            joined.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))),
            "expected version literal in: {joined}"
        );
        assert!(
            joined.contains(rust_i18n::t!("footer.cost-zero").as_ref()),
            "expected cost-zero in: {joined}"
        );
        assert!(
            !joined.contains("? for help"),
            "stale hint still present in: {joined}"
        );
    }

    #[tokio::test]
    async fn context_size_event_updates_token_segment() {
        let mut p = HomeFooterPlugin::new();
        p.on_event(HostEvent::ContextSizeChanged { tokens: 1234 })
            .await
            .unwrap();
        let lines = p.render_slot(
            "home.footer.right",
            Region {
                x: 0,
                y: 0,
                width: 80,
                height: 1,
            },
        );
        let joined: String = lines[0].spans.iter().map(|s| s.text.clone()).collect();
        assert!(
            joined.contains("~1k ctx") || joined.contains("~1234 ctx"),
            "expected token segment, got: {joined}"
        );
    }
}
