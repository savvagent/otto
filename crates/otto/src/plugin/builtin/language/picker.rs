//! Self-contained state for the `/language` picker modal. Mirrors
//! `internal:themes::picker::ThemePicker`. PR 3 wraps this in a
//! `Screen` adapter (`screen.rs`).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::catalog::{Language, supported};

/// Picker state. Owned by the screen adapter (one per open).
#[derive(Debug)]
pub(crate) struct LanguagePicker {
    pub(crate) filter: String,
    pub(crate) cursor: usize,
    pub(crate) pre_open_code: &'static str,
}

/// Per-keystroke outcome surfaced to the screen adapter, which maps
/// each variant to an effect (or nothing).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PickerOutcome {
    Stay,
    PreviewLocale(&'static str),
    Apply(&'static str),
    Cancel,
}

impl LanguagePicker {
    pub(crate) fn new(pre_open_code: &'static str) -> Self {
        let mut p = Self {
            filter: String::new(),
            cursor: 0,
            pre_open_code,
        };
        p.cursor = p
            .filtered()
            .iter()
            .position(|l| l.code == pre_open_code)
            .unwrap_or(0);
        p
    }

    /// Catalog filtered by case-insensitive prefix match against either
    /// the language code or the English name. An empty filter matches
    /// every entry.
    pub(crate) fn filtered(&self) -> Vec<&'static Language> {
        let f = self.filter.to_ascii_lowercase();
        supported()
            .iter()
            .filter(|l| {
                l.code.to_ascii_lowercase().starts_with(&*f)
                    || l.english_name.to_ascii_lowercase().starts_with(&*f)
            })
            .collect()
    }

    pub(crate) fn on_key(&mut self, key: KeyEvent) -> PickerOutcome {
        match key.code {
            KeyCode::Esc => PickerOutcome::Cancel,
            KeyCode::Enter => {
                let candidates = self.filtered();
                match candidates.get(self.cursor) {
                    Some(l) => PickerOutcome::Apply(l.code),
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

    fn cursor_down(&mut self) -> PickerOutcome {
        let filtered = self.filtered();
        if filtered.is_empty() {
            return PickerOutcome::Stay;
        }
        let last = filtered.len() - 1;
        if self.cursor < last {
            self.cursor += 1;
        }
        PickerOutcome::PreviewLocale(filtered[self.cursor].code)
    }

    fn cursor_up(&mut self) -> PickerOutcome {
        let filtered = self.filtered();
        if filtered.is_empty() {
            return PickerOutcome::Stay;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        PickerOutcome::PreviewLocale(filtered[self.cursor].code)
    }

    fn clamp_after_filter_change(&mut self) -> PickerOutcome {
        let filtered = self.filtered();
        if filtered.is_empty() {
            return PickerOutcome::Stay;
        }
        if self.cursor >= filtered.len() {
            self.cursor = filtered.len() - 1;
        }
        PickerOutcome::PreviewLocale(filtered[self.cursor].code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker_starting_at(code: &'static str) -> LanguagePicker {
        LanguagePicker::new(code)
    }

    #[test]
    fn new_positions_cursor_on_pre_open_code() {
        let p = picker_starting_at("pt");
        assert_eq!(p.pre_open_code, "pt");
        let filtered = p.filtered();
        assert_eq!(filtered[p.cursor].code, "pt");
    }

    #[test]
    fn esc_emits_cancel() {
        let mut p = picker_starting_at("en");
        let out = p.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert_eq!(out, PickerOutcome::Cancel);
    }

    #[test]
    fn enter_emits_apply_for_current_cursor_row() {
        let mut p = picker_starting_at("en");
        // Move down twice; we expect to land on "pt" (en, es, pt, hi → idx 2).
        let _ = p.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        let _ = p.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        let out = p.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(out, PickerOutcome::Apply("pt"));
    }

    #[test]
    fn down_emits_preview_for_next_row() {
        let mut p = picker_starting_at("en");
        let out = p.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        assert_eq!(out, PickerOutcome::PreviewLocale("es"));
    }

    #[test]
    fn down_clamps_at_last_row() {
        let mut p = picker_starting_at("hi"); // last in catalog
        let out = p.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        assert_eq!(out, PickerOutcome::PreviewLocale("hi"));
    }

    #[test]
    fn up_clamps_at_first_row() {
        let mut p = picker_starting_at("en"); // first in catalog
        let out = p.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::empty()));
        assert_eq!(out, PickerOutcome::PreviewLocale("en"));
    }

    #[test]
    fn typed_char_filters_case_insensitive() {
        let mut p = picker_starting_at("en");
        let _ = p.on_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::empty()));
        // After "P", catalog should narrow to "pt" (Portuguese).
        let filtered = p.filtered();
        assert!(filtered.iter().all(|l| l.code == "pt"));
        assert_eq!(p.cursor, 0);
    }

    #[test]
    fn backspace_widens_filter_and_clamps() {
        let mut p = picker_starting_at("en");
        let _ = p.on_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::empty()));
        let out = p.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert!(p.filter.is_empty());
        assert_eq!(p.filtered().len(), supported().len());
        assert_eq!(out, PickerOutcome::PreviewLocale("en"));
    }

    #[test]
    fn enter_on_zero_match_filter_emits_stay() {
        let mut p = picker_starting_at("en");
        let _ = p.on_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::empty()));
        let _ = p.on_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::empty()));
        let out = p.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(out, PickerOutcome::Stay);
    }
}
