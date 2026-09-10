#!/usr/bin/env bash
#
# Claude Code skill-stub parity check.
#
# Why this exists: Claude Code discovers project skills only under
# `.claude/skills/<name>/SKILL.md`, while this repo maintains the canonical
# body of its own workflow skills under `.github/skills/<name>/` in the Copilot
# CLI's format. Rather than duplicate ~100KB of the repo's most-edited process
# documents, each canonical skill gets a small adapter stub in `.claude/skills/`
# carrying the canonical frontmatter verbatim plus a pointer at the canonical
# body. That arrangement only works while the two sides stay in step, and
# nothing about it is self-enforcing -- hence this check. For every skill
# directory under `.github/skills/` it asserts:
#
#   1. an adapter stub exists at `.claude/skills/<name>/SKILL.md`, so a new
#      Copilot skill cannot be added while staying invisible to Claude Code;
#   2. the stub's `name:` and `description:` match the canonical body's
#      byte-for-byte (one optional layer of YAML quotes aside), so the two
#      hosts cannot silently start triggering on different requests;
#   3. the stub's `> **Canonical body:**` line names its own canonical body,
#      and every path it names exists, so renaming or moving a canonical file
#      fails CI instead of leaving a dangling instruction behind.
#
# Scope limits worth knowing before trusting this check further than it goes:
# extraction is line-scoped, so only paths on the marker line itself are
# validated -- a path moved onto a continuation line is not checked at all,
# which is why the pointer must name every canonical path in backticks on one
# unwrapped line, and why that line should carry paths and nothing else in
# backticks. Frontmatter is read only from the leading `---` fenced block, so a
# body line cannot stand in for a deleted key. Nothing here validates the
# *content* of a stub's translation table; that is a maintained artifact, as
# the design spec records.
#
# Claude-Code-native skills with no canonical counterpart (`rust-engineer`,
# `tui-engineer`) are deliberately not iterated: they have nothing to drift
# from.
#
# Every failure is reported before the script exits 1, so a contributor fixing
# two stubs does not need two CI runs. Failures are emitted as GitHub
# `::error::` annotations (the house style set by
# `.github/workflows/wit-dep-guard.yml`) so they surface in the PR UI, but the
# check is a standalone script rather than an inline `run:` block so it can be
# run locally before pushing:
#
#   bash .github/scripts/check-claude-skill-stubs.sh

set -euo pipefail

# Resolve the repo root from this script's own location so the check behaves
# identically however it is invoked.
cd "$(dirname "$0")/../.."

CANONICAL_ROOT=".github/skills"
STUB_ROOT=".claude/skills"
POINTER_MARKER='**Canonical body:**'

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

