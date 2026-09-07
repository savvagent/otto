//! End-to-end JSON round-trip tests covering the surface of SPP.

use otto_protocol::{
    BlockDelta, CompleteRequest, CompleteResponse, ContentBlock, ErrorKind, Message, ProviderError,
    Role, StopReason, StreamEvent, ToolDef, Usage, UsageDelta,
};
use serde_json::json;

#[test]
fn full_request_round_trip() {
    let raw = json!({
        "model": "claude-sonnet-4-6",
        "messages": [
            {
                "role": "user",
                "content": [{ "type": "text", "text": "list /tmp" }]
            }
        ],
        "system": "Be terse.",
        "tools": [
            {
                "name": "ls",
                "description": "list a directory",
                "input_schema": { "type": "object", "properties": { "path": { "type": "string" } } }
            }
        ],
        "max_tokens": 1024,
        "stream": true
    });

    let req: CompleteRequest = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(req.tools.len(), 1);
    assert!(req.stream);
    let back = serde_json::to_value(&req).unwrap();
    // No info loss except for omitted-because-default fields.
    assert_eq!(back["model"], raw["model"]);
    assert_eq!(back["tools"][0]["name"], "ls");
}

#[test]
fn full_response_round_trip() {
    let resp = CompleteResponse {
        id: "msg_1".into(),
        model: "claude-sonnet-4-6".into(),
        content: vec![
            ContentBlock::Text { text: "hi".into() },
            ContentBlock::ToolUse {
                id: "toolu_1".into(),
                name: "ls".into(),
                input: json!({ "path": "/tmp" }),
            },
        ],
        stop_reason: StopReason::ToolUse,
        stop_sequence: None,
        usage: Usage {
            input_tokens: 12,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: Some(1024),
        },
    };
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["stop_reason"], "tool_use");
    assert_eq!(v["usage"]["cache_read_input_tokens"], 1024);
    assert!(v["usage"].get("cache_creation_input_tokens").is_none());
    let back: CompleteResponse = serde_json::from_value(v).unwrap();
    assert_eq!(back.content.len(), 2);
}

#[test]
fn stream_events_round_trip() {
    let events = vec![
        StreamEvent::MessageStart {
            id: "msg_1".into(),
            model: "claude-sonnet-4-6".into(),
            usage: Usage {
                input_tokens: 100,
                ..Default::default()
            },
        },
        StreamEvent::ContentBlockStart {
            index: 0,
            block: ContentBlock::Text {
                text: String::new(),
            },
        },
        StreamEvent::ContentBlockDelta {
            index: 0,
            delta: BlockDelta::TextDelta {
                text: "Hello".into(),
            },
        },
        StreamEvent::ContentBlockDelta {
            index: 0,
            delta: BlockDelta::TextDelta {
                text: " world".into(),
            },
        },
        StreamEvent::ContentBlockStop { index: 0 },
        StreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            stop_sequence: None,
            usage_delta: UsageDelta {
                output_tokens: Some(2),
                ..Default::default()
            },
        },
        StreamEvent::MessageStop,
    ];

    for e in &events {
        let v = serde_json::to_value(e).unwrap();
        let back: StreamEvent = serde_json::from_value(v).unwrap();
        assert_eq!(&back, e);
    }
}

#[test]
fn tool_use_input_delta_then_stop() {
    let start = StreamEvent::ContentBlockStart {
        index: 1,
        block: ContentBlock::ToolUse {
            id: "toolu_1".into(),
            name: "ls".into(),
            input: json!({}),
        },
    };
    let delta = StreamEvent::ContentBlockDelta {
        index: 1,
        delta: BlockDelta::InputJsonDelta {
            partial_json: "{\"pa".into(),
        },
    };
    let stop = StreamEvent::ContentBlockStop { index: 1 };

    for e in [&start, &delta, &stop] {
        let v = serde_json::to_value(e).unwrap();
        let _: StreamEvent = serde_json::from_value(v).unwrap();
    }
}

#[test]
fn provider_error_shape() {
    let err = ProviderError {
        kind: ErrorKind::RateLimited,
        message: "Slow down.".into(),
        retry_after_ms: Some(30_000),
        provider_code: Some("rate_limit_error".into()),
    };
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(v["kind"], "rate_limited");
    assert_eq!(v["retry_after_ms"], 30_000);
    let back: ProviderError = serde_json::from_value(v).unwrap();
    assert_eq!(back.kind, ErrorKind::RateLimited);
}

#[test]
fn message_round_trips_with_tool_result() {
    let m = Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "toolu_1".into(),
            content: vec![ContentBlock::Text {
                text: "/tmp/a /tmp/b".into(),
            }],
            is_error: false,
        }],
    };
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v["content"][0]["type"], "tool_result");
    let back: Message = serde_json::from_value(v).unwrap();
    assert_eq!(back, m);
}

#[test]
fn tool_def_with_complex_schema() {
    let td = ToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        input_schema: json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        }),
    };
    let v = serde_json::to_value(&td).unwrap();
    let back: ToolDef = serde_json::from_value(v).unwrap();
    assert_eq!(back.name, "read_file");
}
