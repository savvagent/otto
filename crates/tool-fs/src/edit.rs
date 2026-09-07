//! Structured edit ops over UTF-8 text files: `replace`, `insert`,
//! `multi_edit`. Logical-failure atomicity: if any step in a batch fails,
//! the file on disk is left untouched. The commit step itself goes through
//! [`atomic_write`] (tmp + fsync + rename + parent fsync) so a crash between
//! commit start and OS flush leaves either the old contents or the new —
//! never a half-written file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::FsToolError;

/// Arguments to `replace`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ReplaceInput {
    /// Path to the file. Relative paths resolve against the project root.
    pub path: String,
    /// Substring to replace. UTF-8.
    pub old: String,
    /// Replacement text. UTF-8.
    pub new: String,
    /// Match-count contract. See [`ReplaceCount`].
    #[serde(default)]
    pub count: Option<ReplaceCount>,
}

/// Match-count contract for `replace`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplaceCount {
    /// Require exactly N matches.
    Exactly(u32),
    /// Replace every occurrence, even zero.
    All,
}

/// Result of `replace`.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReplaceOutput {
    /// Echo of the input path.
    pub path: String,
    /// Number of substrings replaced.
    pub replacements: u32,
}

/// Arguments to `insert`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct InsertInput {
    /// Path to the file.
    pub path: String,
    /// Insert after this 1-indexed line number. Use 0 to prepend.
    pub after_line: u32,
    /// Text to insert. A trailing newline is appended if not present.
    pub text: String,
}

/// Result of `insert`.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InsertOutput {
    /// Echo of the input path.
    pub path: String,
    /// Number of newline-separated lines inserted.
    pub lines_inserted: u32,
}

/// Arguments to `multi_edit`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct MultiEditInput {
    /// Path to the file.
    pub path: String,
    /// Sequence of edits applied in order. Either every edit lands or none do.
    pub edits: Vec<MultiEdit>,
}

/// One step in a `multi_edit` batch.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MultiEdit {
    /// In-place text replacement; same semantics as standalone `replace`.
    Replace {
        /// Substring to replace.
        old: String,
        /// Replacement text.
        new: String,
        /// Match-count contract.
        #[serde(default)]
        count: Option<ReplaceCount>,
    },
    /// Insert a block of text after the given 1-indexed line.
    Insert {
        /// Insert after this 1-indexed line. 0 prepends.
        after_line: u32,
        /// Text to insert.
        text: String,
    },
}

/// Result of `multi_edit`.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MultiEditOutput {
    /// Echo of the input path.
    pub path: String,
}

/// Apply `replace` semantics to `text`. Returns the new contents and the
/// number of replacements. Errors mirror the public surface.
pub(crate) fn apply_replace(
    text: &str,
    old: &str,
    new: &str,
    count: Option<ReplaceCount>,
) -> Result<(String, u32), FsToolError> {
    if old.is_empty() {
        return Err(FsToolError::InvalidArgument("`old` is empty".into()));
    }
    let n = text.matches(old).count() as u32;
    match count {
        None => {
            if n == 0 {
                return Err(FsToolError::InvalidArgument(
                    "`old` not found in file".into(),
                ));
            }
            if n > 1 {
                // Compute the 1-indexed line number of each match so the
                // agent can disambiguate by widening `old` to include
                // surrounding context.
                let lines: Vec<usize> = {
                    let mut out = Vec::new();
                    let mut offset = 0;
                    while let Some(pos) = text[offset..].find(old) {
                        let absolute = offset + pos;
                        let line = text[..absolute].bytes().filter(|b| *b == b'\n').count() + 1;
                        out.push(line);
                        // Advance past this match. `old.len().max(1)` guards
                        // the empty-needle case, but `old.is_empty()` is
                        // already rejected above.
                        offset = absolute + old.len().max(1);
                    }
                    out
                };
                return Err(FsToolError::InvalidArgument(format!(
                    "`old` is ambiguous: {n} matches at lines {lines:?}; pass `count = exactly: N` or `all`"
                )));
            }
            Ok((text.replacen(old, new, 1), 1))
        }
        Some(ReplaceCount::Exactly(want)) => {
            if n != want {
                return Err(FsToolError::InvalidArgument(format!(
                    "expected {want} matches, found {n}"
                )));
            }
            Ok((text.replace(old, new), n))
        }
        Some(ReplaceCount::All) => Ok((text.replace(old, new), n)),
    }
}

