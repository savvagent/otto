//! Replacement for the v0.8 `InputMode` flat-field state machine.
//!
//! The stack owns `(Box<dyn Screen>, ScreenLayout)` pairs. Effect::OpenScreen
//! pushes; Effect::CloseScreen pops. Key events route to the top.

use otto_plugin::{Screen, ScreenLayout};

/// LIFO stack of active screens paired with their layout descriptors.
///
/// The layout travels with the screen so the runtime can paint the correct
/// chrome (border, title, centred modal overlay) around each screen's inner
/// content without the screen needing to know about terminal geometry.
pub struct ScreenStack {
    stack: Vec<(Box<dyn Screen>, ScreenLayout)>,
}

impl ScreenStack {
    /// Returns an empty `ScreenStack`.
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Pushes `screen` and its associated `layout` onto the top of the stack.
    ///
    /// Key events and render calls are routed to the new top screen until it
    /// is popped.
    pub fn push(&mut self, screen: Box<dyn Screen>, layout: ScreenLayout) {
        self.stack.push((screen, layout));
    }

    /// Removes and returns the top `(screen, layout)` pair, or `None` when the
    /// stack is empty.
    pub fn pop(&mut self) -> Option<(Box<dyn Screen>, ScreenLayout)> {
        self.stack.pop()
    }

    /// Returns a shared reference to the top screen and its layout, or `None`
    /// when the stack is empty.
    pub fn top(&self) -> Option<(&dyn Screen, &ScreenLayout)> {
        self.stack.last().map(|(s, l)| (s.as_ref(), l))
    }

    /// Returns mutable access to the top screen box and a shared reference to
    /// its layout, or `None` when the stack is empty.
    pub fn top_mut(&mut self) -> Option<(&mut Box<dyn Screen>, &ScreenLayout)> {
        self.stack.last_mut().map(|(s, l)| (s, &*l))
    }

    /// Returns the number of screens currently on the stack.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Returns `true` when no screens are on the stack.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

impl Default for ScreenStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use otto_plugin::{
        Effect, KeyEventPortable, PluginError, Region, ScreenLayout, StyledLine,
    };

    struct DummyScreen(String);

    #[async_trait]
    impl Screen for DummyScreen {
        fn id(&self) -> String {
            self.0.clone()
        }
        fn render(&self, _: Region) -> Vec<StyledLine> {
            vec![]
        }
        async fn on_key(&mut self, _: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
            Ok(vec![])
        }
    }

    fn layout() -> ScreenLayout {
        ScreenLayout::Fullscreen { hide_chrome: false }
    }

    #[test]
    fn push_pop_round_trip() {
        let mut s = ScreenStack::new();
        assert!(s.is_empty());
        s.push(Box::new(DummyScreen("a".into())), layout());
        s.push(Box::new(DummyScreen("b".into())), layout());
        assert_eq!(s.depth(), 2);
        let (top, _) = s.top().unwrap();
        assert_eq!(top.id(), "b");
        let popped = s.pop().unwrap();
        assert_eq!(popped.0.id(), "b");
        let (top, _) = s.top().unwrap();
        assert_eq!(top.id(), "a");
    }

    #[test]
    fn empty_pop_returns_none() {
        let mut s = ScreenStack::new();
        assert!(s.pop().is_none());
    }

    #[test]
    fn push_preserves_distinct_layouts() {
        let mut s = ScreenStack::new();
        s.push(
            Box::new(DummyScreen("a".into())),
            ScreenLayout::Fullscreen { hide_chrome: false },
        );
        s.push(
            Box::new(DummyScreen("b".into())),
            ScreenLayout::CenteredModal {
                width_pct: 60,
                height_pct: 50,
                title: Some("Test".into()),
            },
        );
        let (_, top_layout) = s.top().unwrap();
        assert!(matches!(
            top_layout,
            ScreenLayout::CenteredModal { width_pct: 60, .. }
        ));
        s.pop();
        let (_, top_layout) = s.top().unwrap();
        assert!(matches!(
            top_layout,
            ScreenLayout::Fullscreen { hide_chrome: false }
        ));
    }
}
