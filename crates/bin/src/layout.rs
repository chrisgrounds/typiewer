use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use gpui::{Point, SharedString, point};
use graph::{TypeId, TypeNode};

use crate::theme::{
  BASE_REM_SIZE, BODY_CHAR_WIDTH_REM, BODY_PAD_Y_REM, CARD_BODY_HORIZONTAL_PAD_REM,
  CARD_HEADER_HORIZONTAL_PAD_REM, CARD_WIDTH_CULL_SAFETY_FACTOR, CRATE_BODY_PAD_X_REM,
  CRATE_BODY_PAD_Y_REM, CRATE_GAP_Y, CRATE_HEADER_LINE_HEIGHT_REM, CRATE_HEADER_PAD_Y_REM,
  FILE_GAP_REM, HEADER_CHAR_WIDTH_REM, HEADER_LINE_HEIGHT_REM, HEADER_PAD_Y_REM,
  TYPE_BLOCK_PAD_Y_REM, TYPE_GAP_REM, TYPE_LINE_HEIGHT_REM,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollapsedTarget {
  Crate(usize),
  File(usize, usize),
}

#[derive(Clone, Copy, Debug)]
pub struct TypeLayout {
  /// Offset from the top of the card body's inner content area (excluding body top padding).
  pub offset_rem: f32,
  pub offset_px: f32,
  pub height_rem: f32,
  pub border_px: f32,
}

impl TypeLayout {
  pub fn screen_top(&self, body_content_top_y: f32, rem_size: f32) -> f32 {
    body_content_top_y + self.offset_rem * rem_size + self.offset_px
  }

  pub fn screen_height(&self, rem_size: f32) -> f32 {
    self.height_rem * rem_size + self.border_px
  }
}

pub struct TypeBlock {
  pub id: TypeId,
  pub header_line: SharedString,
  pub rest_body: Option<SharedString>,
  pub rest_line_count: usize,
}

pub struct FileNode {
  pub dir_prefix: SharedString,
  pub file_name: SharedString,
  pub types: Vec<TypeBlock>,
  pub type_layouts: Vec<TypeLayout>,
  pub collapsed: bool,
  /// Offset from the top of the parent crate body's inner content area.
  pub offset_rem: f32,
  pub offset_px: f32,
  /// Total height of the card in its current expanded/collapsed state.
  pub height_rem: f32,
  pub fixed_border_px: f32,
  /// Total height of the card body's inner content area (all type blocks + gaps).
  pub body_content_rem: f32,
  pub body_content_px: f32,
  /// Stable minimum width in rems so culling wide off-screen types doesn't cause horizontal jitter.
  pub min_width_rem: f32,
}

pub struct CrateNode {
  pub name: SharedString,
  pub rel_dir: SharedString,
  pub files: Vec<FileNode>,
  pub total_type_count: usize,
  pub collapsed: bool,
  pub position: Point<f32>,
  /// Total height of the crate box in its current expanded/collapsed state.
  pub height_rem: f32,
  pub fixed_border_px: f32,
  /// Total height of the crate body's inner content area (all file cards + gaps).
  pub body_content_rem: f32,
  pub body_content_px: f32,
  /// Stable minimum width in rems covering the crate header and all contained file cards.
  pub min_width_rem: f32,
}

impl FileNode {
  pub fn screen_top(&self, crate_body_content_top_y: f32, rem_size: f32) -> f32 {
    crate_body_content_top_y + self.offset_rem * rem_size + self.offset_px
  }

  pub fn screen_height(&self, rem_size: f32) -> f32 {
    self.height_rem * rem_size + self.fixed_border_px
  }

  pub fn body_content_top_y(&self, card_screen_y: f32, rem_size: f32) -> f32 {
    let header_and_top_border = (HEADER_PAD_Y_REM * 2.0 + HEADER_LINE_HEIGHT_REM) * rem_size + 2.0;
    card_screen_y + header_and_top_border + BODY_PAD_Y_REM * rem_size
  }

  fn header_height_rem() -> f32 {
    HEADER_PAD_Y_REM * 2.0 + HEADER_LINE_HEIGHT_REM
  }

  fn recompute_dimensions(&mut self) {
    let header_rem = Self::header_height_rem();
    if self.collapsed {
      self.height_rem = header_rem;
      self.fixed_border_px = 2.0;
    } else {
      self.height_rem = header_rem + BODY_PAD_Y_REM * 2.0 + self.body_content_rem;
      self.fixed_border_px = 3.0 + self.body_content_px;
    }
  }
}

impl CrateNode {
  pub fn screen_height(&self, rem_size: f32) -> f32 {
    self.height_rem * rem_size + self.fixed_border_px
  }

  pub fn estimated_cull_width(&self, rem_size: f32) -> f32 {
    (self.min_width_rem * CARD_WIDTH_CULL_SAFETY_FACTOR + 4.0) * rem_size
  }

  pub fn body_content_top_y(&self, crate_screen_y: f32, rem_size: f32) -> f32 {
    let header_and_top_border =
      (CRATE_HEADER_PAD_Y_REM * 2.0 + CRATE_HEADER_LINE_HEIGHT_REM) * rem_size + 2.0;
    crate_screen_y + header_and_top_border + CRATE_BODY_PAD_Y_REM * rem_size
  }

  fn header_height_rem() -> f32 {
    CRATE_HEADER_PAD_Y_REM * 2.0 + CRATE_HEADER_LINE_HEIGHT_REM
  }
}

/// Splits a type body into its first line (header) and remaining lines (rest_body),
/// returning `(header_line, rest_body, total_line_count, max_line_chars)`.
fn split_type_body(body: &str) -> (SharedString, Option<SharedString>, usize, usize) {
  let mut lines = body.lines();
  let Some(first) = lines.next() else {
    return (SharedString::from(" "), None, 1, 0);
  };

  let mut max_chars = first.chars().count();
  let header_line = if first.is_empty() {
    SharedString::from(" ")
  } else {
    SharedString::from(first.to_owned())
  };

  let mut rest = String::new();
  let mut rest_count: usize = 0;
  for line in lines {
    if rest_count > 0 {
      rest.push('\n');
    }
    max_chars = max_chars.max(line.chars().count());
    if line.is_empty() {
      rest.push(' ');
    } else {
      rest.push_str(line);
    }
    rest_count += 1;
  }

  let rest_body = if rest_count > 0 {
    Some(SharedString::from(rest))
  } else {
    None
  };

  (header_line, rest_body, 1 + rest_count, max_chars)
}

fn parse_cargo_package_name(cargo_toml: &Path) -> Option<String> {
  let contents = std::fs::read_to_string(cargo_toml).ok()?;
  let mut in_package = false;
  for raw_line in contents.lines() {
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') {
      continue;
    }
    if line.starts_with('[') {
      in_package = line == "[package]";
      continue;
    }
    if in_package
      && let Some(rest) = line.strip_prefix("name")
      && let Some(rest) = rest.trim_start().strip_prefix('=')
    {
      let value = rest.trim();
      let stripped = value
        .strip_prefix('"')
        .and_then(|s| s.split('"').next())
        .or_else(|| value.strip_prefix('\'').and_then(|s| s.split('\'').next()));
      if let Some(name) = stripped
        && !name.is_empty()
      {
        return Some(name.to_owned());
      }
    }
  }
  None
}

