//! Integration test that confirms the bwrap sandbox actually denies reads of
//! sensitive paths. Linux-only; skipped if `bwrap` is not on PATH.

#![cfg(target_os = "linux")]

use std::fs;
use std::path::PathBuf;

use otto_host::{SandboxConfig, SandboxMode, apply_sandbox};

fn bwrap_available() -> bool {
    std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).find(|d| d.join("bwrap").exists()))
        .is_some()
}

#[tokio::test]
async fn sandbox_blocks_reads_of_ssh_directory() {
    if !bwrap_available() {
        eprintln!("skipping: bwrap not on PATH");
        return;
    }

    let home = tempfile::TempDir::new().unwrap();
    let ssh = home.path().join(".ssh");
    fs::create_dir_all(&ssh).unwrap();
    let secret_path = ssh.join("id_rsa");
    fs::write(&secret_path, b"sensitive-content-xyz\n").unwrap();

    let prev_home = std::env::var_os("HOME");
    // SAFETY: this integration test runs in its own binary and is the only
    // test in the binary that mutates $HOME. Restored before assertions.
    unsafe {
        std::env::set_var("HOME", home.path());
    }

    let project_root = tempfile::TempDir::new().unwrap();
    let mut cmd = tokio::process::Command::new("/bin/cat");
    cmd.arg(&secret_path);
    let tool_bin = PathBuf::from("/usr/bin/cat");
    let config = SandboxConfig {
        mode: SandboxMode::On,
        ..SandboxConfig::default()
    };
    apply_sandbox(&mut cmd, &tool_bin, project_root.path(), &config);

    let output = cmd.output().await.expect("cat must spawn");

    unsafe {
        match prev_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("sensitive-content-xyz"),
        "sandbox leaked the secret: stdout = {stdout:?}"
    );
}
