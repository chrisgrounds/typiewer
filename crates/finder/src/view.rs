use std::path::Path;

use gpui::{
  App, Context, Entity, EventEmitter, FocusHandle, Focusable, Subscription, Window, prelude::*,
};
use graph::{TypeId, TypeNode};

use crate::FINDER_KEY_CONTEXT;
use crate::palette::{FuzzyPalette, PaletteBadge, PaletteConfig, PaletteEvent, PaletteItem};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinderEvent {
  SelectType(TypeId),
  Close,
}

#[derive(Clone, Debug)]
pub struct FinderItem {
  pub id: TypeId,
  pub name: String,
  pub kind: String,
  pub relative_path: String,
  pub line: u32,
}

impl FinderItem {
  pub fn from_type_node(canonical_root: &Path, node: &TypeNode) -> Self {
    let relative_path = node
      .file_path
      .strip_prefix(canonical_root)
      .unwrap_or(&node.file_path)
      .display()
      .to_string();
    Self {
      id: node.id,
      name: node.name.clone(),
      kind: node.kind.clone(),
      relative_path,
      line: node.position.line + 1,
    }
  }

  fn into_palette_item(self) -> PaletteItem<TypeId> {
    let (bg_rgba, border_rgba, fg_rgb) = kind_badge_colors(&self.kind);
    let detail = format!("{}:{}", self.relative_path, self.line);
    let search_fallback = format!("{} {} {}", self.kind, self.name, self.relative_path);
    PaletteItem {
      id: self.id,
      title: self.name,
      badge: Some(PaletteBadge {
        label: self.kind,
        bg_rgba,
        border_rgba,
        fg_rgb,
      }),
      detail,
      search_fallback,
    }
  }
}

fn kind_badge_colors(kind: &str) -> (u32, u32, u32) {
  match kind {
    "struct" => (0x38bdf822, 0x38bdf844, 0x7dd3fc),
    "enum" => (0x34d39922, 0x34d39944, 0x6ee7b7),
    "trait" => (0xfbbf2422, 0xfbbf2444, 0xfde68a),
    "type" => (0xc084fc22, 0xc084fc44, 0xd8b4fe),
    _ => (0x94a3b822, 0x94a3b844, 0xcbd5e1),
  }
}

const TYPE_FINDER_CONFIG: PaletteConfig = PaletteConfig {
  key_context: FINDER_KEY_CONTEXT,
  prompt_badge: "⌘P",
  placeholder: "Search types...",
  empty_message: "No matching types",
  item_singular: "type",
  item_plural: "types",
  footer_left: "↑↓ navigate  ·  ↵ jump to type  ·  esc close",
  footer_right: "fuzzy search by name, kind, or path",
  accent_rgb: 0x38bdf8,
  accent_border_rgba: 0x38bdf844,
  selected_bg_rgba: 0x38bdf81e,
  selected_border_rgba: 0x38bdf855,
  highlight_rgb: 0x7dd3fc,
};

pub struct TypeFinder {
  palette: Entity<FuzzyPalette<TypeId>>,
  _subscription: Subscription,
}

impl TypeFinder {
  pub fn new(canonical_root: &Path, types: &[TypeNode], cx: &mut Context<Self>) -> Self {
    let mut items: Vec<FinderItem> = types
      .iter()
      .map(|ty| FinderItem::from_type_node(canonical_root, ty))
      .collect();
    items.sort_by(|a, b| {
      a.name
        .to_lowercase()
        .cmp(&b.name.to_lowercase())
        .then_with(|| a.relative_path.cmp(&b.relative_path))
        .then_with(|| a.line.cmp(&b.line))
    });

    let palette_items = items
      .into_iter()
      .map(FinderItem::into_palette_item)
      .collect();
    let palette = cx.new(|cx| FuzzyPalette::new(TYPE_FINDER_CONFIG, palette_items, cx));
    let subscription = cx.subscribe(
      &palette,
      |_this, _palette, event: &PaletteEvent<TypeId>, cx| match *event {
        PaletteEvent::Select(id) => cx.emit(FinderEvent::SelectType(id)),
        PaletteEvent::Close => cx.emit(FinderEvent::Close),
      },
    );

    Self {
      palette,
      _subscription: subscription,
    }
  }

  pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.palette.update(cx, |palette, cx| {
      palette.reset(window, cx);
    });
  }
}

impl EventEmitter<FinderEvent> for TypeFinder {}

impl Focusable for TypeFinder {
  fn focus_handle(&self, cx: &App) -> FocusHandle {
    self.palette.read(cx).focus_handle(cx)
  }
}

impl Render for TypeFinder {
  fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
    self.palette.clone()
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use lsp::Position;

  use super::*;

  fn make_type(id: usize, name: &str, kind: &str, path: &str, line: u32) -> TypeNode {
    TypeNode {
      id: TypeId(id),
      name: name.to_owned(),
      kind: kind.to_owned(),
      body: String::new(),
      file_path: PathBuf::from(path),
      position: Position { line, character: 0 },
    }
  }

  #[test]
  fn finder_item_computes_relative_path_and_one_based_line() {
    let root = Path::new("/repo");
    let ty = make_type(0, "Camera", "struct", "/repo/crates/state/src/camera.rs", 4);
    let item = FinderItem::from_type_node(root, &ty);
    assert_eq!(item.relative_path, "crates/state/src/camera.rs");
    assert_eq!(item.line, 5);
    assert_eq!(item.name, "Camera");
    assert_eq!(item.kind, "struct");
  }
}