fn resolve_crate_info(
  file_path: &Path,
  canonical_root: &Path,
  cargo_cache: &mut HashMap<PathBuf, Option<String>>,
) -> (PathBuf, String) {
  let mut current = file_path.parent();
  while let Some(dir) = current {
    if !dir.starts_with(canonical_root) {
      break;
    }
    let pkg_name = cargo_cache
      .entry(dir.to_path_buf())
      .or_insert_with(|| {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
          parse_cargo_package_name(&manifest)
        } else {
          None
        }
      })
      .clone();

    if let Some(name) = pkg_name {
      return (dir.to_path_buf(), name);
    }
    if dir == canonical_root {
      break;
    }
    current = dir.parent();
  }

  let rel = file_path.strip_prefix(canonical_root).unwrap_or(file_path);
  let mut comps = rel.components();
  if let (Some(first), Some(second)) = (comps.next(), comps.next())
    && first.as_os_str() == "crates"
    && let Some(name) = second.as_os_str().to_str()
  {
    return (canonical_root.join(first).join(second), name.to_owned());
  }

  let fallback_name = canonical_root
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("workspace")
    .to_owned();
  (canonical_root.to_path_buf(), fallback_name)
}

pub struct LayoutResult {
  pub type_anchors: HashMap<TypeId, Point<f32>>,
  pub collapsed_targets: HashMap<TypeId, CollapsedTarget>,
}

