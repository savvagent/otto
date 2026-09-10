//! `internal:tool-web-summary` — renders summaries for `tool-web`'s
//! `web_fetch` and `web_search`.

use async_trait::async_trait;
use otto_plugin::{
    Contributions, Manifest, Plugin, PluginId, PluginKind, StyledSpan, ThemeColor, ToolSummarySpec,
};
use tool_web::{FetchInput, FetchOutput, SearchInput, SearchOutput};

/// Plugin rendering summaries for the `tool-web` `web_fetch` and
/// `web_search` tools.
pub struct ToolWebSummaryPlugin;

impl ToolWebSummaryPlugin {
    /// Construct a new instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ToolWebSummaryPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ToolWebSummaryPlugin {
    fn manifest(&self) -> Manifest {
        let mut contributions = Contributions::default();
        contributions.tool_summaries = vec![
            ToolSummarySpec {
                tool_name: "web_fetch".into(),
            },
            ToolSummarySpec {
                tool_name: "web_search".into(),
            },
        ];
        Manifest {
            id: PluginId::new("internal:tool-web-summary").expect("valid built-in id"),
            name: "tool-web summaries".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Renders conversation-log summaries for web_fetch and web_search".into(),
            kind: PluginKind::Core,
            contributions,
        }
    }

    fn summarize_tool_call(&self, name: &str, args: &serde_json::Value) -> Option<Vec<StyledSpan>> {
        match name {
            "web_fetch" => {
                let input: FetchInput = serde_json::from_value(args.clone()).ok()?;
                Some(vec![
                    StyledSpan::colored("fetch ", ThemeColor::Fg),
                    StyledSpan::colored(input.url, ThemeColor::Success),
                ])
            }
            "web_search" => {
                let input: SearchInput = serde_json::from_value(args.clone()).ok()?;
                Some(vec![
                    StyledSpan::colored("search '", ThemeColor::Fg),
                    StyledSpan::colored(input.query, ThemeColor::Success),
                    StyledSpan::colored("'", ThemeColor::Fg),
                ])
            }
            _ => None,
        }
    }

