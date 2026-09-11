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
# mirror the edit into the port under `.claude/skills/`, then regenerate the
# expected diff with this script's `--update` mode. Never edit a port in place.
# Mirroring is verbatim for a canonical edit that lands outside the adapted
# regions; an edit that lands *on* an adapted line needs the host-mechanism
# adaptation re-applied there. The mismatch failure message says both.
#
# Two modes, both run from anywhere (the repo root is resolved from this
# script's own location):
#
#   bash .github/scripts/check-claude-skill-ports.sh             # verify (default)
#   bash .github/scripts/check-claude-skill-ports.sh --update    # rewrite the records
#
# Verify is the default and the only thing CI runs: it exits non-zero on any
# mismatch and never writes. `--update` regenerates every expected-diff record
# through the same `port_diff` function the verifier compares against, and
# reports which records it rewrote.
#
# `--update` is not a bypass for the *structural* failures: a missing port, an
# orphaned record, an orphaned port, a zero-divergence port, a port missing its
# "Ported for Claude Code" note and `name`/`description` drift are all still
# failures under `--update`, because regenerating a record cannot fix any of
# them. It *is* exactly the bypass for the mismatch failure -- which is the one
# this whole arrangement exists to catch. Editing a canonical, not mirroring the
# edit into the port, and running `--update` produces a clean green build over a
# port that is missing the new text. So run `--update` only after mirroring the
# edit by hand; reaching for it *because* the build is red is the wrong reflex,
# and nothing here can stop you.
#
# For every skill directory under `.github/skills/` it asserts:
#
#   1. every `*.md` in the canonical directory *at any depth* has a port at
#      `.claude/skills/<name>/<same relative path>` -- a companion the port
#      lacks is one Claude Code can never read, and `otto-development`'s
#      `agent-prompts.md` holds the dispatch templates its workflow requires be
#      pasted verbatim, so losing it is losing the workflow. The recursion
#      matters: a `reference/` subdirectory is a normal skill shape, and a
#      non-recursive glob would let a nested canonical file drift unported
#      behind a green build;
#   2. every canonical `*.md` has an expected-diff record at
#      `.github/skills/<name>/claude-port/<relative path>.diff`; every `.diff`
#      there has a canonical file (an orphaned record is a record nothing
#      verifies); and every `*.md` in the port directory has a canonical file
#      (an orphaned *port* is a rule Claude Code executes that the source of
#      truth does not contain);
#   3. the recomputed `diff -u canonical ported` matches that record
#      byte-for-byte (compared with `cmp`, so trailing-newline drift is caught
#      too), and is non-empty -- a port with zero divergence is a verbatim
#      copy, which is precisely the arrangement the port exists to avoid, and a
#      zero-byte record would otherwise bless one;
#   4. every port carries the `Ported for Claude Code` note, and every directory
#      under `.claude/skills/` is either a port or a name declared in
#      `NATIVE_SKILLS`. CLAUDE.md requires the note on every port so a reader
#      knows the file is derived rather than authoritative and does not edit it
#      in place, and the verbatim-copy failure message cites it as the minimum
#      divergence -- yet nothing asserted it, so it could be dropped from every
#      port on a green build. The `NATIVE_SKILLS` half is the companion: a port
#      whose canonical directory is deleted takes its records with it and is
#      never iterated, so without an explicit allowlist of the skills that
#      legitimately have no canonical, such a port stays live, executed by
#      Claude Code, and verified by nothing;
#   5. `name:` and `description:` match between the canonical `SKILL.md` and its
#      port (one optional layer of YAML quotes aside). This is reported as its
#      own failure rather than as an opaque diff mismatch because it is the
#      failure that silently changes which requests each host triggers on. It
#      carries more weight than it looks: "regenerate the record" is the
#      endorsed fix for a red build, and after a regeneration this assertion is
#      the only surviving check -- so it rejects block scalars, duplicate keys,
#      an unterminated frontmatter block, and a `key:` prefix that is really a
#      different key (`name:s: v`), rather than comparing whatever it can find.
#
# It also refuses to read hostile repository content. CI runs this check on
# `pull_request`, so a fork's tree reaches it: a canonical file, a port, a
# record or a `claude-port/` directory committed as a *symlink* is rejected
# rather than followed (following
# one would print an arbitrary file -- `.git/config`, which `actions/checkout`
# leaves the job token in -- into the public log via the diff-of-diffs), and a
# skill directory or path segment outside `[A-Za-z0-9._-]` is rejected rather
# than interpolated into a message a maintainer might paste into a shell.
# `fail` escapes `%`, CR and LF so a crafted name cannot forge a workflow
# command (`::add-mask::`, `::stop-commands::`) at column 0 of the job log.
#
# `LC_ALL=C` is exported below and is load-bearing, not hygiene: GNU diffutils
# *translates* `\ No newline at end of file`, so a record regenerated under a
# French locale commits a translated marker and turns CI red for everyone. With
# the locale pinned, a record is reproducible on any machine and any checkout
# date. (It is reproducible across GNU diffutils versions; a record generated
# by GNU diff is not certified to verify byte-identically under BSD diff, whose
# unified-diff output differs in details. CI and the documented local
# invocation both use GNU diff.)
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
# files such as `agent-prompts.md` carry no frontmatter.
#
# Claude-Code-native skills with no canonical counterpart (`rust-engineer`,
# `tui-engineer`) are deliberately not iterated: they have nothing to drift
# from. They must, however, be *declared* in `NATIVE_SKILLS` below. Nothing in
# this check verifies their content -- that is human review's job -- so the
# allowlist is what keeps "nothing verifies this" a deliberate, one-line,
# reviewable statement rather than something a directory can drift into by
# having its canonical deleted.
#
# Every failure is reported before the script exits 1, so a contributor fixing
# two ports does not need two CI runs. Failures are emitted as GitHub
# `::error::` annotations (the house style set by
# `.github/workflows/wit-dep-guard.yml`) so they surface in the PR UI, but the
# check is a standalone script rather than an inline `run:` block so it can be
# run locally before pushing.