/// Groups parsed types by crate and file path, then computes stacked crate/file layouts
/// and left-edge anchor points.
pub fn build_crate_nodes(
  canonical_root: &Path,
  types: Vec<TypeNode>,
) -> (Vec<CrateNode>, LayoutResult) {
  let mut grouped: BTreeMap<PathBuf, (String, BTreeMap<PathBuf, Vec<TypeNode>>)> = BTreeMap::new();
  let mut cargo_cache: HashMap<PathBuf, Option<String>> = HashMap::new();

  for ty in types {
    let (crate_dir, crate_name) =
      resolve_crate_info(&ty.file_path, canonical_root, &mut cargo_cache);
    let entry = grouped
      .entry(crate_dir)
      .or_insert_with(|| (crate_name, BTreeMap::new()));
    entry.1.entry(ty.file_path.clone()).or_default().push(ty);
  }

  let mut crates = Vec::with_capacity(grouped.len());

  for (crate_dir, (crate_name, files_map)) in grouped {
    let mut files = Vec::with_capacity(files_map.len());
    let mut max_file_width_rem: f32 = 0.0;
    let mut total_type_count: usize = 0;

    for (file_path, file_types) in files_map {
      total_type_count += file_types.len();
      let mut type_blocks = Vec::with_capacity(file_types.len());
      let mut type_layouts = Vec::with_capacity(file_types.len());
      let mut offset_rem: f32 = 0.0;
      let mut offset_px: f32 = 0.0;
      let mut max_body_line_chars: usize = 0;

      for (idx, type_node) in file_types.iter().enumerate() {
        let (header_line, rest_body, line_count, type_max_chars) = split_type_body(&type_node.body);
        max_body_line_chars = max_body_line_chars.max(type_max_chars);

        let lines = line_count as f32;
        let block_height_rem = TYPE_BLOCK_PAD_Y_REM * 2.0 + lines * TYPE_LINE_HEIGHT_REM;
        let block_border_px = 2.0;

        type_blocks.push(TypeBlock {
          id: type_node.id,
          header_line,
          rest_body,
          rest_line_count: line_count - 1,
        });
        type_layouts.push(TypeLayout {
          offset_rem,
          offset_px,
          height_rem: block_height_rem,
          border_px: block_border_px,
        });

        offset_rem += block_height_rem;
        offset_px += block_border_px;
        if idx + 1 < file_types.len() {
          offset_rem += TYPE_GAP_REM;
        }
      }

      let rel_to_crate = file_path
        .strip_prefix(&crate_dir)
        .or_else(|_| file_path.strip_prefix(canonical_root))
        .unwrap_or(&file_path);
      let file_name = rel_to_crate
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned();
      let dir_prefix = rel_to_crate
        .parent()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .map(|s| format!("{s}/"))
        .unwrap_or_default();

      let header_chars = dir_prefix.chars().count() + file_name.chars().count() + 12;
      let body_min_w_rem =
        max_body_line_chars as f32 * BODY_CHAR_WIDTH_REM + CARD_BODY_HORIZONTAL_PAD_REM;
      let header_min_w_rem =
        header_chars as f32 * HEADER_CHAR_WIDTH_REM + CARD_HEADER_HORIZONTAL_PAD_REM;
      let min_width_rem = body_min_w_rem.max(header_min_w_rem);
      max_file_width_rem = max_file_width_rem.max(min_width_rem);

      let mut file_node = FileNode {
        dir_prefix: SharedString::from(dir_prefix),
        file_name: SharedString::from(file_name),
        types: type_blocks,
        type_layouts,
        collapsed: false,
        offset_rem: 0.0,
        offset_px: 0.0,
        height_rem: 0.0,
        fixed_border_px: 0.0,
        body_content_rem: offset_rem,
        body_content_px: offset_px,
        min_width_rem,
      };
      file_node.recompute_dimensions();
      files.push(file_node);
    }

    let rel_dir = crate_dir
      .strip_prefix(canonical_root)
      .ok()
      .and_then(|p| p.to_str())
      .filter(|s| !s.is_empty())
      .unwrap_or_default()
      .to_owned();

    let crate_header_chars = crate_name.chars().count() + rel_dir.chars().count() + 24;
    let crate_header_min_w =
      crate_header_chars as f32 * HEADER_CHAR_WIDTH_REM + CARD_HEADER_HORIZONTAL_PAD_REM;
    let crate_min_width_rem =
      (max_file_width_rem + CRATE_BODY_PAD_X_REM * 2.0 + 0.25).max(crate_header_min_w);

    crates.push(CrateNode {
      name: SharedString::from(crate_name),
      rel_dir: SharedString::from(rel_dir),
      files,
      total_type_count,
      collapsed: false,
      position: point(0.0, 0.0),
      height_rem: 0.0,
      fixed_border_px: 0.0,
      body_content_rem: 0.0,
      body_content_px: 0.0,
      min_width_rem: crate_min_width_rem,
    });
  }

  let layout = recompute_layout(&mut crates);
  (crates, layout)
}

