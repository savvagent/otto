//! Lifted from flat `App` fields in v0.8. Same UX; PR 6 wraps this in a
//! Screen plugin.
//!
//! The picker is intentionally not WIT-portable — it takes a
//! [`crossterm::event::KeyEvent`] directly. PR 6 will wrap it in a
//! `Plugin` trait implementation that adapts the WIT-portable
//! `KeyEvent` shape before delegating here.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::catalog::Theme;

/// Self-contained state for the `/theme` picker modal.
///
/// In v0.8 these fields lived directly on `App`. The lift puts them in
/// one struct so PR 6's `Plugin` impl can wrap a single value rather
/// than four scattered fields.
#[derive(Debug)]
pub(crate) struct ThemePicker {
    /// Substring filter narrowing the catalog. Empty means "show every
    /// theme". Matched case-sensitively against [`Theme::name`] (the
    /// built-in name or upstream slug).
    pub(crate) filter: String,
    /// Index into the filtered theme list (`filtered_themes()`); not an
    /// index into the rendered rows, which include section-header
    /// decorations.
    pub(crate) cursor: usize,
    /// Snapshot of the theme that was active when the picker opened.
    /// Set once by `new`; never mutated. Used to restore on `Cancel`.
    pub(crate) pre_open_theme: Theme,
}

/// Result of [`ThemePicker::on_key`]. The caller (host key dispatch)
/// owns the side effects: applying the previewed theme, persisting on
/// commit, or restoring the pre-open snapshot on cancel.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PickerOutcome {
    /// No state change observable to the caller — picker stays open with
    /// no preview update.
    Stay,
    /// Update the host's `active_theme` to render with this palette on
    /// the next frame. Picker remains open.
    PreviewTheme(Theme),
    /// Apply + persist this theme and close the picker.
    Apply(Theme),
    /// Restore the pre-open theme and close the picker.
    Cancel,
}

impl ThemePicker {
    /// Open the picker with the active theme as both the pre-open
    /// snapshot and the initial cursor target. Filter starts empty.
    pub(crate) fn new(active: Theme) -> Self {
        let mut picker = Self {
            filter: String::new(),
            cursor: 0,
            pre_open_theme: active,
        };
        // Position the cursor on the active theme so the user starts on
        // their current choice.
        picker.cursor = picker
            .filtered_themes()
            .iter()
            .position(|t| *t == active)
            .unwrap_or(0);
        picker
    }

    /// Catalog filtered by case-sensitive substring match on
    /// [`Theme::name`]. Returns only selectable rows — section headers
    /// are render-time decoration handled in `ui.rs`.
    pub(crate) fn filtered_themes(&self) -> Vec<Theme> {
        let filter = self.filter.as_str();
        Theme::all()
            .into_iter()
            .filter(|t| t.name().contains(filter))
            .collect()
    }

    /// Dispatch a single key event. Returns the outcome the caller
    /// should act on; the picker mutates its own state in-place.
    pub(crate) fn on_key(&mut self, key: KeyEvent) -> PickerOutcome {
        match key.code {
            KeyCode::Esc => PickerOutcome::Cancel,
            KeyCode::Enter => {
                // Empty filter → stay open (caller does nothing). v0.8
                // edge case 2: Enter on a zero-match filter must NOT
                // commit the last-good preview.
                let candidates = self.filtered_themes();
                match candidates.get(self.cursor).copied() {
                    Some(t) => PickerOutcome::Apply(t),
                    None => PickerOutcome::Stay,
                }
            }
            KeyCode::Up => self.cursor_up(),
            KeyCode::Down => self.cursor_down(),
            KeyCode::Backspace => {
                if self.filter.pop().is_none() {
                    return PickerOutcome::Stay;
                }
                self.clamp_after_filter_change()
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.filter.push(c);
                self.clamp_after_filter_change()
            }
            _ => PickerOutcome::Stay,
        }
    }

    /// Move the cursor down one row, clamping at the last filtered
    /// theme. Returns a `PreviewTheme` outcome for the new cursor row,
    /// or `Stay` if the filter has zero matches.
    fn cursor_down(&mut self) -> PickerOutcome {
        let filtered = self.filtered_themes();
        if filtered.is_empty() {
            return PickerOutcome::Stay;
        }
        let last = filtered.len() - 1;
        if self.cursor < last {
            self.cursor += 1;
        }
        PickerOutcome::PreviewTheme(filtered[self.cursor])
    }

    /// Move the cursor up one row, clamping at the first filtered
    /// theme. Returns a `PreviewTheme` outcome for the new cursor row,
    /// or `Stay` if the filter has zero matches.
    fn cursor_up(&mut self) -> PickerOutcome {
        let filtered = self.filtered_themes();
        if filtered.is_empty() {
            return PickerOutcome::Stay;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        PickerOutcome::PreviewTheme(filtered[self.cursor])
    }

    /// After the filter mutates, clamp the cursor to the new filtered
    /// length and return the new preview. If the new filter matches
    /// zero themes, returns `Stay` (callers must not clobber the live
    /// preview — preserves the v0.8 last-good-preview invariant).
    fn clamp_after_filter_change(&mut self) -> PickerOutcome {
        let filtered = self.filtered_themes();
        if filtered.is_empty() {
            return PickerOutcome::Stay;
        }
        if self.cursor >= filtered.len() {
            self.cursor = filtered.len() - 1;
        }
        PickerOutcome::PreviewTheme(filtered[self.cursor])
    }
}