set -euo pipefail

# Pinned for `diff`'s translated `\ No newline at end of file` marker (see the
# header) and, incidentally, for stable glob and `sort` collation.
export LC_ALL=C

# Both directory sweeps below iterate `<root>/*/`, which does not match a
# dot-prefixed directory. Without this, `.github/skills/.hidden/SKILL.md` needs
# no port and no record and does not even increment `dirs_seen` -- an unexamined
# skill file behind a green build, which is the outcome this check exists to
# make impossible. `valid_path` accepts a leading dot (while still rejecting `.`
# and `..`), so such a directory is verified like any other rather than named in
# an error.
shopt -s dotglob

usage() {
    printf 'usage: bash .github/scripts/check-claude-skill-ports.sh [--update]\n'
    printf '  (no argument)  verify every port against its recorded divergence; exit 1 on any mismatch\n'
    printf '  --update       regenerate every expected-diff record through port_diff, then verify the rest\n'
}

mode="verify"
if [ "$#" -gt 1 ]; then
    printf '::error::too many arguments; this check takes at most one\n' >&2
    usage >&2
    exit 2
fi
case "${1-}" in
    "") ;;
    --update | --write) mode="update" ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        printf '::error::unknown argument: %s\n' "${1-}" >&2
        usage >&2
        exit 2
        ;;
esac

# Resolve the repo root from this script's own location so the check behaves
# identically however it is invoked.
cd "$(dirname "$0")/../.."

CANONICAL_ROOT=".github/skills"
PORT_ROOT=".claude/skills"
DIFF_SUBDIR="claude-port"

# Claude-Code-native skills: entries under `.claude/skills/` that legitimately
# have no canonical counterpart and nothing to drift from. Declared here, as a
# space-delimited allowlist, rather than *inferred* from the absence of the
# "Ported for Claude Code" note -- because inference makes the note the sole
# thing separating "native skill" from "port whose canonical was deleted", and
# the note is a line in a file anyone can remove. Deleting a canonical directory
# takes its `claude-port/` records with it, so a stripped note plus a deleted
# canonical used to leave a port live, executed by Claude Code, and verified by
# nothing, on a green build.
#
# Adding a genuinely native skill means adding its name here. That is the point:
# it is one reviewable line, and it is the only way a directory under
# `.claude/skills/` is allowed to exist with nothing checking it.
NATIVE_SKILLS="rust-engineer tui-engineer otto-scanner"
UPDATE_CMD="bash .github/scripts/check-claude-skill-ports.sh --update"