/// Recomputes crate positions, file offsets, box heights, edge anchors, and collapsed targets
/// after any crate or module is collapsed or expanded.
pub fn recompute_layout(crates: &mut [CrateNode]) -> LayoutResult {
  let mut type_anchors = HashMap::new();
  let mut collapsed_targets = HashMap::new();
  let mut cursor_y: f32 = 0.0;

  let crate_header_rem = CrateNode::header_height_rem();
  let file_left_x = CRATE_BODY_PAD_X_REM * BASE_REM_SIZE + 1.0;

  for (crate_idx, crate_node) in crates.iter_mut().enumerate() {
    crate_node.position = point(0.0, cursor_y);

    if crate_node.collapsed {
      crate_node.body_content_rem = 0.0;
      crate_node.body_content_px = 0.0;
      crate_node.height_rem = crate_header_rem;
      crate_node.fixed_border_px = 2.0;

      let crate_height = crate_node.screen_height(BASE_REM_SIZE);
      let anchor = point(0.0, cursor_y + crate_height * 0.5);
      for file in &crate_node.files {
        for ty in &file.types {
          type_anchors.insert(ty.id, anchor);
          collapsed_targets.insert(ty.id, CollapsedTarget::Crate(crate_idx));
        }
      }

      cursor_y += crate_height + CRATE_GAP_Y;
      continue;
    }

    let crate_body_top_y = crate_node.body_content_top_y(cursor_y, BASE_REM_SIZE);
    let mut offset_rem: f32 = 0.0;
    let mut offset_px: f32 = 0.0;
    let file_count = crate_node.files.len();

    for (file_idx, file) in crate_node.files.iter_mut().enumerate() {
      file.recompute_dimensions();
      file.offset_rem = offset_rem;
      file.offset_px = offset_px;

      let file_top_y = file.screen_top(crate_body_top_y, BASE_REM_SIZE);

      if file.collapsed {
        let file_height = file.screen_height(BASE_REM_SIZE);
        let anchor = point(file_left_x, file_top_y + file_height * 0.5);
        for ty in &file.types {
          type_anchors.insert(ty.id, anchor);
          collapsed_targets.insert(ty.id, CollapsedTarget::File(crate_idx, file_idx));
        }
      } else {
        let body_top_y = file.body_content_top_y(file_top_y, BASE_REM_SIZE);
        for (ty, layout) in file.types.iter().zip(&file.type_layouts) {
          let ty_top = layout.screen_top(body_top_y, BASE_REM_SIZE);
          let ty_height = layout.screen_height(BASE_REM_SIZE);
          type_anchors.insert(ty.id, point(file_left_x, ty_top + ty_height * 0.5));
        }
      }

      offset_rem += file.height_rem;
      offset_px += file.fixed_border_px;
      if file_idx + 1 < file_count {
        offset_rem += FILE_GAP_REM;
      }
    }

    crate_node.body_content_rem = offset_rem;
    crate_node.body_content_px = offset_px;
    crate_node.height_rem = crate_header_rem + CRATE_BODY_PAD_Y_REM * 2.0 + offset_rem;
    crate_node.fixed_border_px = 3.0 + offset_px;

    let crate_height = crate_node.screen_height(BASE_REM_SIZE);
    cursor_y += crate_height + CRATE_GAP_Y;
  }

  LayoutResult {
    type_anchors,
    collapsed_targets,
  }
}

