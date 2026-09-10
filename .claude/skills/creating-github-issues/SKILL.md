---
name: creating-github-issues
description: Use when asked to create, file, open, or log a new ticket — a bug/defect or a feature/enhancement — as a GitHub issue in this repo, as opposed to working an existing one. Handles type selection via labels, an optional assignee, and a duplicate check. For developing/shipping an EXISTING issue (spec → plan → implement → PR → review → merge → release), use `otto-development` instead.
---

> **Canonical body:** `.github/skills/creating-github-issues/SKILL.md`

This file is an adapter stub, not the skill. Claude Code discovers project
skills only under `.claude/skills/`, while this repo maintains one canonical
copy of the skill's body under `.github/skills/` in the Copilot CLI's format.
**Read the canonical body now and follow it** — it is the authority on this
repo's GitHub-Issues-only conventions: label-based type selection, the
duplicate check, the unassigned default, and the rule that no spec or plan is
written at creation time.

**No host-mechanism translation is needed.** The canonical body is plain
`gh issue` / `gh label` invocations, which work identically from Claude Code,
so nothing in it has to be reinterpreted for this host.