/// Apply `insert` semantics to `text`. Returns the new contents and the
/// number of lines inserted (counted by `\n` in the prepared insertion).
///
/// Rule: the inserted block always ends with the file's detected line ending
/// so it occupies its own row. If the fragment immediately to the *left* of
/// the insertion site is unterminated (i.e. the file lacks a trailing newline
/// and we're appending past it), we splice a line-ending onto that prior
/// fragment first — otherwise `split_inclusive('\n')` would glue the two
/// together (e.g. `"a\nb"` + insert `"x"` at line 2 would become `"a\nbx\n"`
/// instead of `"a\nb\nx\n"`). Original line endings are preserved everywhere
/// else; in particular, we never invent a terminator on the file's final line
/// unless the insert itself is happening past it.
pub(crate) fn apply_insert(
    text: &str,
    after_line: u32,
    insert: &str,
) -> Result<(String, u32), FsToolError> {
    let line_count = text.lines().count() as u32;
    if after_line > line_count {
        return Err(FsToolError::InvalidArgument(format!(
            "after_line={after_line} but file has {line_count} line(s)"
        )));
    }

    let line_ending = if text.contains("\r\n") { "\r\n" } else { "\n" };

    // Inserted block always ends with a line-ending; one logical line goes in,
    // it gets its own row.
    let mut to_insert = insert.to_string();
    if !to_insert.ends_with('\n') {
        to_insert.push_str(line_ending);
    }

    // Split preserving the original line endings. The last fragment may lack a
    // terminator (e.g. "a\nb" → ["a\n", "b"]).
    let parts: Vec<&str> = text.split_inclusive('\n').collect();
    let inserted_at = after_line as usize;

    // If the slot immediately to the left of the insertion site is an
    // unterminated fragment, splice a line-ending into it before assembling
    // the result so the inserted line doesn't merge with the previous text.
    let mut owned_parts: Vec<String> = parts.iter().map(|s| (*s).to_string()).collect();
    if inserted_at > 0
        && let Some(prev) = owned_parts.get_mut(inserted_at - 1)
        && !prev.ends_with('\n')
    {
        prev.push_str(line_ending);
    }

    owned_parts.insert(inserted_at, to_insert.clone());
    let out: String = owned_parts.concat();
    let lines_inserted = to_insert.lines().count() as u32;
    Ok((out, lines_inserted))
}

/// Returns true when `relative_path` matches the .env / .ssh / **credential
/// deny floor. Always evaluated against the path *relative to the project
/// root* — never the absolute path — so a workspace whose absolute path
/// happens to contain `credential` or sits under `~/.ssh/` doesn't have every
/// file mass-rejected.
pub(crate) fn is_denied(relative_path: &Path) -> bool {
    relative_path.components().any(|c| {
        let c = c.as_os_str().to_string_lossy().to_lowercase();
        c.contains("credential") || c == ".env" || c.starts_with(".env.") || c == ".ssh"
    })
}

