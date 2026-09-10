#!/usr/bin/env bash
#
# Claude Code skill-port parity check.
#
# Why this exists: Claude Code discovers project skills only under
# `.claude/skills/<name>/`, while this repo maintains the canonical body of its
# own workflow skills under `.github/skills/<name>/` in the GitHub Copilot
# CLI's format. Those canonical bodies name tools Claude Code does not have
# (`agent_type`, `mode: "sync"`, `read_agent`, a session-scoped `todos` table),
# so a symlink or a verbatim copy would hand a Claude Code session ~100KB of
# workflow whose every dispatch step is unexecutable. Instead each canonical
# file gets a *port* in `.claude/skills/`: an adapted copy, identical except for
# host-mechanism references. Two copies of a 100KB document drift, and nothing
# about the arrangement is self-enforcing -- hence this check.
#
# The workflow it enforces: edit the canonical file under `.github/skills/`,
# re-apply the adaptation to the port under `.claude/skills/`, then regenerate
# the expected diff. Never edit a port in place. The failure message for a
# mismatch prints those last two commands verbatim.
#
# For every skill directory under `.github/skills/` it asserts:
#
#   1. every `*.md` in the canonical directory has a port at
#      `.claude/skills/<name>/<same basename>` -- a companion the port lacks is
#      one Claude Code can never read, and `otto-development`'s
#      `agent-prompts.md` holds the dispatch templates its workflow requires be
#      pasted verbatim, so losing it is losing the workflow;
#   2. every canonical `*.md` has an expected-diff record at
#      `.github/skills/<name>/claude-port/<basename>.diff`, and every `.diff`
#      there has a canonical file -- an orphaned record is a record nothing
#      verifies;
#   3. the recomputed `diff -u canonical ported` matches that record
#      byte-for-byte. A canonical edit that was not re-ported and a port edited
#      directly are the same failure seen from two sides, and both are caught
#      here;
#   4. `name:` and `description:` match between the canonical `SKILL.md` and its
#      port (one optional layer of YAML quotes aside). This is reported as its
#      own failure rather than as an opaque diff mismatch because it is the
#      failure that silently changes which requests each host triggers on.
#
# Scope limits worth knowing before trusting this check further than it goes.
# It compares bytes, not meaning: it cannot tell whether an adaptation is a
# *good* adaptation, only that the port's divergence from the canonical is
# still exactly the divergence a reviewer approved. Rewriting the canonical's
# "How dispatch works in this environment" section needs a human to re-adapt
# it; this check can only say that it changed. Hunk headers carry line numbers,
# so an unrelated canonical edit shifts them and fails -- intended, because a
# canonical edit should force a re-read of the port rather than be silently
# absorbed. The frontmatter assertion covers `SKILL.md` only, since companion
# files such as `agent-prompts.md` carry no frontmatter; frontmatter is read
# only from the leading `---` fenced block, so a body line cannot stand in for
# a deleted key.
#
# Claude-Code-native skills with no canonical counterpart (`rust-engineer`,
# `tui-engineer`) are deliberately not iterated: they have nothing to drift
# from.
#
# Every failure is reported before the script exits 1, so a contributor fixing
# two ports does not need two CI runs. Failures are emitted as GitHub
# `::error::` annotations (the house style set by
# `.github/workflows/wit-dep-guard.yml`) so they surface in the PR UI, but the
# check is a standalone script rather than an inline `run:` block so it can be
# run locally before pushing:
#
#   bash .github/scripts/check-claude-skill-ports.sh

set -euo pipefail

# Resolve the repo root from this script's own location so the check behaves
# identically however it is invoked.
cd "$(dirname "$0")/../.."

CANONICAL_ROOT=".github/skills"
PORT_ROOT=".claude/skills"
DIFF_SUBDIR="claude-port"

failures=0
checked=0
dirs_seen=0

