//! Filterable list-of-commands modal.

use async_trait::async_trait;
use otto_plugin::{
    Effect, KeyCodePortable, KeyEventPortable, PluginError, Region, Screen, StyledLine, StyledSpan,
    TextMods, ThemeColor,
};

/// One row in the palette: a slash command name, its summary, and whether
/// it requires an argument (selecting a `needs_arg == true` row prefills
/// the textarea with `"/cmd "` instead of dispatching the slash immediately).
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    /// Slash command name without the leading `/`.
    pub name: String,
    /// One-line summary from the plugin's `SlashSpec.summary`.
    pub description: String,
    /// `true` if the command's `SlashSpec.requires_arg` is set — the
    /// palette then prefills the textarea with `"/cmd "` on selection
    /// instead of dispatching the slash with empty args.
    pub needs_arg: bool,
}

/// Modal screen that lets the user filter and run slash commands by name.
///
/// The command list is populated by `apply_effects::open_screen` from the
/// runtime's [`crate::plugin::manifests::Indexes::slash`] map and each
/// owning plugin's manifest — so disabled plugins' slashes don't appear,
/// and new plugins are picked up without touching this file.
///
/// On `Enter`:
/// - `needs_arg == true` rows emit `Stack([CloseScreen, PrefillInput])`
///   so the user can complete the slash (typically via the `@` file picker).
/// - Other rows emit `Stack([CloseScreen, RunSlash])`.
pub struct PaletteScreen {
    filter: String,
    cursor: usize,
    commands: Vec<PaletteCommand>,
}

impl PaletteScreen {
    /// Empty palette with no rows; only useful before
    /// `apply_effects::open_screen` replaces it with a populated screen
    /// via [`Self::with_commands`].
    pub fn empty() -> Self {
        Self {
            filter: String::new(),
            cursor: 0,
            commands: Vec::new(),
        }
    }

    /// Populate the palette with `commands` (already sorted by the caller).
    pub fn with_commands(commands: Vec<PaletteCommand>) -> Self {
        Self {
            filter: String::new(),
            cursor: 0,
            commands,
        }
    }

    fn filtered(&self) -> Vec<(usize, &PaletteCommand)> {
        let f = self.filter.to_ascii_lowercase();
        self.commands
            .iter()
            .enumerate()
            .filter(|(_, c)| c.name.contains(&f))
            .collect()
    }

    /// The text the prompt should show while this screen owns input: the
    /// leading `/` plus exactly what the user has typed.
    ///
    /// This deliberately does *not* resolve the highlighted row into the
    /// prompt. #92 did, and #96 reversed it: the prompt is the one surface a
    /// terminal user expects to echo their keystrokes, so showing `/connect`
    /// while they typed `/co` made backspace look like it replaced the word.
    /// The `\u{25b6}` marker in the list is what communicates the pending
    /// selection; the resolved name reaches the prompt only on `Enter`.
    ///
    /// The empty-filter case (`/`) and the no-match case (`/<filter>`) both
    /// fall out of this rather than needing their own branches.
    #[must_use]
    pub fn prompt_preview(&self) -> String {
        format!("/{}", self.filter)
    }
}

impl Default for PaletteScreen {
    fn default() -> Self {
        Self::empty()
    }
}

#[async_trait]
impl Screen for PaletteScreen {
    fn id(&self) -> String {
        "palette".to_string()
    }

