//! `internal:tool-grep-summary` — renders summaries for `tool-grep`'s `search`.

use std::collections::HashSet;

use async_trait::async_trait;
use otto_plugin::{
    Contributions, Manifest, Plugin, PluginId, PluginKind, StyledSpan, ThemeColor, ToolSummarySpec,
};
use tool_grep::{SearchInput, SearchOutput};

/// Plugin rendering summaries for the `tool-grep` `search` tool.
pub struct ToolGrepSummaryPlugin;

impl ToolGrepSummaryPlugin {
    /// Construct a new instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ToolGrepSummaryPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ToolGrepSummaryPlugin {
    fn manifest(&self) -> Manifest {
        let mut contributions = Contributions::default();
        contributions.tool_summaries = vec![ToolSummarySpec {
            tool_name: "search".into(),
        }];
        Manifest {
            id: PluginId::new("internal:tool-grep-summary").expect("valid built-in id"),
            name: "tool-grep summaries".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Renders conversation-log summaries for the tool-grep search command"
                .into(),
            kind: PluginKind::Core,
            contributions,
        }
    }

    fn summarize_tool_call(&self, name: &str, args: &serde_json::Value) -> Option<Vec<StyledSpan>> {
        if name != "search" {
            return None;
        }
        let input: SearchInput = serde_json::from_value(args.clone()).ok()?;
        let mut spans = vec![
            StyledSpan::colored("grep '", ThemeColor::Fg),
            StyledSpan::colored(input.pattern, ThemeColor::Success),
            StyledSpan::colored("'", ThemeColor::Fg),
        ];
        if let Some(path) = input.path {
            spans.push(StyledSpan::colored(
                format!(" in {path}"),
                ThemeColor::Muted,
            ));
        }
        if input.case_insensitive {
            spans.push(StyledSpan::colored(" -i", ThemeColor::Muted));
        }
        if input.multiline {
            spans.push(StyledSpan::colored(" --multiline", ThemeColor::Muted));
        }
        Some(spans)
    }

    fn summarize_tool_result(&self, name: &str, result_text: &str) -> Option<Vec<StyledSpan>> {
        if name != "search" {
            return None;
        }
        let out: SearchOutput = serde_json::from_str(result_text).ok()?;
        let unique_files: HashSet<&str> = out.matches.iter().map(|m| m.file.as_str()).collect();
        let mut spans = vec![
            StyledSpan::colored(out.matches.len().to_string(), ThemeColor::Success),
            StyledSpan::colored(" matches in ", ThemeColor::Fg),
            StyledSpan::colored(unique_files.len().to_string(), ThemeColor::Success),
            StyledSpan::colored(" files", ThemeColor::Fg),
        ];
        if out.truncated {
            spans.push(StyledSpan::colored(" (truncated)", ThemeColor::Muted));
        }
        Some(spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(spans: &[StyledSpan]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn manifest_claims_search() {
        let m = ToolGrepSummaryPlugin::new().manifest();
        assert_eq!(m.contributions.tool_summaries.len(), 1);
        assert_eq!(m.contributions.tool_summaries[0].tool_name, "search");
    }

    #[test]
    fn search_call_renders_pattern() {
        let p = ToolGrepSummaryPlugin::new();
        let spans = p
            .summarize_tool_call("search", &serde_json::json!({"pattern": "TODO"}))
            .unwrap();
        assert_eq!(join(&spans), "grep 'TODO'");
    }

    #[test]
    fn search_call_renders_pattern_and_path_and_flags() {
        let p = ToolGrepSummaryPlugin::new();
        let spans = p
            .summarize_tool_call(
                "search",
                &serde_json::json!({
                    "pattern": "fn ",
                    "path": "src",
                    "case_insensitive": true,
                    "multiline": false
                }),
            )
            .unwrap();
        assert_eq!(join(&spans), "grep 'fn ' in src -i");
    }

    #[test]
    fn search_result_counts_matches_and_unique_files() {
        let p = ToolGrepSummaryPlugin::new();
        let result = serde_json::json!({
            "pattern": "fn ",
            "root": ".",
            "matches": [
                {"file": "a.rs", "line": 1, "column": 1, "text": "fn a() {}"},
                {"file": "a.rs", "line": 2, "column": 1, "text": "fn b() {}"},
                {"file": "b.rs", "line": 3, "column": 1, "text": "fn c() {}"}
            ],
            "truncated": false
        })
        .to_string();
        let spans = p.summarize_tool_result("search", &result).unwrap();
        assert_eq!(join(&spans), "3 matches in 2 files");
    }

    #[test]
    fn search_result_shows_truncated_flag() {
        let p = ToolGrepSummaryPlugin::new();
        let result = serde_json::json!({
            "pattern": "fn ",
            "root": ".",
            "matches": [
                {"file": "a.rs", "line": 1, "column": 1, "text": "fn a() {}"}
            ],
            "truncated": true
        })
        .to_string();
        let spans = p.summarize_tool_result("search", &result).unwrap();
        assert_eq!(join(&spans), "1 matches in 1 files (truncated)");
    }

    #[test]
    fn returns_none_for_unknown_tool() {
        let p = ToolGrepSummaryPlugin::new();
        assert!(
            p.summarize_tool_call("read_file", &serde_json::json!({}))
                .is_none()
        );
    }

    #[test]
    fn returns_none_on_args_parse_failure() {
        let p = ToolGrepSummaryPlugin::new();
        // `pattern` is required.
        assert!(
            p.summarize_tool_call("search", &serde_json::json!({}))
                .is_none()
        );
    }

    // Pins span colours so the #117 `StyledSpan::colored()` -> `StyledSpan::colored()` constructor
    // rewrite cannot change them silently.
    #[test]
    fn search_call_span_colors_are_pinned() {
        let p = ToolGrepSummaryPlugin::new();
        let spans = p
            .summarize_tool_call(
                "search",
                &serde_json::json!({
                    "pattern": "fn ",
                    "path": "src",
                    "case_insensitive": true,
                    "multiline": true
                }),
            )
            .unwrap();
        let pairs: Vec<(&str, Option<ThemeColor>)> =
            spans.iter().map(|s| (s.text.as_str(), s.fg)).collect();
        assert_eq!(
            pairs,
            vec![
                ("grep '", Some(ThemeColor::Fg)),
                ("fn ", Some(ThemeColor::Success)),
                ("'", Some(ThemeColor::Fg)),
                (" in src", Some(ThemeColor::Muted)),
                (" -i", Some(ThemeColor::Muted)),
                (" --multiline", Some(ThemeColor::Muted)),
            ]
        );
    }

    #[test]
    fn search_result_span_colors_are_pinned() {
        let p = ToolGrepSummaryPlugin::new();
        let result = serde_json::json!({
            "pattern": "fn ",
            "root": ".",
            "matches": [
                {"file": "a.rs", "line": 1, "column": 1, "text": "fn a() {}"}
            ],
            "truncated": true
        })
        .to_string();
        let spans = p.summarize_tool_result("search", &result).unwrap();
        let pairs: Vec<(&str, Option<ThemeColor>)> =
            spans.iter().map(|s| (s.text.as_str(), s.fg)).collect();
        assert_eq!(
            pairs,
            vec![
                ("1", Some(ThemeColor::Success)),
                (" matches in ", Some(ThemeColor::Fg)),
                ("1", Some(ThemeColor::Success)),
                (" files", Some(ThemeColor::Fg)),
                (" (truncated)", Some(ThemeColor::Muted)),
            ]
        );
    }
}
