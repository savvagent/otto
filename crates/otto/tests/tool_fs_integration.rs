//! End-to-end test: spawn `otto-tool-fs` as a child process and exercise
//! every tool over real MCP stdio framing.

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::json;
use tempfile::tempdir;

const BIN: &str = env!("CARGO_BIN_EXE_otto-tool-fs");

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lists_all_four_tools() {
    let transport =
        TokioChildProcess::new(tokio::process::Command::new(BIN).configure(|_| {})).unwrap();
    let client = ().serve(transport).await.expect("client init");

    let tools = client.list_all_tools().await.expect("list_tools");
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    for expected in ["read_file", "write_file", "list_dir", "glob"] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing tool {expected} in {names:?}"
        );
    }
    client.cancel().await.ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn round_trip_through_child_process() {
    let dir = tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().into_owned();

    let dir_for_env = dir.path().to_path_buf();
    let transport = TokioChildProcess::new(tokio::process::Command::new(BIN).configure(|c| {
        c.env("OTTO_TOOL_FS_ROOT", &dir_for_env);
    }))
    .unwrap();
    let client = ().serve(transport).await.expect("client init");

    // 1. write_file — create a new file with parent dirs.
    let write_path = format!("{dir_str}/sub/note.txt");
    let resp = client
        .call_tool(
            CallToolRequestParams::new("write_file").with_arguments(
                json!({ "path": write_path, "content": "hello mcp", "create_dirs": true })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("write_file");
    let written: serde_json::Value = resp.into_typed().expect("write_file structured content");
    assert_eq!(written["bytes_written"], 9);

    // 2. read_file — confirm we get the same bytes back.
    let resp = client
        .call_tool(
            CallToolRequestParams::new("read_file")
                .with_arguments(json!({ "path": write_path }).as_object().unwrap().clone()),
        )
        .await
        .expect("read_file");
    let read: serde_json::Value = resp.into_typed().expect("read_file structured content");
    assert_eq!(read["content"], "hello mcp");
    assert_eq!(read["bytes"], 9);

    // 3. list_dir — non-recursive should see the `sub` directory we created.
    let resp = client
        .call_tool(
            CallToolRequestParams::new("list_dir").with_arguments(
                json!({ "path": dir_str.clone() })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("list_dir");
    let listed: serde_json::Value = resp.into_typed().expect("list_dir structured content");
    let entries = listed["entries"].as_array().expect("entries array");
    assert!(
        entries
            .iter()
            .any(|e| e["name"] == "sub" && e["is_dir"] == true),
        "expected `sub` dir in {entries:?}"
    );

    // 4. glob — match the file we wrote.
    let resp = client
        .call_tool(
            CallToolRequestParams::new("glob").with_arguments(
                json!({ "pattern": "**/*.txt", "root": dir_str })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("glob");
    let matches: serde_json::Value = resp.into_typed().expect("glob structured content");
    assert!(
        matches["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m.as_str().unwrap().ends_with("note.txt")),
        "expected note.txt match in {matches}",
    );

    client.cancel().await.ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_file_too_large_surfaces_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("big.txt");
    tokio::fs::write(&path, vec![b'x'; 64]).await.unwrap();

    let dir_for_env = dir.path().to_path_buf();
    let transport = TokioChildProcess::new(tokio::process::Command::new(BIN).configure(|c| {
        c.env("OTTO_TOOL_FS_ROOT", &dir_for_env);
    }))
    .unwrap();
    let client = ().serve(transport).await.expect("client init");

    let err = client
        .call_tool(
            CallToolRequestParams::new("read_file").with_arguments(
                json!({ "path": path.to_string_lossy(), "max_bytes": 8 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect_err("expected error");
    let s = format!("{err}");
    assert!(s.contains("too large"), "{s}");

    client.cancel().await.ok();
}