/// Computes the contiguous range of visible file indices inside an expanded crate, plus top/bottom
/// spacer heights (in screen px) that preserve exact vertical alignment with edge anchors.
pub fn visible_file_slice(
  files: &[FileNode],
  crate_body_content_top_y: f32,
  body_content_rem: f32,
  body_content_px: f32,
  rem_size: f32,
  vp_min_y: f32,
  vp_max_y: f32,
) -> (std::ops::Range<usize>, f32, f32) {
  if files.is_empty() {
    return (0..0, 0.0, 0.0);
  }

  let first = files.partition_point(|file| {
    file.screen_top(crate_body_content_top_y, rem_size) + file.screen_height(rem_size) < vp_min_y
  });

  if first >= files.len() {
    let full = body_content_rem * rem_size + body_content_px;
    return (0..0, full, 0.0);
  }

  let last = first
    + files[first..]
      .partition_point(|file| file.screen_top(crate_body_content_top_y, rem_size) <= vp_max_y);

  if first == last {
    let full = body_content_rem * rem_size + body_content_px;
    return (0..0, full, 0.0);
  }

  let top_spacer_px = if first > 0 {
    let first_file = &files[first];
    ((first_file.offset_rem - FILE_GAP_REM) * rem_size + first_file.offset_px).max(0.0)
  } else {
    0.0
  };

  let bottom_spacer_px = if last < files.len() {
    let last_visible = &files[last - 1];
    let last_end_rem = last_visible.offset_rem + last_visible.height_rem;
    let last_end_px = last_visible.offset_px + last_visible.fixed_border_px;
    ((body_content_rem - last_end_rem - FILE_GAP_REM) * rem_size + (body_content_px - last_end_px))
      .max(0.0)
  } else {
    0.0
  };

  (first..last, top_spacer_px, bottom_spacer_px)
}

/// Computes the contiguous range of visible type indices inside a card, plus top/bottom spacer
/// heights (in screen px) that preserve exact vertical alignment with edge anchors.
pub fn visible_type_slice(
  type_layouts: &[TypeLayout],
  body_content_top_y: f32,
  body_content_rem: f32,
  body_content_px: f32,
  rem_size: f32,
  vp_min_y: f32,
  vp_max_y: f32,
) -> (std::ops::Range<usize>, f32, f32) {
  if type_layouts.is_empty() {
    return (0..0, 0.0, 0.0);
  }

  let first = type_layouts.partition_point(|layout| {
    layout.screen_top(body_content_top_y, rem_size) + layout.screen_height(rem_size) < vp_min_y
  });

  if first >= type_layouts.len() {
    let full = body_content_rem * rem_size + body_content_px;
    return (0..0, full, 0.0);
  }

  let last = first
    + type_layouts[first..]
      .partition_point(|layout| layout.screen_top(body_content_top_y, rem_size) <= vp_max_y);

  if first == last {
    let full = body_content_rem * rem_size + body_content_px;
    return (0..0, full, 0.0);
  }

  let top_spacer_px = if first > 0 {
    let first_layout = &type_layouts[first];
    ((first_layout.offset_rem - TYPE_GAP_REM) * rem_size + first_layout.offset_px).max(0.0)
  } else {
    0.0
  };

  let bottom_spacer_px = if last < type_layouts.len() {
    let last_visible = &type_layouts[last - 1];
    let last_end_rem = last_visible.offset_rem + last_visible.height_rem;
    let last_end_px = last_visible.offset_px + last_visible.border_px;
    ((body_content_rem - last_end_rem - TYPE_GAP_REM) * rem_size + (body_content_px - last_end_px))
      .max(0.0)
  } else {
    0.0
  };

  (first..last, top_spacer_px, bottom_spacer_px)
}