# Frontmatter extraction results, set by frontmatter_value. Globals because the
# extractor has to hand back two things -- a value and, on failure, a message --
# and a bash function returns only a status. Routing it through command
# substitution instead would push the failure report inside the function, where
# a `failures` increment would be lost to the subshell.
FM_VALUE=""
FM_ERROR=""

fail() {
    printf '::error::%s\n' "$1"
    failures=$((failures + 1))
}

# The one true expected-diff invocation. The `-L` labels replace the mtime-
# bearing `---`/`+++` header lines `diff -u` would otherwise emit, so a
# committed record is reproducible on any machine and any checkout date. This
# function is what generates the records and what recomputes them, so the two
# can never disagree about the form.
#
# `diff` exits 1 whenever the files differ -- which is always, that being the
# point -- so the call is guarded against `set -e`.
port_diff() {
    diff -u -L "$1" -L "$2" "$1" "$2" || true
}

# Prints the first line of $1 whose key is $2, with any trailing CR stripped.
# Empty output means the key is absent. Line-scoped by design: a folded or
# wrapped YAML value is not supported, and is reported rather than truncated.
#
# Scanning stops at the closing `---`, so only the frontmatter block is read.
# Without that bound, deleting a port's `description:` and leaving an
# unindented `description:` anywhere in the body would satisfy this check
# silently -- and both canonical bodies already document YAML dispatch blocks
# carrying that very key, two spaces from becoming decoys.
frontmatter_line() {
    awk -v key="$2" '
        NR == 1 { if ($0 !~ /^---[[:space:]]*\r?$/) exit; next }
        /^---[[:space:]]*\r?$/ { exit }
        index($0, key ":") == 1 { sub(/\r$/, ""); print; exit }' "$1"
}

# Strips one optional layer of wrapping double or single quotes. The canonical
# skills quote nothing while other `.claude/skills/` files quote their
# description, and that difference alone must not fail the check.
unquote() {
    local value="$1"
    case "$value" in
        '"'*'"') value="${value#\"}"; value="${value%\"}" ;;
        "'"*"'") value="${value#\'}"; value="${value%\'}" ;;
    esac
    printf '%s' "$value"
}

# Reads frontmatter key $2 from file $1 into FM_VALUE. Returns non-zero with a
# message in FM_ERROR when the key is absent or its value is empty -- silently
# comparing two empty strings would make this whole check useless.
frontmatter_value() {
    local file="$1" key="$2" line value
    FM_VALUE=""
    FM_ERROR=""
    line=$(frontmatter_line "$file" "$key")
    if [ -z "$line" ]; then
        FM_ERROR="$file: frontmatter key '$key:' is missing. Both a canonical skill and its Claude Code port must declare '$key:' on a single line."
        return 1
    fi
    value="${line#*": "}"
    if [ "$value" = "$line" ]; then
        # The split found no ': ', so the value is unreachable rather than
        # absent -- say which, or the reader goes hunting for a blank value
        # that is sitting right there after the colon.
        FM_ERROR="$file: frontmatter key '$key:' must be written as '$key: <value>', with a space after the colon."
        return 1
    fi
    if [ -z "$value" ]; then
        FM_ERROR="$file: frontmatter key '$key:' is present but its value is empty."
        return 1
    fi
    FM_VALUE="$(unquote "$value")"
    return 0
}

