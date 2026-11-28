pub mod actions;
pub mod fuzzy;
pub mod palette;
mod view;

use gpui::{App, KeyBinding};

pub use actions::{ClosePalette, ToggleFinder};
pub use palette::{FuzzyPalette, PaletteBadge, PaletteConfig, PaletteEvent, PaletteItem};
pub use view::{FinderEvent, FinderItem, TypeFinder};

pub const FINDER_KEY_CONTEXT: &str = "TypeFinder";

pub fn bind_keys(cx: &mut App) {
  cx.bind_keys([
    KeyBinding::new("cmd-p", ToggleFinder, None),
    KeyBinding::new("escape", ClosePalette, Some(FINDER_KEY_CONTEXT)),
  ]);
}