#[cfg(test)]
mod tests {
  use lsp::Position;

  use super::*;

  fn dummy_type(id: usize, name: &str, file_path: &str, body: &str) -> TypeNode {
    TypeNode {
      id: TypeId(id),
      name: name.to_owned(),
      kind: "struct".to_owned(),
      body: body.to_owned(),
      file_path: PathBuf::from(file_path),
      position: Position {
        line: 0,
        character: 0,
      },
    }
  }

  #[test]
  fn split_type_body_handles_empty_single_and_multiline() {
    let (h, r, lines, max_c) = split_type_body("");
    assert_eq!(h.as_ref(), " ");
    assert!(r.is_none());
    assert_eq!(lines, 1);
    assert_eq!(max_c, 0);

    let (h, r, lines, max_c) = split_type_body("pub struct Unit;");
    assert_eq!(h.as_ref(), "pub struct Unit;");
    assert!(r.is_none());
    assert_eq!(lines, 1);
    assert_eq!(max_c, 16);

    let (h, r, lines, max_c) = split_type_body("pub struct Pair {\n  a: u32,\n\n  b: String,\n}");
    assert_eq!(h.as_ref(), "pub struct Pair {");
    assert_eq!(r.as_deref(), Some("  a: u32,\n \n  b: String,\n}"));
    assert_eq!(lines, 5);
    assert_eq!(max_c, 17);
  }

  #[test]
  fn visible_type_slice_preserves_total_body_height_across_zooms() {
    // Build 10 type layouts with varying line counts.
    let line_counts = [2, 5, 1, 8, 3, 4, 6, 2, 7, 3];
    let mut layouts = Vec::new();
    let mut offset_rem = 0.0_f32;
    let mut offset_px = 0.0_f32;
    for (idx, &lines) in line_counts.iter().enumerate() {
      let height_rem = TYPE_BLOCK_PAD_Y_REM * 2.0 + lines as f32 * TYPE_LINE_HEIGHT_REM;
      let border_px = 2.0;
      layouts.push(TypeLayout {
        offset_rem,
        offset_px,
        height_rem,
        border_px,
      });
      offset_rem += height_rem;
      offset_px += border_px;
      if idx + 1 < line_counts.len() {
        offset_rem += TYPE_GAP_REM;
      }
    }
    let body_content_rem = offset_rem;
    let body_content_px = offset_px;

    for zoom in [0.25_f32, 1.0, 2.5] {
      let rem_size = BASE_REM_SIZE * zoom;
      let gap_px = TYPE_GAP_REM * rem_size;
      let full_height = body_content_rem * rem_size + body_content_px;

      // Position card so middle types intersect [0, 300]
      let body_top_y = -full_height * 0.35;
      let (range, top_spacer, bottom_spacer) = visible_type_slice(
        &layouts,
        body_top_y,
        body_content_rem,
        body_content_px,
        rem_size,
        0.0,
        300.0,
      );

      assert!(!range.is_empty());
      let mut child_heights: Vec<f32> = Vec::new();
      if top_spacer > 0.0 {
        child_heights.push(top_spacer);
      }
      for layout in &layouts[range.clone()] {
        child_heights.push(layout.screen_height(rem_size));
      }
      if bottom_spacer > 0.0 {
        child_heights.push(bottom_spacer);
      }
      let reconstructed: f32 =
        child_heights.iter().sum::<f32>() + (child_heights.len().saturating_sub(1) as f32) * gap_px;

      assert!(
        (reconstructed - full_height).abs() < 1e-2,
        "zoom={zoom}: reconstructed={reconstructed} != full_height={full_height}"
      );

      // Also verify the first visible type lands at its exact original Y coordinate.
      let expected_first_y = layouts[range.start].screen_top(body_top_y, rem_size);
      let actual_first_y = if top_spacer > 0.0 {
        body_top_y + top_spacer + gap_px
      } else {
        body_top_y
      };
      assert!(
        (actual_first_y - expected_first_y).abs() < 1e-2,
        "zoom={zoom}: actual_first_y={actual_first_y} != expected_first_y={expected_first_y}"
      );
    }
  }