    fn render(&self, region: Region) -> Vec<StyledLine> {
        // No `> <filter>` header here: the sheet is anchored directly above
        // the prompt (`ui.rs::bottom_sheet_rect`), and the prompt now echoes
        // the filter itself, so a header would put the same string on two
        // adjacent rows. The prompt is the input surface; this is the list.
        let mut lines: Vec<StyledLine> = Vec::new();
        if self.commands.is_empty() {
            lines.push(StyledLine::plain(""));
            lines.push(StyledLine::muted(
                rust_i18n::t!("picker.command-palette.no-commands").to_string(),
            ));
            return lines;
        }
        // Computed once: `render` runs at >=20Hz and `filtered` allocates
        // both a lowercased needle and a Vec on every call.
        let filtered = self.filtered();
        // A filter matching nothing needs its own empty state, and it has to
        // return here — before the windowing arithmetic — so the blank
        // spacer row below doesn't land above it. Without the old header
        // this state would otherwise render as a blank rectangle.
        if filtered.is_empty() {
            lines.push(StyledLine::plain(""));
            lines.push(StyledLine::muted(
                rust_i18n::t!("picker.command-palette.no-matches").to_string(),
            ));
            return lines;
        }
        // Align description column across rows by padding the slash-name
        // span to the widest name in the filtered list (with a 12-char
        // floor + 2 cols of breathing room). The internal `connect <id>`
        // namespace is filtered out at palette-build time (in
        // `effects.rs::build_palette_commands`), so the dynamic width
        // only needs to accommodate the remaining (shorter) command names.
        let name_col_width = filtered
            .iter()
            .map(|(_, c)| c.name.chars().count())
            .max()
            .unwrap_or(0)
            .max(12)
            + 2;
        // The list lives in a fixed-height `BottomSheet`, so a long,
        // unfiltered command set (30+ builtins) won't fit. Window the rows
        // around the cursor, budgeting *two* of the region's rows for
        // non-command chrome: the spacer/scroll-hint line, and the sheet's
        // last row — which the host overpaints with our `tips()` after the
        // paragraph (`ui.rs::paint_screen`). Reserving one fewer would put
        // a row we still drew underneath the tips line, and since the
        // window is anchored so the cursor sits on its *last* row, that
        // hidden row is the selected one: the `▶` highlight would vanish
        // for every scrolled list. (This was three while `render` also
        // drew a `> filter` header; dropping that header freed a row.)
        //
        // The spacer stays unconditional even with the header gone.
        // Reclaiming it when there is no hint would be circular: `hint`
        // derives from `hidden_above`/`hidden_below`, which derive from
        // `capacity`, which would then derive from `hint`.
        //
        // `.max(1)` is a floor, not panic-protection — a capacity of 0
        // yields a valid empty slice. What it does at `height <= 2` is
        // force one row into a budget with no room for it, so `tips()`
        // overpaints the `▶` row. That boundary is pre-existing and this
        // change shrinks it: it used to bite at `height <= 3`.
        let capacity = (region.height as usize).saturating_sub(2).max(1);
        let window_start = if filtered.len() <= capacity {
            0
        } else {
            (self.cursor + 1).saturating_sub(capacity)
        };
        let window_end = (window_start + capacity).min(filtered.len());

        let hidden_above = window_start;
        let hidden_below = filtered.len() - window_end;
        // Only name the side that actually has hidden rows: at either end
        // of a long list the other count is zero, and "↑0 more above" is
        // noise rather than information.
        let hint = match (hidden_above, hidden_below) {
            (0, 0) => String::new(),
            (0, below) => format!("  ↓{below} more below"),
            (above, 0) => format!("  ↑{above} more above"),
            (above, below) => format!("  ↑{above} more above · ↓{below} more below"),
        };
        if hint.is_empty() {
            lines.push(StyledLine::plain(""));
        } else {
            lines.push(StyledLine::muted(hint));
        }
        for (visual_idx, (_, cmd)) in filtered[window_start..window_end]
            .iter()
            .enumerate()
            .map(|(i, c)| (window_start + i, c))
        {
            let marker = if visual_idx == self.cursor {
                "▶ "
            } else {
                "  "
            };
            let name_with_slash = format!("/{}", cmd.name);
            let pad_count = name_col_width.saturating_sub(name_with_slash.chars().count());
            let padding = " ".repeat(pad_count);
            lines.push(StyledLine {
                spans: vec![
                    StyledSpan {
                        text: format!("{marker}{name_with_slash}{padding}"),
                        fg: Some(if visual_idx == self.cursor {
                            ThemeColor::Accent
                        } else {
                            ThemeColor::Fg
                        }),
                        bg: None,
                        modifiers: TextMods {
                            bold: visual_idx == self.cursor,
                            ..Default::default()
                        },
                    },
                    StyledSpan::muted(cmd.description.clone()),
                ],
            });
        }
        lines
    }

    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        match key.code {
            KeyCodePortable::Esc => {
                // Close the screen and clear the preview.
                Ok(vec![
                    Effect::CloseScreen,
                    Effect::PrefillInput {
                        text: String::new(),
                    },
                ])
            }
            // Navigation moves the highlight, which is not an edit to the
            // user's text — so it emits nothing, matching `connect`'s picker.
            // The prompt keeps whatever was typed. Safe for rendering: the
            // main loop redraws unconditionally every iteration and polls on
            // a 50ms timeout, so the marker repaints without an effect.
            KeyCodePortable::Up => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                Ok(vec![])
            }
            KeyCodePortable::Down => {
                let max = self.filtered().len().saturating_sub(1);
                if self.cursor < max {
                    self.cursor += 1;
                }
                Ok(vec![])
            }
            KeyCodePortable::Backspace => {
                // Backspacing past the leading `/` closes the palette. Now
                // that the prompt is a literal echo of what was typed, a
                // `/` that cannot be deleted would contradict that — and
                // would leave backspace as the one editing key with no
                // effect in a prompt the user believes they are editing.
                // This is the codebase's own established intent: see the
                // legacy helper's doc comment in `app.rs`, "the caller can
                // use this to close the palette on Backspace past the
                // leading `/`".
                if self.filter.pop().is_none() {
                    return Ok(vec![
                        Effect::CloseScreen,
                        Effect::PrefillInput {
                            text: String::new(),
                        },
                    ]);
                }
                self.cursor = 0;
                Ok(vec![Effect::PrefillInput {
                    text: self.prompt_preview(),
                }])
            }
            KeyCodePortable::Char(c) => {
                self.filter.push(c);
                self.cursor = 0;
                // Emit the updated preview.
                Ok(vec![Effect::PrefillInput {
                    text: self.prompt_preview(),
                }])
            }
            KeyCodePortable::Enter => {
                let filtered = self.filtered();
                let Some((_, cmd)) = filtered.get(self.cursor).cloned() else {
                    // No match: close screen and clear preview.
                    return Ok(vec![
                        Effect::CloseScreen,
                        Effect::PrefillInput {
                            text: String::new(),
                        },
                    ]);
                };
                let name = cmd.name.clone();
                if cmd.needs_arg {
                    // Don't fire the slash with empty args (which would
                    // error with "usage: /<cmd> <path>"). Instead, close
                    // the palette and seed the textarea so the user can
                    // complete the line — typically via the `@` file
                    // picker — before pressing Enter.
                    Ok(vec![Effect::Stack(vec![
                        Effect::CloseScreen,
                        Effect::PrefillInput {
                            text: format!("/{name} "),
                        },
                    ])])
                } else {
                    // No-argument command: clear preview before running
                    // so stale text doesn't remain behind.
                    Ok(vec![Effect::Stack(vec![
                        Effect::CloseScreen,
                        Effect::PrefillInput {
                            text: String::new(),
                        },
                        Effect::RunSlash { name, args: vec![] },
                    ])])
                }
            }
            // Everything else is inert, and deliberately so: the prompt is
            // an *echo* of `self.filter`, not an editable buffer. The
            // filter is append-only (plus the backspace above), so there is
            // no cursor within it for Left/Right/Home/End/Delete/Ctrl-W to
            // address. Do not read the backspace rationale above as a
            // general principle and wire these up without first giving the
            // filter a cursor position of its own.
            _ => Ok(vec![]),
        }
    }

    /// The tips row names the command `Enter` will actually dispatch.
    ///
    /// This is load-bearing, not decoration. `filtered` matches on
    /// `name.contains(filter)` — a substring, not a prefix — over slash
    /// commands contributed by *any* enabled plugin, including third-party
    /// ones. So the highlighted row is not necessarily the command whose
    /// name the user is partway through typing: a plugin registering
    /// `acl-export` captures the highlight for someone typing `cl` on
    /// their way to `clear`, and `RunSlash` is applied with no further
    /// confirmation.
    ///
    /// Until #96 the prompt itself showed the resolved name, which was the
    /// signal that the pending command was not the one being typed. Now
    /// that the prompt echoes the raw filter, that signal has to live
    /// somewhere, and this row is the right place: the host paints it last
    /// (`ui.rs::paint_screen`), so it survives even at sheet heights too
    /// short to show the `\u{25b6}` row at all.
    fn tips(&self) -> Vec<StyledLine> {
        let filtered = self.filtered();
        match filtered.get(self.cursor) {
            Some((_, cmd)) => vec![StyledLine::plain(
                rust_i18n::t!(
                    "picker.command-palette.tips-run",
                    cmd = format!("/{}", cmd.name)
                )
                .to_string(),
            )],
            // Nothing is highlighted, so there is nothing for `Enter` to
            // name — fall back to the generic affordance line.
            None => vec![StyledLine::plain(
                rust_i18n::t!("picker.command-palette.tips").to_string(),
            )],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otto_plugin::KeyMods;

    fn key(c: KeyCodePortable) -> KeyEventPortable {
        KeyEventPortable {
            code: c,
            modifiers: KeyMods::default(),
        }
    }

    fn cmd(name: &str, needs_arg: bool) -> PaletteCommand {
        PaletteCommand {
            name: name.into(),
            description: format!("{name} description"),
            needs_arg,
        }
    }

    fn fixture() -> PaletteScreen {
        // Alphabetically sorted — matches apply_effects::open_screen ordering.
        // `demo`/`zeta` are synthetic stand-ins for an arg-taking command
        // (not aliases of any real slash), so their `needs_arg` flag here
        // doesn't need to track any actual command's `requires_arg` value.
        PaletteScreen::with_commands(vec![
            cmd("clear", false),
            cmd("demo", true),
            cmd("exit", false),
            cmd("theme", false),
            cmd("zeta", true),
        ])
    }

    #[tokio::test]
    async fn enter_emits_close_then_runslash_for_first_match() {
        let mut p = fixture();
        let effs = p.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match effs.first() {
            Some(Effect::Stack(children)) => {
                assert!(matches!(children[0], Effect::CloseScreen));
                // children[1] should be PrefillInput to clear the preview
                match &children[1] {
                    Effect::PrefillInput { text } if text.is_empty() => {}
                    other => panic!("expected PrefillInput to clear preview, got {other:?}"),
                }
                // children[2] should be RunSlash
                match &children[2] {
                    Effect::RunSlash { name, .. } => assert_eq!(name, "clear"),
                    other => panic!("expected RunSlash, got {other:?}"),
                }
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn typing_filters_and_resets_cursor() {
        let mut p = fixture();
        p.on_key(key(KeyCodePortable::Char('e'))).await.unwrap();
        let filtered = p.filtered();
        assert!(filtered.iter().all(|(_, c)| c.name.contains('e')));
        assert_eq!(p.cursor, 0);
    }

    /// Selecting a `needs_arg` command must seed the textarea with its
    /// name plus a trailing space rather than firing the slash with empty
    /// args (which would error out with a usage message). Regression test
    /// for hotfix bug #1.
    #[tokio::test]
    async fn enter_on_needs_arg_command_emits_prefill_not_runslash() {
        let mut p = fixture();
        for ch in "demo".chars() {
            p.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        let effs = p.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match effs.first() {
            Some(Effect::Stack(children)) => {
                assert!(matches!(children[0], Effect::CloseScreen));
                match &children[1] {
                    Effect::PrefillInput { text } => assert_eq!(text, "/demo "),
                    other => panic!("expected PrefillInput, got {other:?}"),
                }
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// `exit` is reachable from the palette (post-v0.9 regression).
    #[tokio::test]
    async fn exit_is_listed_and_runs_via_runslash() {
        let mut p = fixture();
        for ch in "exit".chars() {
            p.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        assert!(!p.filtered().is_empty(), "palette should list /exit");
        let effs = p.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match effs.first() {
            Some(Effect::Stack(children)) => {
                assert!(matches!(children[0], Effect::CloseScreen));
                // children[1] should be PrefillInput to clear the preview
                match &children[1] {
                    Effect::PrefillInput { text } if text.is_empty() => {}
                    other => panic!("expected PrefillInput to clear preview, got {other:?}"),
                }
                // children[2] should be RunSlash
                match &children[2] {
                    Effect::RunSlash { name, args } => {
                        assert_eq!(name, "exit");
                        assert!(args.is_empty());
                    }
                    other => panic!("expected RunSlash, got {other:?}"),
                }
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// Empty command set renders a placeholder and Enter is a no-op-ish close.
    #[tokio::test]
    async fn empty_palette_renders_placeholder_and_enter_closes() {
        let mut p = PaletteScreen::empty();
        let lines = p.render(Region {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        });
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect();
        assert!(
            joined.contains(rust_i18n::t!("picker.command-palette.no-commands").as_ref()),
            "empty render should show placeholder, got: {joined}"
        );
        // Pins span colours so the #117 constructor rewrite cannot change them silently.
        let no_commands_line = lines
            .iter()
            .find(|l| !l.spans.is_empty() && !l.spans[0].text.is_empty())
            .expect("no-commands line");
        assert_eq!(no_commands_line.spans[0].fg, Some(ThemeColor::Muted));
        let effs = p.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        assert!(matches!(effs[0], Effect::CloseScreen));
    }

    #[tokio::test]
    async fn esc_closes() {
        let mut p = fixture();
        let effs = p.on_key(key(KeyCodePortable::Esc)).await.unwrap();
        assert!(matches!(effs[0], Effect::CloseScreen));
    }

    /// The palette renders inside a fixed-height `BottomSheet`, so a
    /// command set that doesn't fit the region must be windowed around
    /// the cursor rather than silently truncated with no indication.
    #[tokio::test]
    async fn long_list_fits_shows_no_scroll_hint() {
        let commands: Vec<_> = (0..5).map(|i| cmd(&format!("cmd{i}"), false)).collect();
        let p = PaletteScreen::with_commands(commands);
        // capacity = height(12) - 2 = 10, which comfortably fits all 5 rows.
        let lines = p.render(Region {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        });
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect();
        assert!(!joined.contains("more above"));
        assert!(!joined.contains("more below"));
        for i in 0..5 {
            assert!(joined.contains(&format!("/cmd{i}")));
        }
    }

    /// When the filtered list overflows the sheet's capacity, only a
    /// window around the cursor renders, plus a hint showing how many
    /// rows are hidden above/below. Only the non-zero side of the hint is
    /// named — at either end of the list the other count is 0, and
    /// "↑0 more above" would be noise.
    #[tokio::test]
    async fn overflowing_list_windows_around_cursor_with_scroll_hint() {
        let commands: Vec<_> = (0..20).map(|i| cmd(&format!("cmd{i:02}"), false)).collect();
        let mut p = PaletteScreen::with_commands(commands);
        // capacity = height(6) - 2 = 4 visible rows out of 20 commands.
        let region = Region {
            x: 0,
            y: 0,
            width: 80,
            height: 6,
        };

        let lines = p.render(region);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect();
        // Cursor starts at 0: window is [0, 4), nothing hidden above but
        // 16 rows hidden below.
        assert!(joined.contains("/cmd00"));
        assert!(joined.contains("/cmd01"));
        assert!(joined.contains("/cmd02"));
        assert!(joined.contains("/cmd03"));
        assert!(!joined.contains("/cmd04"));
        assert!(joined.contains("↓16 more below"));
        assert!(
            !joined.contains("more above"),
            "nothing is hidden above at the top of the list, got: {joined}"
        );
        // Pins span colours so the #117 constructor rewrite cannot change them silently.
        let hint_line = lines
            .iter()
            .find(|l| {
                l.spans
                    .first()
                    .is_some_and(|s| s.text.contains("more below"))
            })
            .expect("scroll-hint line");
        assert_eq!(hint_line.spans[0].fg, Some(ThemeColor::Muted));

        // Move the cursor to the end; the window should follow it so the
        // selected row is always visible, and hidden-above must update.
        for _ in 0..19 {
            p.on_key(key(KeyCodePortable::Down)).await.unwrap();
        }
        let lines = p.render(region);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect();
        assert!(joined.contains("/cmd19"));
        assert!(joined.contains("↑16 more above"));
        assert!(
            !joined.contains("more below"),
            "nothing is hidden below at the end of the list, got: {joined}"
        );
        // Pins span colours so the #117 constructor rewrite cannot change them silently.
        let hint_line = lines
            .iter()
            .find(|l| {
                l.spans
                    .first()
                    .is_some_and(|s| s.text.contains("more above"))
            })
            .expect("scroll-hint line");
        assert_eq!(hint_line.spans[0].fg, Some(ThemeColor::Muted));
    }

    /// Regression: the host overpaints the sheet's last row with `tips()`
    /// *after* the paragraph, so `render` must emit at most
    /// `region.height - 1` lines. The window is anchored so the cursor
    /// sits on its last row, which is precisely the row that would be
    /// swallowed — leaving the `▶` selection invisible for the rest of
    /// the list once it scrolls.
    ///
    /// Swept across region heights rather than pinned to one, because the
    /// reserved-row count is a constant that a future edit can move by
    /// one without any single height noticing.
    ///
    /// The sweep starts at 3: at `height <= 2` the `.max(1)` floor forces
    /// one command row into a budget with no room for it, so the
    /// line-count bound is false by construction there. That boundary is
    /// pre-existing and documented at the `capacity` binding; the `▶`
    /// presence half is asserted at every height including those.
    #[tokio::test]
    async fn cursor_row_never_lands_on_the_tips_row_at_any_height() {
        for height in 1..=14u16 {
            let commands: Vec<_> = (0..30).map(|i| cmd(&format!("cmd{i:02}"), false)).collect();
            let mut p = PaletteScreen::with_commands(commands);
            let region = Region {
                x: 0,
                y: 0,
                width: 80,
                height,
            };

            // Walk the whole list; at no point may the selected row fall
            // on (or past) the row the tips line will claim.
            for step in 0..30 {
                let lines = p.render(region);
                let cursor_row = lines
                    .iter()
                    .position(|l| l.spans.iter().any(|s| s.text.starts_with("▶")))
                    .unwrap_or_else(|| {
                        panic!("selected row missing at height {height}, step {step}")
                    });

                if height >= 3 {
                    let visible = height as usize - 1; // tips row is not ours
                    assert!(
                        lines.len() <= visible,
                        "height {height}: render emitted {} lines into {visible} usable rows",
                        lines.len()
                    );
                    assert!(
                        cursor_row < visible,
                        "height {height}: cursor row {cursor_row} would be overpainted by tips"
                    );
                }
                p.on_key(key(KeyCodePortable::Down)).await.unwrap();
            }
        }
    }

    /// The sheet must never render as a blank panel. With commands loaded
    /// but a filter matching none of them, the old `> <filter>` header was
    /// the only line drawn; removing it without an empty state would leave
    /// an empty rectangle above the prompt.
    #[tokio::test]
    async fn no_match_renders_an_empty_state_not_a_blank_sheet() {
        let mut p = fixture();
        for ch in "xyz".chars() {
            p.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        assert!(p.filtered().is_empty(), "filter should match nothing");
        let lines = p.render(Region {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        });
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect();
        assert!(
            joined.contains(rust_i18n::t!("picker.command-palette.no-matches").as_ref()),
            "no-match render should show the empty state, got: {joined:?}"
        );
        assert!(
            !joined.trim().is_empty(),
            "the sheet must not be blank when the filter matches nothing"
        );
        // Pins span colours so the #117 constructor rewrite cannot change them silently.
        // Both the empty-commands state above and this no-matches state use Muted —
        // unlike `connect`'s picker, where the two empty states use different colours.
        let no_matches_line = lines
            .iter()
            .find(|l| {
                l.spans
                    .first()
                    .is_some_and(|s| s.text == rust_i18n::t!("picker.command-palette.no-matches"))
            })
            .expect("no-matches line");
        assert_eq!(no_matches_line.spans[0].fg, Some(ThemeColor::Muted));
    }

    /// The sheet no longer draws its own `> <filter>` header — the prompt
    /// one row below echoes the filter, so a header would duplicate it.
    #[tokio::test]
    async fn render_has_no_filter_header() {
        let mut p = fixture();
        p.on_key(key(KeyCodePortable::Char('c'))).await.unwrap();
        let lines = p.render(Region {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        });
        assert!(
            !lines
                .iter()
                .any(|l| l.spans.iter().any(|s| s.text.starts_with("> "))),
            "no rendered line may start with the old header prefix"
        );
    }

    /// A filtered list whose length is exactly the capacity must render in
    /// full — the off-by-one that hid the cursor also hid this last row
    /// while reporting `0 more below`.
    #[tokio::test]
    async fn list_exactly_filling_capacity_renders_every_row() {
        // capacity = height(12) - 2 = 10.
        let commands: Vec<_> = (0..10).map(|i| cmd(&format!("cmd{i}"), false)).collect();
        let p = PaletteScreen::with_commands(commands);
        let lines = p.render(Region {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        });
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect();
        for i in 0..10 {
            assert!(joined.contains(&format!("/cmd{i}")), "missing /cmd{i}");
        }
        assert!(!joined.contains("more below"));
    }

    // --- Prompt preview tests (issues #80, #96) ---

    /// The preview echoes the typed filter, not the highlighted row. A
    /// fresh palette has an empty filter, so it previews a bare `/`.
    #[test]
    fn prompt_preview_starts_as_a_bare_slash() {
        let p = fixture();
        assert_eq!(p.prompt_preview(), "/");
    }

    /// An empty palette previews `/` for the same reason: empty filter.
    #[test]
    fn prompt_preview_empty_palette_shows_slash() {
        let p = PaletteScreen::empty();
        assert_eq!(p.prompt_preview(), "/");
    }

    /// Typing echoes character-for-character, even though the highlighted
    /// row resolves to a longer name. This is the #96 behavior: typing
    /// `cl` must not turn the prompt into `/clear`.
    #[tokio::test]
    async fn prompt_preview_echoes_typed_characters() {
        let mut p = fixture();
        p.on_key(key(KeyCodePortable::Char('c'))).await.unwrap();
        assert_eq!(p.prompt_preview(), "/c");
        p.on_key(key(KeyCodePortable::Char('l'))).await.unwrap();
        assert_eq!(p.prompt_preview(), "/cl");
        assert_eq!(
            p.filtered().first().map(|(_, c)| c.name.as_str()),
            Some("clear"),
            "a row should be highlighted — the point is that it does not \
             reach the prompt"
        );
    }

    /// A filter matching nothing still echoes the filter.
    #[tokio::test]
    async fn prompt_preview_no_match_shows_filter() {
        let mut p = fixture();
        for ch in "xyz".chars() {
            p.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        assert!(p.filtered().is_empty(), "filter should match nothing");
        assert_eq!(p.prompt_preview(), "/xyz");
    }

    /// Backspace on a non-empty filter removes exactly one character.
    #[tokio::test]
    async fn prompt_preview_backspace_removes_one_character() {
        let mut p = fixture();
        p.on_key(key(KeyCodePortable::Char('c'))).await.unwrap();
        p.on_key(key(KeyCodePortable::Char('o'))).await.unwrap();
        assert_eq!(p.prompt_preview(), "/co");
        p.on_key(key(KeyCodePortable::Backspace)).await.unwrap();
        assert_eq!(p.prompt_preview(), "/c");
    }

    /// Navigation moves the highlight without touching the prompt.
    #[tokio::test]
    async fn prompt_preview_is_unchanged_by_navigation() {
        let mut p = fixture();
        p.on_key(key(KeyCodePortable::Char('e'))).await.unwrap();
        assert_eq!(p.prompt_preview(), "/e");
        let before = p.cursor;
        p.on_key(key(KeyCodePortable::Down)).await.unwrap();
        assert_ne!(p.cursor, before, "Down should move the highlight");
        assert_eq!(p.prompt_preview(), "/e");
        p.on_key(key(KeyCodePortable::Up)).await.unwrap();
        assert_eq!(p.prompt_preview(), "/e");
    }

    /// `Char` and `Backspace` edit the user's text, so they emit
    /// `PrefillInput`. `Up` and `Down` do not — see the two tests below.
    #[tokio::test]
    async fn char_emits_prefill_input() {
        let mut p = fixture();
        let effs = p.on_key(key(KeyCodePortable::Char('d'))).await.unwrap();
        let has_prefill = effs
            .iter()
            .any(|e| matches!(e, Effect::PrefillInput { .. }));
        assert!(
            has_prefill,
            "Char key should emit PrefillInput, got: {:?}",
            effs
        );
    }

    #[tokio::test]
    async fn backspace_emits_prefill_input() {
        let mut p = fixture();
        p.on_key(key(KeyCodePortable::Char('d'))).await.unwrap();
        let effs = p.on_key(key(KeyCodePortable::Backspace)).await.unwrap();
        let has_prefill = effs
            .iter()
            .any(|e| matches!(e, Effect::PrefillInput { .. }));
        assert!(
            has_prefill,
            "Backspace should emit PrefillInput, got: {:?}",
            effs
        );
    }

    #[tokio::test]
    async fn up_emits_no_effects() {
        let mut p = fixture();
        p.on_key(key(KeyCodePortable::Down)).await.unwrap();
        let effs = p.on_key(key(KeyCodePortable::Up)).await.unwrap();
        assert!(
            effs.is_empty(),
            "Up must not touch the prompt, got: {:?}",
            effs
        );
    }

    #[tokio::test]
    async fn down_emits_no_effects() {
        let mut p = fixture();
        let effs = p.on_key(key(KeyCodePortable::Down)).await.unwrap();
        assert!(
            effs.is_empty(),
            "Down must not touch the prompt, got: {:?}",
            effs
        );
    }

    /// `Esc` should close the screen and clear the preview by emitting
    /// `PrefillInput` with an empty string.
    #[tokio::test]
    async fn esc_clears_preview() {
        let mut p = fixture();
        let effs = p.on_key(key(KeyCodePortable::Esc)).await.unwrap();
        let clear_effect = effs
            .iter()
            .find(|e| matches!(e, Effect::PrefillInput { text } if text.is_empty()));
        assert!(
            clear_effect.is_some(),
            "Esc should emit PrefillInput with empty text to clear preview, got: {:?}",
            effs
        );
    }

    /// Empty-result `Enter` should close the screen and clear the preview.
    #[tokio::test]
    async fn empty_result_enter_clears_preview() {
        let mut p = fixture();
        // Filter to no matches.
        for ch in "xyz".chars() {
            p.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        assert!(p.filtered().is_empty());
        let effs = p.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        let clear_effect = effs
            .iter()
            .find(|e| matches!(e, Effect::PrefillInput { text } if text.is_empty()));
        assert!(
            clear_effect.is_some(),
            "empty-result Enter should emit PrefillInput with empty text, got: {:?}",
            effs
        );
    }

    /// `Enter` on a no-argument command should run the slash and clear the
    /// preview (no stale preview text remains afterward).
    #[tokio::test]
    async fn no_arg_enter_clears_preview_before_run() {
        let mut p = fixture();
        let effs = p.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        // Should have Stack([CloseScreen, ?, RunSlash]) where one of the
        // middle effects is a PrefillInput that clears the preview.
        match effs.first() {
            Some(Effect::Stack(children)) => {
                assert!(matches!(children[0], Effect::CloseScreen));
                let has_clear = children
                    .iter()
                    .any(|e| matches!(e, Effect::PrefillInput { text } if text.is_empty()));
                let has_runslash = children
                    .iter()
                    .any(|e| matches!(e, Effect::RunSlash { .. }));
                assert!(
                    has_clear && has_runslash,
                    "no-arg Enter should clear preview before RunSlash, got: {:?}",
                    children
                );
            }
            other => panic!("expected Stack, got: {:?}", other),
        }
    }

    /// `Enter` on an argument-taking command should close the screen and
    /// prefill the input with `/<command> `, preserving the palette semantics.
    #[tokio::test]
    async fn arg_taking_enter_preserves_prefill_semantics() {
        let mut p = fixture();
        for ch in "demo".chars() {
            p.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        let effs = p.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match effs.first() {
            Some(Effect::Stack(children)) => {
                assert!(matches!(children[0], Effect::CloseScreen));
                match &children[1] {
                    Effect::PrefillInput { text } => {
                        assert_eq!(text, "/demo ");
                    }
                    other => panic!("expected PrefillInput, got: {:?}", other),
                }
            }
            other => panic!("expected Stack, got: {:?}", other),
        }
    }

    /// Typing narrows the list but the prompt still shows only what was
    /// typed. `d` matches exactly one command, `demo` — the case where
    /// resolving the highlight into the prompt is most tempting and most
    /// wrong, because the user typed one character, not four.
    #[tokio::test]
    async fn prompt_preview_echoes_the_char_not_the_sole_match() {
        let mut p = fixture();
        assert_eq!(p.prompt_preview(), "/");
        p.on_key(key(KeyCodePortable::Char('d'))).await.unwrap();
        assert_eq!(
            p.filtered().len(),
            1,
            "'d' should narrow the fixture to exactly one command"
        );
        assert_eq!(p.prompt_preview(), "/d");
    }

    /// Backspacing back to an empty filter returns the prompt to a bare
    /// slash rather than to the newly highlighted command.
    #[tokio::test]
    async fn prompt_preview_returns_to_slash_on_backspace() {
        let mut p = fixture();
        p.on_key(key(KeyCodePortable::Char('d'))).await.unwrap();
        assert_eq!(p.prompt_preview(), "/d");
        p.on_key(key(KeyCodePortable::Backspace)).await.unwrap();
        assert_eq!(p.prompt_preview(), "/");
    }

    /// Backspace on a non-empty filter edits, and does not close.
    #[tokio::test]
    async fn backspace_on_a_non_empty_filter_does_not_close() {
        let mut p = fixture();
        p.on_key(key(KeyCodePortable::Char('c'))).await.unwrap();
        let effs = p.on_key(key(KeyCodePortable::Backspace)).await.unwrap();
        assert!(
            !effs.iter().any(|e| matches!(e, Effect::CloseScreen)),
            "backspace over a typed character must not close, got: {effs:?}"
        );
        match effs.as_slice() {
            [Effect::PrefillInput { text }] => assert_eq!(text, "/"),
            other => panic!("expected a single PrefillInput, got: {other:?}"),
        }
    }

    /// Navigation still drives selection. The old code proved this
    /// incidentally, because `Down` emitted a `PrefillInput` naming the new
    /// row; now that it emits nothing, assert the coupling directly.
    #[tokio::test]
    async fn navigation_still_drives_what_enter_dispatches() {
        let mut p = fixture();
        p.on_key(key(KeyCodePortable::Down)).await.unwrap();
        p.on_key(key(KeyCodePortable::Down)).await.unwrap();
        let effs = p.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match effs.first() {
            Some(Effect::Stack(children)) => match &children[2] {
                Effect::RunSlash { name, .. } => assert_eq!(name, "exit"),
                other => panic!("expected RunSlash for the third row, got {other:?}"),
            },
            other => panic!("expected Stack, got {other:?}"),
        }
    }

    /// The tips row names what `Enter` will dispatch, so the pending
    /// command is stated somewhere even though the prompt now echoes only
    /// the raw filter.
    #[tokio::test]
    async fn tips_name_the_command_enter_will_run() {
        let mut p = fixture();
        assert!(
            p.tips()[0].spans.iter().any(|s| s.text.contains("/clear")),
            "tips should name the highlighted command, got: {:?}",
            p.tips()
        );
        p.on_key(key(KeyCodePortable::Down)).await.unwrap();
        assert!(
            p.tips()[0].spans.iter().any(|s| s.text.contains("/demo")),
            "tips should follow the highlight, got: {:?}",
            p.tips()
        );
    }

    /// The case the tips row exists for. `filtered` is a substring match
    /// over commands contributed by any enabled plugin, so a plugin can
    /// register a name that captures the highlight from the builtin the
    /// user is typing toward. The prompt shows what they typed; the tips
    /// row must show what would actually run.
    #[tokio::test]
    async fn tips_name_a_plugin_command_that_captures_the_highlight() {
        // Sorts before "clear" and contains "cl".
        let mut p =
            PaletteScreen::with_commands(vec![cmd("acl-export", false), cmd("clear", false)]);
        for ch in "cl".chars() {
            p.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        assert_eq!(
            p.prompt_preview(),
            "/cl",
            "the prompt echoes what was typed"
        );
        let tips = p.tips();
        let text: String = tips[0].spans.iter().map(|s| s.text.clone()).collect();
        assert!(
            text.contains("/acl-export"),
            "tips must name the command that would actually run, got: {text}"
        );
    }

    /// With nothing highlighted there is no command to name, so the tips
    /// row falls back to the generic affordance line rather than naming a
    /// stale command.
    #[tokio::test]
    async fn tips_fall_back_when_nothing_is_highlighted() {
        let mut p = fixture();
        for ch in "xyz".chars() {
            p.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        assert!(p.filtered().is_empty());
        let text: String = p.tips()[0].spans.iter().map(|s| s.text.clone()).collect();
        assert_eq!(
            text,
            rust_i18n::t!("picker.command-palette.tips").to_string(),
            "with nothing highlighted the tips row must be the generic line"
        );
    }

    /// Backspace past the leading `/` closes the palette and clears the
    /// prompt — otherwise the `/` would be undeletable.
    #[tokio::test]
    async fn backspace_past_the_slash_closes_and_clears() {
        let mut p = fixture();
        let effs = p.on_key(key(KeyCodePortable::Backspace)).await.unwrap();
        match effs.as_slice() {
            [Effect::CloseScreen, Effect::PrefillInput { text }] => {
                assert!(text.is_empty(), "prompt should be cleared, got: {text:?}");
            }
            other => panic!("expected CloseScreen then an empty PrefillInput, got: {other:?}"),
        }
    }

    // Pins span colours so the #117 constructor rewrite cannot change them silently.
    #[test]
    fn row_spans_use_accent_for_cursor_and_fg_for_others_with_muted_description() {
        let p = fixture();
        let lines = p.render(Region {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        });
        // Row 0 is unconditional plain spacer/hint line; rows are command rows after that.
        let cursor_row = &lines[1];
        assert_eq!(cursor_row.spans[0].fg, Some(ThemeColor::Accent));
        assert!(cursor_row.spans[0].modifiers.bold);
        assert_eq!(cursor_row.spans[1].fg, Some(ThemeColor::Muted));

        let other_row = &lines[2];
        assert_eq!(other_row.spans[0].fg, Some(ThemeColor::Fg));
        assert!(!other_row.spans[0].modifiers.bold);
        assert_eq!(other_row.spans[1].fg, Some(ThemeColor::Muted));
    }
}
