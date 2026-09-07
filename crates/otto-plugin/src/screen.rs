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
    /// (borders, title, centering) is painted by the runtime around this content.
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
    /// contributions for the duration of this screen's lifetime.
    fn tips(&self) -> Vec<StyledLine> {
        vec![]
    }
}