# Prints the first line of $1 whose key is $2, with any trailing CR stripped.
# Empty output means the key is absent. Line-scoped by design: a folded or
# wrapped YAML value is not supported, and is reported rather than truncated.
#
# Scanning stops at the closing `---`, so only the frontmatter block is read.
# Without that bound, deleting a stub's `description:` and leaving an
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
# skills quote nothing while the existing `.claude/skills/` files quote their
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
        FM_ERROR="$file: frontmatter key '$key:' is missing. Both a canonical skill and its adapter stub must declare '$key:' on a single line."
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
    canonical="$CANONICAL_ROOT/$name/SKILL.md"
    stub="$STUB_ROOT/$name/SKILL.md"
    dirs_seen=$((dirs_seen + 1))

    if [ ! -f "$canonical" ]; then
        fail "skill '$name': $canonical does not exist, so there is nothing to check parity against. Add it, or remove the directory."
        continue
    fi

    checked=$((checked + 1))

    # 1. Parity: the stub must exist at all.
    if [ ! -f "$stub" ]; then
        fail "skill '$name': no Claude Code adapter stub. Create $stub with the canonical name/description frontmatter and a '> $POINTER_MARKER' line naming $canonical."
        continue
    fi

    # 2. Frontmatter match, key by key.
    for key in name description; do
        if ! frontmatter_value "$canonical" "$key"; then
            fail "$FM_ERROR"
            continue
        fi
        canonical_value="$FM_VALUE"

        if ! frontmatter_value "$stub" "$key"; then
            fail "$FM_ERROR"
            continue
        fi
        stub_value="$FM_VALUE"

        if [ "$canonical_value" != "$stub_value" ]; then
            fail "skill '$name': frontmatter '$key' differs between the canonical body and its adapter stub. Copy the canonical value byte-for-byte, or the two hosts will trigger differently on the same request."
            printf '  %s: %s = %s\n' "$canonical" "$key" "$canonical_value"
            printf '  %s: %s = %s\n' "$stub" "$key" "$stub_value"
        fi
    done

    # 3. Pointer targets: every canonical path the stub names must exist.
    pointer="$(awk -v marker="$POINTER_MARKER" 'index($0, marker) > 0 { sub(/\r$/, ""); print; exit }' "$stub")"
    if [ -z "$pointer" ]; then
        fail "$stub: no '> $POINTER_MARKER' line found. The stub must name its canonical body on one unwrapped line -- this extractor reads only the marker line, so a path on a continuation line is not validated at all."
        continue
    fi

    # Odd-indexed fields are the backtick-delimited spans. Anything else in
    # backticks on this line is read as a path and reported as missing, which
    # fails safe but confusingly -- keep the pointer line to paths only.
    targets="$(printf '%s\n' "$pointer" | awk -F'`' '{ for (i = 2; i <= NF; i += 2) if ($i != "") print $i }')"
    if [ -z "$targets" ]; then
        fail "$stub: the '$POINTER_MARKER' line names no backtick-quoted path. Wrap each canonical path in backticks, on that one line, so this check can validate it."
        continue
    fi

    # A stub must point at its OWN canonical body. Existence alone is too weak:
    # after a half-applied rename the pointer can still name some other skill's
    # file, which exists, so the check would pass while the stub sends its
    # reader to the wrong workflow. This also catches a pointer whose first
    # path is right but whose remaining paths were reflowed onto a
    # continuation line, where nothing validates them.
    case $'\n'"$targets"$'\n' in
        *$'\n'"$canonical"$'\n'*) ;;
        *) fail "$stub: the '$POINTER_MARKER' line does not name $canonical. A stub must point at its own canonical body, in backticks, on that one line." ;;
    esac

    # Every Markdown file in the canonical directory must be named, not just
    # SKILL.md. A companion the stub never names is a companion Claude Code
    # never reads -- `otto-development`'s `agent-prompts.md` holds the dispatch
    # templates its workflow requires be pasted verbatim, so losing it is
    # losing the workflow. This is also what makes the line-scoped extractor
    # safe: reflow a companion onto a continuation line and it stops being
    # named here, which now fails rather than passing silently.
    for companion in "$CANONICAL_ROOT/$name"/*.md; do
        [ -f "$companion" ] || continue
        case $'\n'"$targets"$'\n' in
            *$'\n'"$companion"$'\n'*) ;;
            *) fail "$stub: the '$POINTER_MARKER' line does not name $companion, which is part of this skill's canonical body. Name every canonical file in backticks on that one line -- a companion the stub does not name is one Claude Code will never read." ;;
        esac
    done

    while IFS= read -r target; do
        [ -n "$target" ] || continue
        if [ ! -f "$target" ]; then
            fail "$stub: the '$POINTER_MARKER' line names '$target', which does not exist. Update the pointer, and any stub prose naming it, to the file's current path."
        fi
    done <<< "$targets"
done

# Zero skills must not be a green build. Without this, moving or renaming
# `.github/skills/` orphans every stub while the glob quietly matches nothing
# and the summary reports success over an empty set -- the drift class this
# check exists to catch, arriving as a pass.
if [ "$dirs_seen" -eq 0 ]; then
    fail "$CANONICAL_ROOT contains no skill directories. If the canonical skills moved, update CANONICAL_ROOT in this script; otherwise this check is silently passing over nothing."
fi

if [ "$failures" -gt 0 ]; then
    printf '\n%d Claude Code skill-stub parity failure(s). See CLAUDE.md, section "Claude Code skills".\n' "$failures"
    exit 1
fi

printf 'ok: %d skill(s) under %s have a matching Claude Code adapter stub\n' "$checked" "$CANONICAL_ROOT"