for dir in "$CANONICAL_ROOT"/*/; do
    [ -d "$dir" ] || continue
    name="$(basename "$dir")"
    dirs_seen=$((dirs_seen + 1))

    canonical_skill="$CANONICAL_ROOT/$name/SKILL.md"
    port_skill="$PORT_ROOT/$name/SKILL.md"
    diff_dir="$CANONICAL_ROOT/$name/$DIFF_SUBDIR"

    if [ ! -f "$canonical_skill" ]; then
        fail "skill '$name': $canonical_skill does not exist, so there is nothing to port. Add it, or remove the directory."
        continue
    fi

    # 1-3. Every canonical Markdown file needs a port and an expected diff, and
    #      the recomputed diff must match that record.
    for canonical in "$CANONICAL_ROOT/$name"/*.md; do
        [ -f "$canonical" ] || continue
        base="$(basename "$canonical")"
        port="$PORT_ROOT/$name/$base"
        expected="$diff_dir/$base.diff"

        checked=$((checked + 1))

        if [ ! -f "$port" ]; then
            fail "skill '$name': no Claude Code port of $base. Create $port as an adapted copy of $canonical, then record the divergence: bash .github/scripts/check-claude-skill-ports.sh will tell you the exact regeneration command."
            continue
        fi

        if [ ! -f "$expected" ]; then
            fail "skill '$name': no expected-diff record for $base. Generate it with: diff -u -L '$canonical' -L '$port' '$canonical' '$port' > '$expected'"
            continue
        fi

        actual="$(port_diff "$canonical" "$port")"
        if [ "$actual" != "$(cat "$expected")" ]; then
            fail "skill '$name': the port of $base no longer diverges from its canonical body in the recorded way. Either the canonical was edited without re-porting, or the port was edited directly. Fix the port first, then refresh the record:
    1. re-apply the host-mechanism adaptation to $port (edit the canonical, never the port, then re-port)
    2. diff -u -L '$canonical' -L '$port' '$canonical' '$port' > '$expected'"
            printf '  --- diff-of-diffs (expected vs. recomputed) for %s ---\n' "$base"
            printf '%s\n' "$actual" | diff -u -L "$expected" -L "recomputed" "$expected" - || true
        fi
    done

    # 2 (reverse). An expected diff with no canonical file verifies nothing and
    #    hides a rename: the record survives while the file it described is
    #    gone.
    if [ -d "$diff_dir" ]; then
        for record in "$diff_dir"/*.diff; do
            [ -f "$record" ] || continue
            record_base="$(basename "$record" .diff)"
            if [ ! -f "$CANONICAL_ROOT/$name/$record_base" ]; then
                fail "skill '$name': orphaned expected-diff record $record — there is no $CANONICAL_ROOT/$name/$record_base for it to describe. Delete the record, or restore the canonical file it was generated from."
            fi
        done
    fi

    # 4. Frontmatter match, key by key. SKILL.md only: companion files such as
    #    agent-prompts.md carry no frontmatter by design.
    if [ -f "$port_skill" ]; then
        for key in name description; do
            if ! frontmatter_value "$canonical_skill" "$key"; then
                fail "$FM_ERROR"
                continue
            fi
            canonical_value="$FM_VALUE"

            if ! frontmatter_value "$port_skill" "$key"; then
                fail "$FM_ERROR"
                continue
            fi
            port_value="$FM_VALUE"

            if [ "$canonical_value" != "$port_value" ]; then
                fail "skill '$name': frontmatter '$key' differs between the canonical body and its Claude Code port. Copy the canonical value byte-for-byte, or the two hosts will trigger differently on the same request."
                printf '  %s: %s = %s\n' "$canonical_skill" "$key" "$canonical_value"
                printf '  %s: %s = %s\n' "$port_skill" "$key" "$port_value"
            fi
        done
    fi
done

# Zero skills must not be a green build. Without this, moving or renaming
# `.github/skills/` orphans every port while the glob quietly matches nothing
# and the summary reports success over an empty set -- the drift class this
# check exists to catch, arriving as a pass.
if [ "$dirs_seen" -eq 0 ]; then
    fail "$CANONICAL_ROOT contains no skill directories. If the canonical skills moved, update CANONICAL_ROOT in this script; otherwise this check is silently passing over nothing."
fi

if [ "$failures" -gt 0 ]; then
    printf '\n%d Claude Code skill-port parity failure(s). See CLAUDE.md, section "Claude Code skills".\n' "$failures"
    exit 1
fi

printf 'ok: %d canonical skill file(s) under %s match their Claude Code port and its recorded divergence\n' "$checked" "$CANONICAL_ROOT"
