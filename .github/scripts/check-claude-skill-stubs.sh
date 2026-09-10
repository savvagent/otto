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
#   3. every path named on the stub's `> **Canonical body:**` line exists, so
#      renaming or moving a canonical file fails CI instead of leaving a
#      dangling instruction behind.
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

# Frontmatter extraction results, set by frontmatter_value. Globals rather than
# command substitution, so a failure reported while extracting still counts
# towards the exit status (a subshell's increment would be lost).
FM_VALUE=""
FM_ERROR=""

fail() {
    printf '::error::%s\n' "$1"
    failures=$((failures + 1))
}

# Prints the first line of $1 whose key is $2, with any trailing CR stripped.
# Empty output means the key is absent. Line-scoped by design: a folded or
# wrapped YAML value is not supported, and is reported rather than truncated.
frontmatter_line() {
    awk -v key="$2" 'index($0, key ":") == 1 { sub(/\r$/, ""); print; exit }' "$1"
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
    if [ "$value" = "$line" ] || [ -z "$value" ]; then
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
    checked=$((checked + 1))

    if [ ! -f "$canonical" ]; then
        fail "skill '$name': $canonical does not exist, so there is nothing to check parity against. Add it, or remove the directory."
        continue
    fi

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
        fail "$stub: no '> $POINTER_MARKER' line found. The stub must name its canonical body on one unwrapped line -- this extractor is line-scoped, so a pointer wrapped across two lines is a pointer the check cannot read."
        continue
    fi

    targets="$(printf '%s\n' "$pointer" | awk -F'`' '{ for (i = 2; i <= NF; i += 2) if ($i != "") print $i }')"
    if [ -z "$targets" ]; then
        fail "$stub: the '$POINTER_MARKER' line names no backtick-quoted path. Wrap each canonical path in backticks so this check can validate it."
        continue
    fi

    while IFS= read -r target; do
        [ -n "$target" ] || continue
        if [ ! -f "$target" ]; then
            fail "$stub: the '$POINTER_MARKER' line names '$target', which does not exist. Update the pointer, and any stub prose naming it, to the file's current path."
        fi
    done <<< "$targets"
done

if [ "$failures" -gt 0 ]; then
    printf '\n%d Claude Code skill-stub parity failure(s). See CLAUDE.md, section "Claude Code skills".\n' "$failures"
    exit 1
fi

printf 'ok: %d skill(s) under %s have a matching Claude Code adapter stub\n' "$checked" "$CANONICAL_ROOT"
