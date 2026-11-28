pub mod actions;

use finder::{ClosePalette, FuzzyPalette, PaletteBadge, PaletteConfig, PaletteEvent, PaletteItem};
use gpui::{
  App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, Subscription, Window,
  prelude::*,
};

pub use actions::ToggleCommandPalette;

pub const COMMAND_PALETTE_KEY_CONTEXT: &str = "CommandPalette";

pub fn bind_keys(cx: &mut App) {
  cx.bind_keys([
    KeyBinding::new("cmd-shift-p", ToggleCommandPalette, None),
    KeyBinding::new("escape", ClosePalette, Some(COMMAND_PALETTE_KEY_CONTEXT)),
  ]);
}

#[derive(Clone, Debug)]
pub enum CommandPaletteEvent<T> {
  RunCommand(T),
  Close,
}

#[derive(Clone, Debug)]
pub struct CommandItem<T> {
  pub id: T,
  pub title: String,
  pub category: String,
  pub detail: String,
  pub keywords: String,
}

impl<T> CommandItem<T> {
  pub fn new(id: T, category: impl Into<String>, title: impl Into<String>) -> Self {
    Self {
      id,
      title: title.into(),
      category: category.into(),
      detail: String::new(),
      keywords: String::new(),
    }
  }

  pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
    self.detail = detail.into();
    self
  }

  pub fn with_keywords(mut self, keywords: impl Into<String>) -> Self {
    self.keywords = keywords.into();
    self
  }

  fn into_palette_item(self) -> PaletteItem<T> {
    let (bg_rgba, border_rgba, fg_rgb) = category_badge_colors(&self.category);
    let search_fallback = format!("{} {} {}", self.category, self.title, self.keywords);
    PaletteItem {
      id: self.id,
      title: self.title,
      badge: Some(PaletteBadge {
        label: self.category,
        bg_rgba,
        border_rgba,
        fg_rgb,
      }),
      detail: self.detail,
      search_fallback,
    }
  }
}

fn category_badge_colors(category: &str) -> (u32, u32, u32) {
  match category.to_ascii_lowercase().as_str() {
    "settings" => (0xfbbf2422, 0xfbbf2444, 0xfde68a),
    "view" => (0x38bdf822, 0x38bdf844, 0x7dd3fc),
    "layout" => (0x34d39922, 0x34d39944, 0x6ee7b7),
    "navigation" => (0xc084fc22, 0xc084fc44, 0xd8b4fe),
    _ => (0x94a3b822, 0x94a3b844, 0xcbd5e1),
  }
}

const COMMAND_PALETTE_CONFIG: PaletteConfig = PaletteConfig {
  key_context: COMMAND_PALETTE_KEY_CONTEXT,
  prompt_badge: ">",
  placeholder: "Run a command or change a setting...",
  empty_message: "No matching commands",
  item_singular: "command",
  item_plural: "commands",
  footer_left: "↑↓ navigate  ·  ↵ run command  ·  esc close",
  footer_right: "fuzzy search commands & settings",
  accent_rgb: 0xfbbf24,
  accent_border_rgba: 0xfbbf2444,
  selected_bg_rgba: 0xfbbf241e,
  selected_border_rgba: 0xfbbf2455,
  highlight_rgb: 0xfde68a,
};

pub struct CommandPalette<T: Clone + 'static> {
  palette: Entity<FuzzyPalette<T>>,
  _subscription: Subscription,
}

impl<T: Clone + 'static> CommandPalette<T> {
  pub fn new(commands: Vec<CommandItem<T>>, cx: &mut Context<Self>) -> Self {
    let palette_items = commands
      .into_iter()
      .map(CommandItem::into_palette_item)
      .collect();
    let palette = cx.new(|cx| FuzzyPalette::new(COMMAND_PALETTE_CONFIG, palette_items, cx));
    let subscription =
      cx.subscribe(
        &palette,
        |_this, _palette, event: &PaletteEvent<T>, cx| match event {
          PaletteEvent::Select(id) => cx.emit(CommandPaletteEvent::RunCommand(id.clone())),
          PaletteEvent::Close => cx.emit(CommandPaletteEvent::Close),
        },
      );

    Self {
      palette,
      _subscription: subscription,
    }
  }

  pub fn set_commands(&mut self, commands: Vec<CommandItem<T>>, cx: &mut Context<Self>) {
    let palette_items = commands
      .into_iter()
      .map(CommandItem::into_palette_item)
      .collect();
    self.palette.update(cx, |palette, cx| {
      palette.set_items(palette_items, cx);
    });
  }

  pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.palette.update(cx, |palette, cx| {
      palette.reset(window, cx);
    });
  }
}

impl<T: Clone + 'static> EventEmitter<CommandPaletteEvent<T>> for CommandPalette<T> {}

impl<T: Clone + 'static> Focusable for CommandPalette<T> {
  fn focus_handle(&self, cx: &App) -> FocusHandle {
    self.palette.read(cx).focus_handle(cx)
  }
}

impl<T: Clone + 'static> Render for CommandPalette<T> {
  fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
    self.palette.clone()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn command_item_builds_palette_item_with_category_badge_and_fallback() {
    let item = CommandItem::new("toggle_grid", "Settings", "Toggle Dot Grid")
      .with_detail("On")
      .with_keywords("background canvas");

    let palette_item = item.into_palette_item();
    assert_eq!(palette_item.id, "toggle_grid");
    assert_eq!(palette_item.title, "Toggle Dot Grid");
    assert_eq!(palette_item.detail, "On");
    assert!(palette_item.badge.is_some());
    assert!(palette_item.search_fallback.contains("background canvas"));
  }
}