  #[test]
  fn build_crate_nodes_groups_by_crate_and_recomputes_on_collapse() {
    let root = PathBuf::from("/workspace");
    let types = vec![
      dummy_type(
        0,
        "A",
        "/workspace/crates/alpha/src/a.rs",
        "pub struct A {\n  x: u32,\n  y: u32,\n}",
      ),
      dummy_type(1, "B", "/workspace/crates/alpha/src/a.rs", "pub struct B;"),
      dummy_type(2, "C", "/workspace/crates/alpha/src/b.rs", "pub struct C;"),
      dummy_type(3, "D", "/workspace/crates/beta/src/lib.rs", "pub struct D;"),
    ];

    let (mut crates, initial) = build_crate_nodes(&root, types);
    assert_eq!(crates.len(), 2);
    assert_eq!(crates[0].name.as_ref(), "alpha");
    assert_eq!(crates[0].rel_dir.as_ref(), "crates/alpha");
    assert_eq!(crates[0].files.len(), 2);
    assert_eq!(crates[0].files[0].dir_prefix.as_ref(), "src/");
    assert_eq!(crates[0].files[0].file_name.as_ref(), "a.rs");
    assert_eq!(crates[1].name.as_ref(), "beta");

    let initial_beta_y = crates[1].position.y;
    assert!(initial.collapsed_targets.is_empty());
    assert_ne!(
      initial.type_anchors.get(&TypeId(0)),
      initial.type_anchors.get(&TypeId(1))
    );

    // Collapsing the first file shifts beta up and merges anchors for types in that file.
    crates[0].files[0].collapsed = true;
    let after_file_collapse = recompute_layout(&mut crates);
    assert!(crates[1].position.y < initial_beta_y);
    assert_eq!(
      after_file_collapse.type_anchors.get(&TypeId(0)),
      after_file_collapse.type_anchors.get(&TypeId(1))
    );
    assert_eq!(
      after_file_collapse.collapsed_targets.get(&TypeId(0)),
      Some(&CollapsedTarget::File(0, 0))
    );
    assert_eq!(after_file_collapse.collapsed_targets.get(&TypeId(2)), None);

    // Collapsing the entire alpha crate shifts beta up further and merges all alpha anchors at x=0.
    let beta_y_after_file = crates[1].position.y;
    crates[0].collapsed = true;
    let after_crate_collapse = recompute_layout(&mut crates);
    assert!(crates[1].position.y < beta_y_after_file);
    assert_eq!(
      after_crate_collapse.type_anchors.get(&TypeId(0)),
      after_crate_collapse.type_anchors.get(&TypeId(2))
    );
    assert_eq!(
      after_crate_collapse
        .type_anchors
        .get(&TypeId(0))
        .map(|p| p.x),
      Some(0.0)
    );
    assert_eq!(
      after_crate_collapse.collapsed_targets.get(&TypeId(2)),
      Some(&CollapsedTarget::Crate(0))
    );
  }
}