failures=0
checked=0
ports_seen=0
records_seen=0
dirs_seen=0
updated=0

# Scratch space for the byte-exact record comparison. `$(port_diff ...)` would
# strip trailing newlines from both sides, hiding exactly the trailing-newline
# drift a byte-for-byte comparison is supposed to catch.
# `mktemp -d` with no template is a GNU extension; BSD/macOS `mktemp` has
# accepted it only since a relatively recent version, and a failure here kills
# the script under `set -e` before a single check runs. Pass an explicit
# template, which every implementation accepts. INT/TERM join EXIT on the trap
# so an interrupted local run does not leave the scratch directory behind.
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/skillports.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM

# Frontmatter extraction results, set by frontmatter_value. Globals because the
# extractor has to hand back two things -- a value and, on failure, a message --
# and a bash function returns only a status. Routing it through command
# substitution instead would push the failure report inside the function, where
# a `failures` increment would be lost to the subshell.
FM_VALUE=""
FM_ERROR=""

# GitHub's workflow-command parser reads `::error::` lines out of stdout, so an
# unescaped value reaching this printf is an injection vector: a filename
# carrying a newline could emit its own `::add-mask::` or `::stop-commands::`
# line at column 0, and a bare `%` can be decoded as an escape. Escape the
# three characters that matter, `%` first -- otherwise the escapes we introduce
# get re-escaped.
fail() {
    local msg="$1"
    msg="${msg//'%'/%25}"
    msg="${msg//$'\r'/%0D}"
    msg="${msg//$'\n'/%0A}"
    printf '::error::%s\n' "$msg"
    failures=$((failures + 1))
}

# Rejects anything outside `[A-Za-z0-9._-]+`, and `.`/`..`, one path segment at
# a time. Names from the repository tree end up inside human-readable messages
# and, historically, inside a shell command a maintainer was told to paste -- a
# canonical file named `a'$(id)'b.md` closes the quote in such a command.
# Refusing the name outright is cheaper than trying to quote every use of it.
valid_path() {
    local remaining="$1" segment
    [ -n "$remaining" ] || return 1
    while [ -n "$remaining" ]; do
        segment="${remaining%%/*}"
        case "$segment" in
            "" | "." | "..") return 1 ;;
            *[!A-Za-z0-9._-]*) return 1 ;;
        esac
        if [ "$segment" = "$remaining" ]; then
            break
        fi
        remaining="${remaining#*/}"
    done
    return 0
}

# `-f` follows symlinks, so without this a port committed as a symlink to
# `../../../.git/config` would be read and printed into the CI log by the
# diff-of-diffs. Nothing in this arrangement has any reason to be a symlink.
reject_symlink() {
    if [ -L "$1" ]; then
        fail "$1 is a symlink. Canonical skill bodies, their Claude Code ports and their expected-diff records must be regular files -- this check refuses to follow a symlink out of the tree."
        return 1
    fi
    return 0
}

# The one true expected-diff invocation. The `-L` labels replace the mtime-
# bearing `---`/`+++` header lines `diff -u` would otherwise emit, so a
# committed record is reproducible on any machine and any checkout date. Both
# `--update` (which writes the records) and the verifier (which recomputes
# them) go through this one function, so the two cannot disagree about the
# form.
#
# `diff` exits 1 whenever the files differ -- which is always, that being the
# point -- and 2 on *trouble* (an unreadable file, most likely). Collapsing
# those two with a blanket `|| true` would turn "could not read the canonical"
# into an empty diff that compares equal to an empty record, i.e. a green build
# over a file nobody looked at. So: 0 and 1 pass through, anything higher is
# reported by the caller.
port_diff() {
    local status=0
    diff -u -L "$1" -L "$2" "$1" "$2" || status=$?
    if [ "$status" -gt 1 ]; then
        return "$status"
    fi
    return 0
}

