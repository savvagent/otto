//! The `Screen` trait — per-open instances pushed onto the runtime's screen stack.

use async_trait::async_trait;

use crate::effect::Effect;
use crate::error::PluginError;
use crate::event::HostEvent;
use crate::styled::StyledLine;
use crate::types::{KeyEventPortable, Region};

/// A per-open screen instance pushed onto the runtime's screen stack via
/// `Effect::OpenScreen`. Owns its own state; popped by `Effect::CloseScreen`.
#[async_trait]
pub trait Screen: Send {
    /// Returns the screen id this instance was created for. Matches the
    /// `ScreenSpec::id` from the originating plugin's manifest. Returns an
    /// owned `String` rather than a borrow so the value crosses the
    /// plugin boundary without lifetime constraints (WIT-portability).
    fn id(&self) -> String;

    /// Render the screen's content lines for the given inner region. Chrome
    /// (borders, title, centering) is painted by the runtime around this
    /// content — never inside it. For `Fullscreen`/`BottomSheet` layouts,
    /// when [`Screen::tips`] returns a non-empty line, the runtime reserves
    /// that line's row for it *before* calling `render`, so `region` here
    /// already excludes it; `render` is free to fill the full region it's
    /// given without checking `tips()` itself.
    fn render(&self, region: Region) -> Vec<StyledLine>;

    /// Handle a key event while this screen is on top of the runtime's stack.
    /// Returned effects are applied after the call. Returning `Effect::CloseScreen`
    /// pops this screen off the stack.
    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError>;

    /// Optional: handle a `HostEvent` while this screen is open. Default impl
    /// returns no effects. Useful for screens that need to react to async work
    /// (e.g., transcript-list-ready notifications).
    async fn on_event(&mut self, event: HostEvent) -> Result<Vec<Effect>, PluginError> {
        let _ = event;
        Ok(vec![])
    }

    /// Optional tips line shown above the prompt while this screen is active.
    /// Default impl returns no tips. When non-empty, replaces the `home.tips` slot
    /// contributions for the duration of this screen's lifetime. For
    /// `Fullscreen`/`BottomSheet` layouts, only the first returned line is
    /// painted, on its own row at the bottom of the frame/sheet — that row is
    /// reserved out of the region passed to [`Screen::render`], not overlaid
    /// on top of it.
    fn tips(&self) -> Vec<StyledLine> {
        vec![]
    }

    /// Optional: the remainder of a predicted completion, rendered as dim
    /// "ghost" text immediately after the prompt's cursor. Returning `None`
    /// (the default) means: no ghost text. This is advisory only — it is
    /// never written into the prompt's editable buffer, so it can never be
    /// deleted, submitted, or otherwise treated as real input.
    ///
    /// `prompt` is the runtime's own authoritative view of what's currently
    /// on the prompt line (the same text driving the textarea render).
    /// Implementations must derive the returned suffix from `prompt`
    /// itself, not from separately-tracked internal state — a screen that
    /// computes the suffix length against its own filter/cursor state
    /// instead of `prompt` risks painting ghost text over real characters
    /// the moment that internal state drifts from what's actually
    /// on-screen. The runtime does not (and cannot generically) verify
    /// this on the implementer's behalf.
    fn ghost_completion(&self, prompt: &str) -> Option<String> {
        let _ = prompt;
        None
    }
}
