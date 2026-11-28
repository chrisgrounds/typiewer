use std::path::Path;

use graph::{TypeId, TypeKind, TypeNode, relative_display_path};
use palette::{
  BadgeTone, COMMAND_PALETTE_KEY_CONTEXT, FINDER_KEY_CONTEXT, PaletteBadge, PaletteConfig,
  PaletteItem,
};
use state::ViewerSettings;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppCommand {
  ToggleDotGrid,
  ToggleEdges,
  ToggleZoomBadge,
  OpenTypeFinder,
  ExpandAll,
  CollapseAllFiles,
  CollapseAllCrates,
  ZoomIn,
  ZoomOut,
  ResetZoom,
  ClearSelection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandCategory {
  Settings,
  View,
  Layout,
  Navigation,
  Selection,
}

impl CommandCategory {
  const fn label(self) -> &'static str {
    match self {
      Self::Settings => "Settings",
      Self::View => "View",
      Self::Layout => "Layout",
      Self::Navigation => "Navigation",
      Self::Selection => "Selection",
    }
  }

  const fn tone(self) -> BadgeTone {
    match self {
      Self::Settings => BadgeTone::Amber,
      Self::View => BadgeTone::Sky,
      Self::Layout => BadgeTone::Emerald,
      Self::Navigation => BadgeTone::Purple,
      Self::Selection => BadgeTone::Slate,
    }
  }
}

const fn type_kind_tone(kind: TypeKind) -> BadgeTone {
  match kind {
    TypeKind::Struct => BadgeTone::Sky,
    TypeKind::Enum => BadgeTone::Emerald,
    TypeKind::Trait => BadgeTone::Amber,
    TypeKind::Type => BadgeTone::Purple,
  }
}

pub const TYPE_FINDER_CONFIG: PaletteConfig = PaletteConfig {
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

pub const COMMAND_PALETTE_CONFIG: PaletteConfig = PaletteConfig {
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

pub fn build_type_finder_items(
  canonical_root: &Path,
  types: &[TypeNode],
) -> Vec<PaletteItem<TypeId>> {
  let mut entries: Vec<(&TypeNode, String, u32)> = types
    .iter()
    .map(|ty| {
      (
        ty,
        relative_display_path(canonical_root, &ty.file_path),
        ty.position.line + 1,
      )
    })
    .collect();

  entries.sort_by(|(a, a_path, a_line), (b, b_path, b_line)| {
    a.name
      .to_lowercase()
      .cmp(&b.name.to_lowercase())
      .then_with(|| a_path.cmp(b_path))
      .then_with(|| a_line.cmp(b_line))
  });

  entries
    .into_iter()
    .map(|(ty, rel_path, line)| PaletteItem {
      id: ty.id,
      title: ty.name.clone(),
      badge: Some(PaletteBadge::new(ty.kind.as_str(), type_kind_tone(ty.kind))),
      detail: format!("{rel_path}:{line}"),
      search_fallback: format!("{} {} {rel_path}", ty.kind, ty.name),
    })
    .collect()
}

fn command_item(
  id: AppCommand,
  category: CommandCategory,
  title: &str,
  detail: &str,
  keywords: &str,
) -> PaletteItem<AppCommand> {
  PaletteItem {
    id,
    title: title.to_owned(),
    badge: Some(PaletteBadge::new(category.label(), category.tone())),
    detail: detail.to_owned(),
    search_fallback: format!("{} {title} {keywords}", category.label()),
  }
}

const fn on_off(value: bool) -> &'static str {
  if value { "On" } else { "Off" }
}

pub fn build_command_items(settings: ViewerSettings) -> Vec<PaletteItem<AppCommand>> {
  vec![
    command_item(
      AppCommand::ToggleDotGrid,
      CommandCategory::Settings,
      "Toggle Dot Grid",
      on_off(settings.show_dot_grid),
      "background canvas dots grid",
    ),
    command_item(
      AppCommand::ToggleEdges,
      CommandCategory::Settings,
      "Toggle Function Edges",
      on_off(settings.show_edges),
      "arrows lines connections graph",
    ),
    command_item(
      AppCommand::ToggleZoomBadge,
      CommandCategory::Settings,
      "Toggle Zoom Indicator",
      on_off(settings.show_zoom_badge),
      "hud badge percentage",
    ),
    command_item(
      AppCommand::OpenTypeFinder,
      CommandCategory::Navigation,
      "Go to Type...",
      "⌘P",
      "find search jump struct enum trait",
    ),
    command_item(
      AppCommand::ExpandAll,
      CommandCategory::Layout,
      "Expand All Crates & Files",
      "",
      "unfold open show",
    ),
    command_item(
      AppCommand::CollapseAllFiles,
      CommandCategory::Layout,
      "Collapse All Files",
      "",
      "fold hide",
    ),
    command_item(
      AppCommand::CollapseAllCrates,
      CommandCategory::Layout,
      "Collapse All Crates",
      "",
      "fold hide",
    ),
    command_item(
      AppCommand::ZoomIn,
      CommandCategory::View,
      "Zoom In",
      "⌘+",
      "scale magnify",
    ),
    command_item(
      AppCommand::ZoomOut,
      CommandCategory::View,
      "Zoom Out",
      "⌘-",
      "scale shrink",
    ),
    command_item(
      AppCommand::ResetZoom,
      CommandCategory::View,
      "Reset Zoom & Pan",
      "⌘0",
      "camera center default 100%",
    ),
    command_item(
      AppCommand::ClearSelection,
      CommandCategory::Selection,
      "Clear Selection",
      "Esc",
      "deselect close inspector",
    ),
  ]
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use lsp::Position;

  use super::*;

  #[test]
  fn build_type_finder_items_computes_relative_path_and_sorts() {
    let root = Path::new("/repo");
    let types = vec![
      TypeNode {
        id: TypeId(0),
        name: "Camera".to_owned(),
        kind: TypeKind::Struct,
        body: String::new(),
        file_path: PathBuf::from("/repo/crates/state/src/camera.rs"),
        position: Position::new(4, 0),
      },
      TypeNode {
        id: TypeId(1),
        name: "Alpha".to_owned(),
        kind: TypeKind::Enum,
        body: String::new(),
        file_path: PathBuf::from("/repo/crates/graph/src/lib.rs"),
        position: Position::new(0, 0),
      },
    ];

    let items = build_type_finder_items(root, &types);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].title, "Alpha");
    assert_eq!(items[1].title, "Camera");
    assert_eq!(items[1].detail, "crates/state/src/camera.rs:5");
    assert_eq!(
      items[1].badge.as_ref().map(|b| (b.label.as_ref(), b.tone)),
      Some(("struct", BadgeTone::Sky))
    );
  }

  #[test]
  fn build_command_items_reflects_settings_and_badges() {
    let items = build_command_items(ViewerSettings {
      show_dot_grid: false,
      ..ViewerSettings::default()
    });
    assert_eq!(items[0].id, AppCommand::ToggleDotGrid);
    assert_eq!(items[0].detail, "Off");
    assert!(items.iter().all(|item| item.badge.is_some()));
  }
}
