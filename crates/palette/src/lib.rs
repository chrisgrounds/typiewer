pub mod fuzzy;
pub mod palette;

use gpui::{App, KeyBinding};

pub use fuzzy::{FuzzyMatch, fuzzy_match};
pub use palette::{
  BadgeTone, FuzzyPalette, PaletteBadge, PaletteConfig, PaletteEvent, PaletteItem,
};

gpui::actions!(palette, [ToggleFinder, ToggleCommandPalette, ClosePalette]);

pub const FINDER_KEY_CONTEXT: &str = "TypeFinder";
pub const COMMAND_PALETTE_KEY_CONTEXT: &str = "CommandPalette";

pub fn bind_keys(cx: &mut App) {
  cx.bind_keys([
    KeyBinding::new("cmd-p", ToggleFinder, None),
    KeyBinding::new("cmd-shift-p", ToggleCommandPalette, None),
    KeyBinding::new("escape", ClosePalette, Some(FINDER_KEY_CONTEXT)),
    KeyBinding::new("escape", ClosePalette, Some(COMMAND_PALETTE_KEY_CONTEXT)),
  ]);
}