/// Atomic write: tmp-file in same dir → fsync → rename.
///
/// Blocking call; wrap in [`tokio::task::spawn_blocking`] when invoked from
/// async context.
pub(crate) fn atomic_write(target: &Path, contents: &[u8]) -> Result<(), FsToolError> {
    use std::io::Write;
    let parent = target.parent().ok_or_else(|| {
        FsToolError::InvalidArgument(format!("path has no parent: {}", target.display()))
    })?;
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(
        ".otto-tmp.{pid}.{nonce}.{}",
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));

    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, target)?;
        Ok(())
    })();

    if let Err(e) = res {
        // Distinguish "tmp never got created" (ENOENT — benign) from real
        // cleanup failures that would leave a stale tmp file in the user's
        // project directory.
        match std::fs::remove_file(&tmp) {
            Ok(()) => {}
            Err(cleanup_err) if cleanup_err.kind() == std::io::ErrorKind::NotFound => {}
            Err(cleanup_err) => {
                tracing::warn!(
                    tmp = %tmp.display(),
                    error = %cleanup_err,
                    "failed to remove temp file after atomic_write error",
                );
            }
        }
        return Err(FsToolError::Io {
            op: "atomic_write".into(),
            source: e,
        });
    }

    // Durability: on POSIX, a rename is only crash-safe once the parent
    // directory entry is also synced. Best-effort — some platforms / FSes
    // (notably Windows) reject directory fsync; log and continue in that case.
    if let Some(parent) = target.parent()
        && let Ok(dir) = std::fs::File::open(parent)
        && let Err(e) = dir.sync_all()
    {
        tracing::debug!(parent = %parent.display(), "parent-dir fsync skipped: {e}");
    }

    Ok(())
}

#[cfg(test)]
mod replace_tests {
    use super::*;

    #[test]
    fn replace_unique_match() {
        let (out, n) = apply_replace("foo bar baz", "bar", "BAR", None).unwrap();
        assert_eq!(out, "foo BAR baz");
        assert_eq!(n, 1);
    }

    #[test]
    fn replace_zero_matches_errors() {
        let err = apply_replace("foo", "missing", "x", None).unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn replace_ambiguous_errors() {
        // Two matches on distinct lines so we can verify the line-number
        // disambiguation hint promised by the spec.
        let err = apply_replace("ab\nab", "ab", "X", None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ambiguous"), "{msg}");
        assert!(msg.contains("at lines"), "{msg}");
        assert!(msg.contains("1, 2"), "{msg}");
    }

    #[test]
    fn replace_count_exactly_n() {
        let (out, n) =
            apply_replace("ab ab ab", "ab", "X", Some(ReplaceCount::Exactly(3))).unwrap();
        assert_eq!(out, "X X X");
        assert_eq!(n, 3);

        let err = apply_replace("ab ab", "ab", "X", Some(ReplaceCount::Exactly(3))).unwrap_err();
        assert!(err.to_string().contains("expected 3"), "{err}");
    }

    #[test]
    fn replace_count_all_zero_ok() {
        let (out, n) = apply_replace("foo", "missing", "x", Some(ReplaceCount::All)).unwrap();
        assert_eq!(out, "foo");
        assert_eq!(n, 0);
    }
}

#[cfg(test)]
mod insert_tests {
    use super::*;

    #[test]
    fn insert_at_zero_prepends() {
        let (out, n) = apply_insert("a\nb\n", 0, "first").unwrap();
        assert_eq!(out, "first\na\nb\n");
        assert_eq!(n, 1);
    }

    #[test]
    fn insert_after_first_line() {
        let (out, n) = apply_insert("a\nb\n", 1, "x").unwrap();
        assert_eq!(out, "a\nx\nb\n");
        assert_eq!(n, 1);
    }

    #[test]
    fn insert_after_last_line_appends() {
        let (out, n) = apply_insert("a\nb\n", 2, "x").unwrap();
        assert_eq!(out, "a\nb\nx\n");
        assert_eq!(n, 1);
    }

    #[test]
    fn insert_beyond_eof_errors() {
        let err = apply_insert("a\n", 5, "x").unwrap_err();
        assert!(err.to_string().contains("after_line=5"), "{err}");
    }

    #[test]
    fn insert_preserves_crlf() {
        let (out, _) = apply_insert("a\r\nb\r\n", 1, "x").unwrap();
        assert_eq!(out, "a\r\nx\r\nb\r\n");
    }

    #[test]
    fn insert_into_file_without_trailing_newline() {
        // Append (after the last unterminated line). Without the fix the
        // previous unterminated "b" would merge with the inserted "x\n" and
        // produce "a\nbx\n".
        let (out, n) = apply_insert("a\nb", 2, "x").unwrap();
        assert_eq!(
            out, "a\nb\nx\n",
            "append must add separator before insert: {out}"
        );
        assert_eq!(n, 1);

        // Mid-file insert where the *next* fragment is the unterminated tail.
        // We must NOT invent a trailing newline for the inserted block — the
        // original file didn't have one for that line.
        let (out, n) = apply_insert("a\nb", 1, "x").unwrap();
        assert_eq!(
            out, "a\nx\nb",
            "mid-file insert preserves the unterminated tail: {out}"
        );
        assert_eq!(n, 1);
    }
}

#[cfg(test)]
mod atomic_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_round_trip() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("file.txt");
        std::fs::write(&target, b"old").unwrap();

        atomic_write(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");

        // No leftover tmp files.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".otto-tmp."))
            .collect();
        assert!(leftovers.is_empty(), "leftover: {leftovers:?}");
    }

    #[test]
    fn atomic_write_failure_leaves_original_intact() {
        let dir = tempdir().unwrap();
        // Target is a directory, not a file — rename will fail.
        let target = dir.path().join("not-a-file");
        std::fs::create_dir(&target).unwrap();

        let err = atomic_write(&target, b"data").unwrap_err();
        assert!(matches!(err, FsToolError::Io { .. }), "{err}");

        // No leftover tmp files in parent.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".otto-tmp."))
            .collect();
        assert!(leftovers.is_empty(), "leftover: {leftovers:?}");
    }
}

#[cfg(test)]
mod serde_tests {
    //! Lock the wire shapes of `MultiEdit` and `ReplaceCount` — these are
    //! exposed via JSON to MCP clients, and a tagged-enum representation
    //! change would silently break agent calls.
    use super::*;