# Extracts frontmatter key $2 from file $1, emitting one of:
#
#   NOFENCE            -- no leading `---` line (a UTF-8 BOM or a blank first
#                         line lands here too)
#   UNTERMINATED       -- the leading `---` is never closed, so there is no
#                         frontmatter block at all; such a file is also one
#                         Claude Code cannot load
#   COUNT <n>\nVALUE <v>
#
# A key line must be the literal key, a colon, and then end-of-line or
# whitespace. Splitting on the first `": "` anywhere in the line instead would
# read `name:s: v` -- whose only key is `name:s` -- as a `name` of `s`, letting
# a typo that deletes the key pass as a match. Occurrences are counted rather
# than short-circuited: two `description:` lines make this check verify a value
# the host may never use, since YAML parsers variously take the last or error.
frontmatter_raw() {
    awk -v key="$2" '
        BEGIN { klen = length(key) + 1; count = 0; opened = 0; closed = 0; bad = 0; value = "" }
        NR == 1 {
            if ($0 !~ /^---[[:space:]]*$/) { bad = 1; exit }
            opened = 1
            next
        }
        /^---[[:space:]]*$/ { closed = 1; exit }
        {
            if (substr($0, 1, klen) == key ":") {
                rest = substr($0, klen + 1)
                if (rest == "" || rest ~ /^[[:space:]]/) {
                    count++
                    if (count == 1) { value = rest }
                }
            }
        }
        END {
            if (bad || !opened) { print "NOFENCE"; exit }
            if (!closed) { print "UNTERMINATED"; exit }
            sub(/^[[:space:]]+/, "", value)
            sub(/[[:space:]]+$/, "", value)
            printf "COUNT %d\n", count
            printf "VALUE %s\n", value
        }' "$1"
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
# message in FM_ERROR for every way the value can fail to be a single-line
# plain scalar -- silently comparing two empty strings, or two block-scalar
# introducers, would make this whole check vacuous.
#
# Note for future call sites: the `if ! frontmatter_raw ...` form is
# deliberate. A bare `out=$(frontmatter_raw ...)` assignment would let
# `errexit` kill the script mid-loop on an unreadable file instead of reporting
# it, so keep the read inside a condition.
frontmatter_value() {
    local file="$1" key="$2" out count value
    FM_VALUE=""
    FM_ERROR=""
    if ! out="$(frontmatter_raw "$file" "$key")"; then
        FM_ERROR="$file: could not be read while extracting frontmatter key '$key:'."
        return 1
    fi
    case "$out" in
        NOFENCE)
            FM_ERROR="$file: no '---' frontmatter block at the top of the file. Line 1 must be exactly '---' -- a UTF-8 BOM or a leading blank line breaks it even when '$key:' is visibly present below."
            return 1
            ;;
        UNTERMINATED)
            FM_ERROR="$file: unterminated frontmatter block -- the leading '---' is never closed by a second '---' line. Claude Code cannot load such a skill, and this check will not fall back to scanning the body for '$key:'."
            return 1
            ;;
    esac
    count="${out%%$'\n'*}"
    count="${count#COUNT }"
    value="${out#*$'\n'}"
    value="${value#VALUE }"
    if [ "$count" = "0" ]; then
        FM_ERROR="$file: frontmatter key '$key:' is missing. Both a canonical skill and its Claude Code port must declare '$key:' on a single line, written as '$key: <value>' -- note that '$key:x: <value>' declares a key called '$key:x', not '$key'."
        return 1
    fi
    if [ "$count" != "1" ]; then
        FM_ERROR="$file: frontmatter key '$key:' is declared $count times. Declare it once -- YAML parsers variously take the last one or error, so this check cannot know which value the host will use."
        return 1
    fi
    if [ -z "$value" ]; then
        FM_ERROR="$file: frontmatter key '$key:' is present but its value is empty. Write it as '$key: <value>' on one line."
        return 1
    fi
    # A prefix rule, deliberately not a list of known introducers. An
    # enumeration of '>' '>-' '|-' … misses every neighbour that is equally
    # valid YAML -- '>2' and '|4' (indentation indicators), '>- # c' (trailing
    # comment), '~' and 'null' (null), '# c' (a comment, so the value is null)
    # -- and each of those would let the canonical and its port carry entirely
    # different text while this check compared two identical meaningless
    # strings. That matters more than it looks: regenerating a record is the
    # prescribed fix for a red build, and after a regeneration the frontmatter
    # assertion is the only check still standing.
    #
    # No false positives: a plain YAML scalar cannot begin with '>', '|' or
    # '#', since those are indicators that force quoting -- so a description
    # legitimately starting with one arrives quoted, and this tests the raw
    # value before `unquote` runs.
    case "$value" in
        [\>\|]* | '~' | 'null' | '#'*)
            FM_ERROR="$file: frontmatter key '$key:' is not a single-line plain value ('$value'). This check compares plain scalars, so a block scalar, a null, or a comment would let the canonical and its port carry entirely different text and still match. Put the value itself on the '$key:' line, quoting it if it starts with '>', '|' or '#'."
            return 1
            ;;
    esac
    FM_VALUE="$(unquote "$value")"
    if [ -z "$FM_VALUE" ]; then
        FM_ERROR="$file: frontmatter key '$key:' is an empty quoted string. Give it the canonical value."
        return 1
    fi
    return 0
}