    fn summarize_tool_result(&self, name: &str, result_text: &str) -> Option<Vec<StyledSpan>> {
        match name {
            "web_fetch" => {
                let out: FetchOutput = serde_json::from_str(result_text).ok()?;
                let mut spans = vec![
                    StyledSpan::colored(out.status.to_string(), ThemeColor::Success),
                    StyledSpan::colored(" · ", ThemeColor::Muted),
                    StyledSpan::colored(out.content.len().to_string(), ThemeColor::Success),
                    StyledSpan::colored(" chars", ThemeColor::Fg),
                ];
                if out.truncated {
                    spans.push(StyledSpan::colored(" (truncated)", ThemeColor::Muted));
                }
                Some(spans)
            }
            "web_search" => {
                let out: SearchOutput = serde_json::from_str(result_text).ok()?;
                Some(vec![
                    StyledSpan::colored(out.results.len().to_string(), ThemeColor::Success),
                    StyledSpan::colored(" results via ", ThemeColor::Fg),
                    StyledSpan::colored(out.backend, ThemeColor::Muted),
                ])
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(spans: &[StyledSpan]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn manifest_claims_both_tools() {
        let m = ToolWebSummaryPlugin::new().manifest();
        assert_eq!(m.contributions.tool_summaries.len(), 2);
        assert_eq!(m.contributions.tool_summaries[0].tool_name, "web_fetch");
        assert_eq!(m.contributions.tool_summaries[1].tool_name, "web_search");
    }

    #[test]
    fn fetch_call_renders_url() {
        let p = ToolWebSummaryPlugin::new();
        let spans = p
            .summarize_tool_call(
                "web_fetch",
                &serde_json::json!({"url": "https://example.com"}),
            )
            .unwrap();
        assert_eq!(join(&spans), "fetch https://example.com");
    }

    #[test]
    fn search_call_renders_query() {
        let p = ToolWebSummaryPlugin::new();
        let spans = p
            .summarize_tool_call("web_search", &serde_json::json!({"query": "rust async"}))
            .unwrap();
        assert_eq!(join(&spans), "search 'rust async'");
    }

    #[test]
    fn fetch_result_shows_status_and_length() {
        let p = ToolWebSummaryPlugin::new();
        let result = serde_json::json!({
            "url": "https://example.com",
            "status": 200,
            "content_type": "text/html",
            "content": "hello",
            "truncated": false
        })
        .to_string();
        let spans = p.summarize_tool_result("web_fetch", &result).unwrap();
        assert_eq!(join(&spans), "200 · 5 chars");
    }

    #[test]
    fn fetch_result_shows_truncated_flag() {
        let p = ToolWebSummaryPlugin::new();
        let result = serde_json::json!({
            "url": "https://example.com",
            "status": 200,
            "content_type": null,
            "content": "hello",
            "truncated": true
        })
        .to_string();
        let spans = p.summarize_tool_result("web_fetch", &result).unwrap();
        assert_eq!(join(&spans), "200 · 5 chars (truncated)");
    }

    #[test]
    fn search_result_shows_count_and_backend() {
        let p = ToolWebSummaryPlugin::new();
        let result = serde_json::json!({
            "backend": "brave",
            "results": [
                {"title": "A", "url": "https://a.example", "snippet": ""},
                {"title": "B", "url": "https://b.example", "snippet": ""}
            ]
        })
        .to_string();
        let spans = p.summarize_tool_result("web_search", &result).unwrap();
        assert_eq!(join(&spans), "2 results via brave");
    }

    #[test]
    fn returns_none_for_unknown_tool() {
        let p = ToolWebSummaryPlugin::new();
        assert!(
            p.summarize_tool_call("read_file", &serde_json::json!({}))
                .is_none()
        );
    }

    #[test]
    fn returns_none_on_args_parse_failure() {
        let p = ToolWebSummaryPlugin::new();
        assert!(
            p.summarize_tool_call("web_fetch", &serde_json::json!({}))
                .is_none()
        );
    }

    // Pins span colours so the #117 `StyledSpan::colored()` -> `StyledSpan::colored()` constructor
    // rewrite cannot change them silently.
    #[test]
    fn fetch_call_span_colors_are_pinned() {
        let p = ToolWebSummaryPlugin::new();
        let spans = p
            .summarize_tool_call(
                "web_fetch",
                &serde_json::json!({"url": "https://example.com"}),
            )
            .unwrap();
        let pairs: Vec<(&str, Option<ThemeColor>)> =
            spans.iter().map(|s| (s.text.as_str(), s.fg)).collect();
        assert_eq!(
            pairs,
            vec![
                ("fetch ", Some(ThemeColor::Fg)),
                ("https://example.com", Some(ThemeColor::Success)),
            ]
        );
    }

    #[test]
    fn search_call_span_colors_are_pinned() {
        let p = ToolWebSummaryPlugin::new();
        let spans = p
            .summarize_tool_call("web_search", &serde_json::json!({"query": "rust async"}))
            .unwrap();
        let pairs: Vec<(&str, Option<ThemeColor>)> =
            spans.iter().map(|s| (s.text.as_str(), s.fg)).collect();
        assert_eq!(
            pairs,
            vec![
                ("search '", Some(ThemeColor::Fg)),
                ("rust async", Some(ThemeColor::Success)),
                ("'", Some(ThemeColor::Fg)),
            ]
        );
    }

    #[test]
    fn fetch_result_span_colors_are_pinned() {
        let p = ToolWebSummaryPlugin::new();
        let result = serde_json::json!({
            "url": "https://example.com",
            "status": 200,
            "content_type": null,
            "content": "hello",
            "truncated": true
        })
        .to_string();
        let spans = p.summarize_tool_result("web_fetch", &result).unwrap();
        let pairs: Vec<(&str, Option<ThemeColor>)> =
            spans.iter().map(|s| (s.text.as_str(), s.fg)).collect();
        assert_eq!(
            pairs,
            vec![
                ("200", Some(ThemeColor::Success)),
                (" · ", Some(ThemeColor::Muted)),
                ("5", Some(ThemeColor::Success)),
                (" chars", Some(ThemeColor::Fg)),
                (" (truncated)", Some(ThemeColor::Muted)),
            ]
        );
    }

    #[test]
    fn search_result_span_colors_are_pinned() {
        let p = ToolWebSummaryPlugin::new();
        let result = serde_json::json!({
            "backend": "brave",
            "results": [
                {"title": "A", "url": "https://a.example", "snippet": ""},
                {"title": "B", "url": "https://b.example", "snippet": ""}
            ]
        })
        .to_string();
        let spans = p.summarize_tool_result("web_search", &result).unwrap();
        let pairs: Vec<(&str, Option<ThemeColor>)> =
            spans.iter().map(|s| (s.text.as_str(), s.fg)).collect();
        assert_eq!(
            pairs,
            vec![
                ("2", Some(ThemeColor::Success)),
                (" results via ", Some(ThemeColor::Fg)),
                ("brave", Some(ThemeColor::Muted)),
            ]
        );
    }
}