    #[test]
    fn multi_edit_replace_roundtrip() {
        let json = r#"{"op":"replace","old":"a","new":"b"}"#;
        let parsed: MultiEdit = serde_json::from_str(json).unwrap();
        match parsed {
            MultiEdit::Replace { old, new, count } => {
                assert_eq!(old, "a");
                assert_eq!(new, "b");
                assert!(count.is_none());
            }
            other => panic!("expected Replace, got {other:?}"),
        }
    }

    #[test]
    fn multi_edit_replace_with_count_all_roundtrip() {
        let json = r#"{"op":"replace","old":"a","new":"b","count":"all"}"#;
        let parsed: MultiEdit = serde_json::from_str(json).unwrap();
        assert!(matches!(
            parsed,
            MultiEdit::Replace {
                count: Some(ReplaceCount::All),
                ..
            }
        ));
    }

    #[test]
    fn multi_edit_replace_with_count_exactly_roundtrip() {
        let json = r#"{"op":"replace","old":"a","new":"b","count":{"exactly":3}}"#;
        let parsed: MultiEdit = serde_json::from_str(json).unwrap();
        assert!(matches!(
            parsed,
            MultiEdit::Replace {
                count: Some(ReplaceCount::Exactly(3)),
                ..
            }
        ));
    }

    #[test]
    fn multi_edit_insert_roundtrip() {
        let json = r#"{"op":"insert","after_line":3,"text":"hi"}"#;
        let parsed: MultiEdit = serde_json::from_str(json).unwrap();
        match parsed {
            MultiEdit::Insert { after_line, text } => {
                assert_eq!(after_line, 3);
                assert_eq!(text, "hi");
            }
            other => panic!("expected Insert, got {other:?}"),
        }
    }
}