# Emits every `*.md` under $1 at any depth, NUL-separated, pruning $2. NUL and
# `read -d ''` rather than a newline-separated pipeline: a newline in a
# filename must not be able to split one path into two, and the loop has to run
# in this shell or its `failures` increments would be lost to a subshell.
find_md() {
    find "$1" -path "$2" -prune -o -type f -name '*.md' -print0
}

# Emits every symlink under $1 at any depth, NUL-separated. Deliberately prunes
# *nothing*.
#
# `find_md` filters on `-type f`, which excludes symlinks, and `find` defaults
# to `-P`, so it does not descend into a symlinked directory either. Without
# this pass a committed symlink is not rejected the way a constructed path is
# -- it is simply never examined: a symlinked canonical `*.md` would need no
# port and no record, a symlinked port would escape the orphan assertion, and a
# symlinked subdirectory would hide every file beneath it. All three are exit-0
# with an unexamined skill file, which is the outcome this whole check exists to
# make impossible.
#
# The absent `-prune` is the point, and it is a fixed bug rather than an
# oversight: pruning `claude-port/` here also prunes the *symlink named*
# `claude-port`, so this sweep structurally could not reject a `claude-port`
# committed as a link to a directory outside both trees -- and `[ -d ]` and
# `[ -f ]` below follow straight through it, reading records from wherever it
# points while the orphan walk (`find -P` on a symlinked start point) sees
# nothing. It also hid a symlinked *orphaned record*: pruned from this sweep,
# and excluded from the orphan walk by its `-type f`. `find_md` still prunes,
# because a `.diff` is not a canonical file; this sweep must see everything.
find_links() {
    find "$1" -type l -print0
}

# Emits every expected-diff record under $1, and every port `*.md` under $1,
# NUL-separated. Named functions rather than inline `find`s so both reverse
# orphan walks can be routed through `run_find` -- see the note there.
find_records() {
    find "$1" -type f -name '*.diff' -print0
}

find_ports() {
    find "$1" -type f -name '*.md' -print0
}

# Runs a NUL-emitting find into $tmp_dir/list and fails if find itself had
# trouble. `while … done < <(find …)` cannot observe find's exit status by
# construction, so an unreadable subtree would truncate the walk and read as
# "nothing to see" -- the same defect class as a `diff` trouble status being
# mistaken for "no differences", one layer up.
run_find() {
    local what="$1" dest="$2"
    shift 2
    if ! "$@" > "$dest" 2> "$tmp_dir/find_err"; then
        fail "$what: could not be walked completely -- $(tr '\n' ' ' < "$tmp_dir/find_err"). Some skill files may not have been examined, so nothing about this skill has been verified."
        return 1
    fi
    return 0
}

for dir in "$CANONICAL_ROOT"/*/; do
    [ -d "$dir" ] || continue
    name="$(basename "$dir")"
    dirs_seen=$((dirs_seen + 1))

    if ! valid_path "$name"; then
        fail "unsupported skill directory name under $CANONICAL_ROOT. A skill directory name must match [A-Za-z0-9._-]+; this check will not interpolate anything else into its output."
        continue
    fi

    reject_symlink "$CANONICAL_ROOT/$name" || continue
    if [ -e "$PORT_ROOT/$name" ] || [ -L "$PORT_ROOT/$name" ]; then
        reject_symlink "$PORT_ROOT/$name" || continue
    fi

    canonical_dir="$CANONICAL_ROOT/$name"
    port_dir="$PORT_ROOT/$name"
    canonical_skill="$canonical_dir/SKILL.md"
    port_skill="$port_dir/SKILL.md"
    diff_dir="$canonical_dir/$DIFF_SUBDIR"

    # The records directory itself, before anything reads through it. `[ -d ]`
    # and `[ -f ]` follow symlinks, so a `claude-port` committed as a link to a
    # directory outside both trees would otherwise have every record read from
    # wherever it points, with the orphan walk below silently seeing nothing.
    if [ -e "$diff_dir" ] || [ -L "$diff_dir" ]; then
        reject_symlink "$diff_dir" || continue
    fi

    if [ -L "$canonical_skill" ] || [ ! -f "$canonical_skill" ]; then
        fail "skill '$name': $canonical_skill is not a regular file, so there is nothing to port. Add it, or remove the directory."
        continue
    fi

    # 0. No symlinks anywhere in either tree, at any depth. This runs before
    #    the walks below because those filter on `-type f` and would skip a
    #    symlink rather than reject it, leaving a skill file unexamined.
    for tree in "$canonical_dir" "$port_dir"; do
        [ -d "$tree" ] || continue
        if run_find "skill '$name': $tree" "$tmp_dir/link_list" find_links "$tree"; then
            while IFS= read -r -d '' link; do
                fail "skill '$name': $link is a symlink. This check refuses to follow links inside a skill tree — a symlinked file is skipped by the walks rather than verified, and a symlinked directory hides every file beneath it. Commit a regular file."
            done < "$tmp_dir/link_list"
        fi
    done

    # 1-3. Every canonical Markdown file, at any depth, needs a port and an
    #      expected diff, and the recomputed diff must match that record.
    run_find "skill '$name': $canonical_dir" "$tmp_dir/canonical_list" find_md "$canonical_dir" "$diff_dir" || continue
    while IFS= read -r -d '' canonical; do
        rel="${canonical#"$canonical_dir"/}"

        if ! valid_path "$rel"; then
            fail "skill '$name': unsupported file path under $canonical_dir. Every path segment of a canonical skill file must match [A-Za-z0-9._-]+; this check will not interpolate anything else into its output."
            continue
        fi

        port="$port_dir/$rel"
        expected="$diff_dir/$rel.diff"

        checked=$((checked + 1))

        reject_symlink "$canonical" || continue

        if [ -e "$port" ] || [ -L "$port" ]; then
            reject_symlink "$port" || continue
        fi
        if [ ! -f "$port" ]; then
            fail "skill '$name': no Claude Code port of $rel. Create $port as an adapted copy of $canonical, then record the divergence by running, from the repo root: $UPDATE_CMD"
            continue
        fi

        # 4. The note, asserted here because this loop is already reading every
        #    port. One `grep` per port file. Nothing else verifies it, and three
        #    things depend on it: CLAUDE.md requires it, the verbatim-copy
        #    message cites it as the minimum divergence, and the
        #    orphaned-port-directory check at the bottom of this script uses it
        #    to word its message. (That check *decides* on `NATIVE_SKILLS`, not
        #    on the note -- deciding on the note would mean stripping one line
        #    and deleting a canonical directory together produced a green build.)
        if ! grep -q 'Ported for Claude Code' "$port"; then
            fail "skill '$name': $port does not carry the 'Ported for Claude Code' note. Every port must open with a line of the form '> **Ported for Claude Code** from $canonical, which is the canonical body' — without it a reader has no way to know the file is derived rather than authoritative, and would edit it in place, which is lost work as soon as the next re-port lands."
        fi

        if [ -e "$expected" ] || [ -L "$expected" ]; then
            reject_symlink "$expected" || continue
        fi

        actual="$tmp_dir/actual.diff"
        if ! port_diff "$canonical" "$port" > "$actual"; then
            fail "skill '$name': diff could not compare $canonical with $port (it exited with 'trouble' status). One of the two is unreadable — check file permissions. Nothing about this port has been verified."
            continue
        fi

        if [ ! -s "$actual" ]; then
            fail "skill '$name': the port of $rel is byte-identical to its canonical body. A port with zero divergence is a verbatim copy — the arrangement the port exists to avoid, since the canonical names tools Claude Code does not have — and it must at minimum carry the 'Ported for Claude Code' note. Adapt the port; do not record an empty divergence."
            continue
        fi

        if [ "$mode" = "update" ]; then
            if [ -f "$expected" ] && cmp -s "$actual" "$expected"; then
                continue
            fi
            mkdir -p "$(dirname "$expected")"
            cp "$actual" "$expected"
            updated=$((updated + 1))
            printf 'updated: %s\n' "$expected"
            continue
        fi

        if [ ! -f "$expected" ]; then
            fail "skill '$name': no expected-diff record for $rel. Generate it by running, from the repo root: $UPDATE_CMD"
            continue
        fi

        if [ ! -s "$expected" ]; then
            fail "skill '$name': the expected-diff record $expected is empty. An empty record asserts that the port is a verbatim copy of the canonical, which this arrangement rejects — and it would compare equal to a diff that could not be computed. Regenerate it from the repo root with: $UPDATE_CMD"
            continue
        fi

        if ! cmp -s "$actual" "$expected"; then
            fail "skill '$name': the port of $rel no longer diverges from its canonical body in the recorded way. Either the canonical was edited without mirroring the edit into the port, or the port was edited directly. Fix the port first: a canonical edit that lands outside the adapted regions is mirrored into the port verbatim, while an edit that lands on an adapted line needs the host-mechanism adaptation re-applied there. Then regenerate the record by running, from the repo root: $UPDATE_CMD"
            # Capped, and every line prefixed. Capped because a single
            # full-file mismatch has emitted 633 lines and all three ports at
            # once would be ~1,900 -- enough to bury the `::error::`
            # annotations that say what to do. Prefixed because a *context*
            # line of a unified diff begins with a space: two of them and a
            # `::`, and the runner (which trims leading whitespace before
            # matching workflow commands) would see a forged annotation at
            # column 0. `fail` escapes its own inputs, but this dump is raw
            # file content, so the prefix is what makes it inert. `head`
            # closing the pipe early raises SIGPIPE under `pipefail`; the
            # existing `|| true` absorbs it.
            printf '  --- diff-of-diffs (expected vs. recomputed) for %s, first 200 lines ---\n' "$rel"
            diff -u -L "$expected" -L "recomputed" "$expected" "$actual" | sed 's/^/    │ /' | head -n 200 || true
            printf '  --- end diff-of-diffs; run the check locally for the full text ---\n'
        fi
    done < "$tmp_dir/canonical_list"

    # 2 (reverse). An expected diff with no canonical file verifies nothing and
    #    hides a rename: the record survives while the file it described is
    #    gone.
    if [ -d "$diff_dir" ] && run_find "skill '$name': $diff_dir" "$tmp_dir/record_list" find_records "$diff_dir"; then
        while IFS= read -r -d '' record; do
            records_seen=$((records_seen + 1))
            record_rel="${record#"$diff_dir"/}"
            record_rel="${record_rel%.diff}"
            if ! valid_path "$record_rel"; then
                fail "skill '$name': unsupported expected-diff record path under $diff_dir. Every path segment must match [A-Za-z0-9._-]+; this check will not interpolate anything else into its output."
                continue
            fi
            if [ ! -f "$canonical_dir/$record_rel" ]; then
                fail "skill '$name': orphaned expected-diff record $record — there is no $canonical_dir/$record_rel for it to describe. Delete the record, or restore the canonical file it was generated from."
            fi
        done < "$tmp_dir/record_list"
    fi

    # 2 (other reverse). A port with no canonical file is a rule Claude Code
    #    executes that the source of truth does not contain — the precedence
    #    rule in CLAUDE.md says the canonical body is authoritative, so an
    #    extra port is drift, not a local override.
    if [ -d "$port_dir" ] && run_find "skill '$name': $port_dir" "$tmp_dir/port_list" find_ports "$port_dir"; then
        while IFS= read -r -d '' port_file; do
            ports_seen=$((ports_seen + 1))
            port_rel="${port_file#"$port_dir"/}"
            if ! valid_path "$port_rel"; then
                fail "skill '$name': unsupported file path under $port_dir. Every path segment of a port file must match [A-Za-z0-9._-]+; this check will not interpolate anything else into its output."
                continue
            fi
            if [ ! -f "$canonical_dir/$port_rel" ]; then
                fail "skill '$name': orphaned port $port_file — there is no $canonical_dir/$port_rel it was ported from. Claude Code would execute it while the canonical workflow does not contain it. Add the canonical file and re-port, or delete the port."
            fi
        done < "$tmp_dir/port_list"
    fi

    # 5. Frontmatter match, key by key. SKILL.md only: companion files such as
    #    agent-prompts.md carry no frontmatter by design.
    if [ -f "$port_skill" ] && [ ! -L "$port_skill" ]; then
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

# A whole port directory whose canonical skill no longer exists. The orphan
# check above runs inside the per-canonical-skill loop, so it structurally
# cannot fire for a skill whose canonical directory is gone: deleting
# `.github/skills/<name>/` takes its `claude-port/` records with it, the loop
# never sees the name, and the port is left live and unowned on a green build.
#
# Every directory here must therefore be one of two things: a port (it has a
# canonical directory) or a declared native skill (it is named in
# `NATIVE_SKILLS`). Anything else fails. The "Ported for Claude Code" note is
# used only to word the message, never to decide it -- keying the decision on
# the note would mean deleting one line from a port and deleting its canonical
# directory together produced a green build over a live, unverified skill, which
# is exactly the hole this loop exists to close.
if [ -d "$PORT_ROOT" ]; then
    for port_dir_top in "$PORT_ROOT"/*/; do
        [ -d "$port_dir_top" ] || continue
        port_name="$(basename "$port_dir_top")"
        # `fail` escapes the characters that matter, so this is consistency with
        # the script's own stated policy rather than a second line of defence:
        # no name from the tree reaches a message without passing valid_path.
        if ! valid_path "$port_name"; then
            fail "unsupported skill directory name under $PORT_ROOT. A skill directory name must match [A-Za-z0-9._-]+; this check will not interpolate anything else into its output."
            continue
        fi
        if [ -d "$CANONICAL_ROOT/$port_name" ]; then
            continue
        fi
        case " $NATIVE_SKILLS " in
            *" $port_name "*) continue ;;
        esac
        if grep -qlr 'Ported for Claude Code' "$port_dir_top" 2>/dev/null; then
            fail "skill '$port_name': $port_dir_top carries the 'Ported for Claude Code' note, but $CANONICAL_ROOT/$port_name does not exist. Its canonical body — and, since the records live beside it, its divergence records — were deleted while the port was left live: Claude Code still executes it and nothing verifies it. Restore the canonical, or delete the port."
        else
            fail "skill '$port_name': $port_dir_top is neither a port (there is no $CANONICAL_ROOT/$port_name) nor a declared Claude-Code-native skill (it is not named in NATIVE_SKILLS in this script). Claude Code executes it and nothing verifies it. If it is a port whose canonical was deleted, restore the canonical — or delete the port. If it is genuinely native, add '$port_name' to NATIVE_SKILLS."
        fi
    done
fi

# Zero skills must not be a green build. Without this, moving or renaming
# `.github/skills/` orphans every port while the glob quietly matches nothing
# and the summary reports success over an empty set -- the drift class this
# check exists to catch, arriving as a pass.
if [ "$dirs_seen" -eq 0 ]; then
    fail "$CANONICAL_ROOT contains no skill directories. If the canonical skills moved, update CANONICAL_ROOT in this script; if they have not, this check is silently passing over nothing — or it was run from outside its repository, since the repo root is resolved from the script's own path and invoking it through a symlink resolves the wrong one."
fi

if [ "$failures" -gt 0 ]; then
    printf '\n%d Claude Code skill-port parity failure(s). See CLAUDE.md, section "Claude Code skills".\n' "$failures"
    exit 1
fi

if [ "$mode" = "update" ]; then
    printf 'ok: %d expected-diff record(s) rewritten; %d canonical skill file(s) across %d skill(s) under %s, %d port file(s), %d record(s)\n' \
        "$updated" "$checked" "$dirs_seen" "$CANONICAL_ROOT" "$ports_seen" "$records_seen"
    exit 0
fi

printf 'ok: %d canonical skill file(s) across %d skill(s) under %s match their Claude Code port and its recorded divergence; %d port file(s) and %d record(s) accounted for, none orphaned\n' \
    "$checked" "$dirs_seen" "$CANONICAL_ROOT" "$ports_seen" "$records_seen"
