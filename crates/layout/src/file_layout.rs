use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use graph::{EdgeId, Graph, TypeId, TypeNode};

use crate::edges;
use crate::geometry::{EdgeCurve, Point2, Rect, RemPx, VerticalSpan, point2};
use crate::metrics::{
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

pub struct TypeBlock {
  pub id: TypeId,
  pub header_line: Arc<str>,
  pub rest_body: Option<Arc<str>>,
  pub rest_line_count: usize,
  pub span: VerticalSpan,
}

pub struct FileNode {
  pub dir_prefix: Arc<str>,
  pub file_name: Arc<str>,
  pub types: Vec<TypeBlock>,
  pub collapsed: bool,
  /// Vertical offset and total height within the parent crate's body content area.
  pub span: VerticalSpan,
  /// Total height of the card body's inner content area (all type blocks + gaps).
  pub body_content: RemPx,
  /// Stable minimum width in rems so culling wide off-screen types doesn't cause horizontal jitter.
  pub min_width_rem: f32,
}

pub struct CrateNode {
  pub name: Arc<str>,
  pub rel_dir: Arc<str>,
  pub files: Vec<FileNode>,
  pub total_type_count: usize,
  pub collapsed: bool,
  pub position: Point2,
  /// Total height of the crate box in its current expanded/collapsed state.
  pub height: RemPx,
  /// Total height of the crate body's inner content area (all file cards + gaps).
  pub body_content: RemPx,
  /// Stable minimum width in rems covering the crate header and all contained file cards.
  pub min_width_rem: f32,
}

fn body_content_top(
  card_top_y: f32,
  header_height_rem: f32,
  body_pad_y_rem: f32,
  rem_size: f32,
) -> f32 {
  card_top_y + RemPx::new(header_height_rem + body_pad_y_rem, 2.0).to_px(rem_size)
}

impl FileNode {
  pub fn screen_top(&self, crate_body_content_top_y: f32, rem_size: f32) -> f32 {
    self.span.screen_top(crate_body_content_top_y, rem_size)
  }

  pub fn screen_height(&self, rem_size: f32) -> f32 {
    self.span.screen_height(rem_size)
  }

  pub fn body_content_top_y(&self, card_screen_y: f32, rem_size: f32) -> f32 {
    body_content_top(card_screen_y, Self::header_height_rem(), BODY_PAD_Y_REM, rem_size)
  }

  pub fn visible_types(
    &self,
    body_content_top_y: f32,
    rem_size: f32,
    vp_y: (f32, f32),
  ) -> (Range<usize>, f32, f32) {
    visible_slice(
      &self.types,
      |ty| ty.span,
      body_content_top_y,
      self.body_content,
      TYPE_GAP_REM,
      rem_size,
      vp_y,
    )
  }

  const fn header_height_rem() -> f32 {
    HEADER_PAD_Y_REM * 2.0 + HEADER_LINE_HEIGHT_REM
  }

  fn recompute_dimensions(&mut self) {
    let header_rem = Self::header_height_rem();
    self.span.height = if self.collapsed {
      RemPx::new(header_rem, 2.0)
    } else {
      RemPx::new(header_rem + BODY_PAD_Y_REM * 2.0, 3.0) + self.body_content
    };
  }
}

impl CrateNode {
  pub fn screen_height(&self, rem_size: f32) -> f32 {
    self.height.to_px(rem_size)
  }

  pub fn estimated_cull_width(&self, rem_size: f32) -> f32 {
    (self.min_width_rem * CARD_WIDTH_CULL_SAFETY_FACTOR + 4.0) * rem_size
  }

  pub fn body_content_top_y(&self, crate_screen_y: f32, rem_size: f32) -> f32 {
    body_content_top(
      crate_screen_y,
      Self::header_height_rem(),
      CRATE_BODY_PAD_Y_REM,
      rem_size,
    )
  }

  pub fn visible_files(
    &self,
    crate_body_content_top_y: f32,
    rem_size: f32,
    vp_y: (f32, f32),
  ) -> (Range<usize>, f32, f32) {
    visible_slice(
      &self.files,
      |file| file.span,
      crate_body_content_top_y,
      self.body_content,
      FILE_GAP_REM,
      rem_size,
      vp_y,
    )
  }

  const fn header_height_rem() -> f32 {
    CRATE_HEADER_PAD_Y_REM * 2.0 + CRATE_HEADER_LINE_HEIGHT_REM
  }
}

pub struct WorkspaceLayout {
  pub crates: Vec<CrateNode>,
  pub type_anchors: HashMap<TypeId, Point2>,
  pub collapsed_targets: HashMap<TypeId, CollapsedTarget>,
  type_locations: HashMap<TypeId, (usize, usize)>,
}

impl WorkspaceLayout {
  /// Groups parsed types by crate and file path, then computes stacked crate/file layouts
  /// and left-edge anchor points.
  pub fn build(canonical_root: &Path, types: &[TypeNode]) -> Self {
    let mut grouped: BTreeMap<PathBuf, (String, BTreeMap<PathBuf, Vec<&TypeNode>>)> =
      BTreeMap::new();
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
    let mut type_locations = HashMap::with_capacity(types.len());

    for (crate_idx, (crate_dir, (crate_name, files_map))) in grouped.into_iter().enumerate() {
      let mut files = Vec::with_capacity(files_map.len());
      let mut max_file_width_rem: f32 = 0.0;
      let mut total_type_count: usize = 0;

      for (file_idx, (file_path, file_types)) in files_map.into_iter().enumerate() {
        total_type_count += file_types.len();
        let mut type_blocks = Vec::with_capacity(file_types.len());
        let mut offset = RemPx::ZERO;
        let mut max_body_line_chars: usize = 0;

        for (idx, type_node) in file_types.iter().enumerate() {
          type_locations.insert(type_node.id, (crate_idx, file_idx));
          let (header_line, rest_body, line_count, type_max_chars) =
            split_type_body(&type_node.body);
          max_body_line_chars = max_body_line_chars.max(type_max_chars);

          let height = RemPx::new(
            TYPE_BLOCK_PAD_Y_REM * 2.0 + line_count as f32 * TYPE_LINE_HEIGHT_REM,
            2.0,
          );

          type_blocks.push(TypeBlock {
            id: type_node.id,
            header_line,
            rest_body,
            rest_line_count: line_count - 1,
            span: VerticalSpan { offset, height },
          });

          offset += height;
          if idx + 1 < file_types.len() {
            offset += RemPx::rems(TYPE_GAP_REM);
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
          dir_prefix: Arc::from(dir_prefix),
          file_name: Arc::from(file_name),
          types: type_blocks,
          collapsed: true,
          span: VerticalSpan::default(),
          body_content: offset,
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
        name: Arc::from(crate_name),
        rel_dir: Arc::from(rel_dir),
        files,
        total_type_count,
        collapsed: true,
        position: point2(0.0, 0.0),
        height: RemPx::ZERO,
        body_content: RemPx::ZERO,
        min_width_rem: crate_min_width_rem,
      });
    }

    let mut layout = Self {
      crates,
      type_anchors: HashMap::with_capacity(types.len()),
      collapsed_targets: HashMap::new(),
      type_locations,
    };
    layout.recompute();
    layout
  }

  /// Recomputes crate positions, file offsets, box heights, edge anchors, and collapsed targets
  /// after any crate or module is collapsed or expanded.
  pub fn recompute(&mut self) {
    self.type_anchors.clear();
    self.collapsed_targets.clear();

    let mut cursor_y: f32 = 0.0;
    let crate_header_rem = CrateNode::header_height_rem();
    let file_left_x = CRATE_BODY_PAD_X_REM * BASE_REM_SIZE + 1.0;

    for (crate_idx, crate_node) in self.crates.iter_mut().enumerate() {
      crate_node.position = point2(0.0, cursor_y);

      if crate_node.collapsed {
        crate_node.body_content = RemPx::ZERO;
        crate_node.height = RemPx::new(crate_header_rem, 2.0);

        let crate_height = crate_node.screen_height(BASE_REM_SIZE);
        let anchor = point2(0.0, cursor_y + crate_height * 0.5);
        for file in &crate_node.files {
          for ty in &file.types {
            self.type_anchors.insert(ty.id, anchor);
            self
              .collapsed_targets
              .insert(ty.id, CollapsedTarget::Crate(crate_idx));
          }
        }

        cursor_y += crate_height + CRATE_GAP_Y;
        continue;
      }

      let crate_body_top_y = crate_node.body_content_top_y(cursor_y, BASE_REM_SIZE);
      let mut offset = RemPx::ZERO;
      let file_count = crate_node.files.len();

      for (file_idx, file) in crate_node.files.iter_mut().enumerate() {
        file.recompute_dimensions();
        file.span.offset = offset;

        let file_top_y = file.screen_top(crate_body_top_y, BASE_REM_SIZE);

        if file.collapsed {
          let file_height = file.screen_height(BASE_REM_SIZE);
          let anchor = point2(file_left_x, file_top_y + file_height * 0.5);
          for ty in &file.types {
            self.type_anchors.insert(ty.id, anchor);
            self
              .collapsed_targets
              .insert(ty.id, CollapsedTarget::File(crate_idx, file_idx));
          }
        } else {
          let body_top_y = file.body_content_top_y(file_top_y, BASE_REM_SIZE);
          for ty in &file.types {
            let ty_top = ty.span.screen_top(body_top_y, BASE_REM_SIZE);
            let ty_height = ty.span.screen_height(BASE_REM_SIZE);
            self
              .type_anchors
              .insert(ty.id, point2(file_left_x, ty_top + ty_height * 0.5));
          }
        }

        offset += file.span.height;
        if file_idx + 1 < file_count {
          offset += RemPx::rems(FILE_GAP_REM);
        }
      }

      crate_node.body_content = offset;
      crate_node.height = RemPx::new(crate_header_rem + CRATE_BODY_PAD_Y_REM * 2.0, 3.0) + offset;

      cursor_y += crate_node.screen_height(BASE_REM_SIZE) + CRATE_GAP_Y;
    }
  }

  pub fn type_anchor(&self, type_id: TypeId) -> Option<Point2> {
    self.type_anchors.get(&type_id).copied()
  }

  /// Returns the screen-space `(right_x, top_y)` point at the right edge of the crate box
  /// aligned with the top of `type_id`, or `None` if the type is inside a collapsed crate/file.
  pub fn type_right_anchor(
    &self,
    type_id: TypeId,
    pan: Point2,
    zoom: f32,
    rem_size: f32,
  ) -> Option<Point2> {
    let &(crate_idx, file_idx) = self.type_locations.get(&type_id)?;
    let crate_node = self.crates.get(crate_idx)?;
    if crate_node.collapsed {
      return None;
    }
    let file_node = crate_node.files.get(file_idx)?;
    if file_node.collapsed {
      return None;
    }
    let type_block = file_node.types.iter().find(|ty| ty.id == type_id)?;

    let crate_screen_x = crate_node.position.x * zoom + pan.x;
    let crate_screen_y = crate_node.position.y * zoom + pan.y;
    let crate_body_top_y = crate_node.body_content_top_y(crate_screen_y, rem_size);
    let file_screen_y = file_node.screen_top(crate_body_top_y, rem_size);
    let body_content_top_y = file_node.body_content_top_y(file_screen_y, rem_size);
    let type_top_y = type_block.span.screen_top(body_content_top_y, rem_size);
    let crate_right_x = crate_screen_x + crate_node.min_width_rem * rem_size + 2.0;

    Some(point2(crate_right_x, type_top_y))
  }

  pub fn toggle_crate(&mut self, crate_idx: usize) -> bool {
    let Some(crate_node) = self.crates.get_mut(crate_idx) else {
      return false;
    };
    crate_node.collapsed = !crate_node.collapsed;
    self.recompute();
    true
  }

  pub fn toggle_file(&mut self, crate_idx: usize, file_idx: usize) -> bool {
    let Some(file_node) = self
      .crates
      .get_mut(crate_idx)
      .and_then(|c| c.files.get_mut(file_idx))
    else {
      return false;
    };
    file_node.collapsed = !file_node.collapsed;
    self.recompute();
    true
  }

  pub fn expand_all(&mut self) {
    for crate_node in &mut self.crates {
      crate_node.collapsed = false;
      for file_node in &mut crate_node.files {
        file_node.collapsed = false;
      }
    }
    self.recompute();
  }

  pub fn collapse_all_files(&mut self) {
    for crate_node in &mut self.crates {
      for file_node in &mut crate_node.files {
        file_node.collapsed = true;
      }
    }
    self.recompute();
  }

  pub fn collapse_all_crates(&mut self) {
    for crate_node in &mut self.crates {
      crate_node.collapsed = true;
    }
    self.recompute();
  }

  pub fn ensure_type_expanded(&mut self, type_id: TypeId) -> bool {
    let Some(&(crate_idx, file_idx)) = self.type_locations.get(&type_id) else {
      return false;
    };
    let Some(crate_node) = self.crates.get_mut(crate_idx) else {
      return false;
    };
    let mut changed = false;
    if crate_node.collapsed {
      crate_node.collapsed = false;
      changed = true;
    }
    if let Some(file_node) = crate_node.files.get_mut(file_idx)
      && file_node.collapsed
    {
      file_node.collapsed = false;
      changed = true;
    }
    if changed {
      self.recompute();
    }
    changed
  }

  pub fn iter_edge_curves<'a>(
    &'a self,
    graph: &'a Graph,
    pan: Point2,
    zoom: f32,
    cull_rect: Option<(Rect, f32)>,
  ) -> impl Iterator<Item = EdgeCurve> + 'a {
    edges::iter_edge_curves(graph, self, pan, zoom, cull_rect)
  }

  pub fn hit_test_edge(
    &self,
    graph: &Graph,
    pan: Point2,
    zoom: f32,
    screen_pos: Point2,
  ) -> Option<EdgeId> {
    edges::hit_test_edge(graph, self, pan, zoom, screen_pos)
  }
}

/// Computes the contiguous range of visible items inside a vertically stacked container, plus
/// top/bottom spacer heights (in screen px) that preserve exact vertical alignment with anchors.
fn visible_slice<T>(
  items: &[T],
  span_of: impl Fn(&T) -> VerticalSpan,
  body_content_top_y: f32,
  body_content: RemPx,
  gap_rem: f32,
  rem_size: f32,
  (vp_min_y, vp_max_y): (f32, f32),
) -> (Range<usize>, f32, f32) {
  if items.is_empty() {
    return (0..0, 0.0, 0.0);
  }

  let first = items.partition_point(|item| {
    let span = span_of(item);
    span.screen_top(body_content_top_y, rem_size) + span.screen_height(rem_size) < vp_min_y
  });

  if first >= items.len() {
    return (0..0, body_content.to_px(rem_size), 0.0);
  }

  let last = first
    + items[first..]
      .partition_point(|item| span_of(item).screen_top(body_content_top_y, rem_size) <= vp_max_y);

  if first == last {
    return (0..0, body_content.to_px(rem_size), 0.0);
  }

  let gap = RemPx::rems(gap_rem);
  let top_spacer_px = if first > 0 {
    (span_of(&items[first]).offset - gap).to_px(rem_size).max(0.0)
  } else {
    0.0
  };

  let bottom_spacer_px = if last < items.len() {
    let last_span = span_of(&items[last - 1]);
    let last_end = last_span.offset + last_span.height;
    (body_content - last_end - gap).to_px(rem_size).max(0.0)
  } else {
    0.0
  };

  (first..last, top_spacer_px, bottom_spacer_px)
}

/// Splits a type body into its first line (header) and remaining lines (rest_body),
/// returning `(header_line, rest_body, total_line_count, max_line_chars)`.
fn split_type_body(body: &str) -> (Arc<str>, Option<Arc<str>>, usize, usize) {
  let mut lines = body.lines();
  let Some(first) = lines.next() else {
    return (Arc::from(" "), None, 1, 0);
  };

  let mut max_chars = first.chars().count();
  let header_line = if first.is_empty() {
    Arc::from(" ")
  } else {
    Arc::from(first)
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
    Some(Arc::from(rest))
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

#[cfg(test)]
mod tests {
  use graph::TypeKind;
  use lsp::Position;

  use super::*;

  fn dummy_type(id: usize, name: &str, file_path: &str, body: &str) -> TypeNode {
    TypeNode {
      id: TypeId(id),
      name: name.to_owned(),
      kind: TypeKind::Struct,
      body: body.to_owned(),
      file_path: PathBuf::from(file_path),
      position: Position::new(0, 0),
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
  fn visible_slice_preserves_total_body_height_across_zooms() {
    let line_counts = [2, 5, 1, 8, 3, 4, 6, 2, 7, 3];
    let mut spans = Vec::new();
    let mut offset = RemPx::ZERO;
    for (idx, &lines) in line_counts.iter().enumerate() {
      let height = RemPx::new(
        TYPE_BLOCK_PAD_Y_REM * 2.0 + lines as f32 * TYPE_LINE_HEIGHT_REM,
        2.0,
      );
      spans.push(VerticalSpan { offset, height });
      offset += height;
      if idx + 1 < line_counts.len() {
        offset += RemPx::rems(TYPE_GAP_REM);
      }
    }
    let body_content = offset;

    for zoom in [0.25_f32, 1.0, 2.5] {
      let rem_size = BASE_REM_SIZE * zoom;
      let gap_px = TYPE_GAP_REM * rem_size;
      let full_height = body_content.to_px(rem_size);

      let body_top_y = -full_height * 0.35;
      let (range, top_spacer, bottom_spacer) = visible_slice(
        &spans,
        |&s| s,
        body_top_y,
        body_content,
        TYPE_GAP_REM,
        rem_size,
        (0.0, 300.0),
      );

      assert!(!range.is_empty());
      let mut child_heights: Vec<f32> = Vec::new();
      if top_spacer > 0.0 {
        child_heights.push(top_spacer);
      }
      for span in &spans[range.clone()] {
        child_heights.push(span.screen_height(rem_size));
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

      let expected_first_y = spans[range.start].screen_top(body_top_y, rem_size);
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
  fn workspace_layout_groups_by_crate_and_recomputes_on_collapse() {
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

    let mut layout = WorkspaceLayout::build(&root, &types);
    assert_eq!(layout.crates.len(), 2);
    assert_eq!(layout.crates[0].name.as_ref(), "alpha");
    assert_eq!(layout.crates[0].rel_dir.as_ref(), "crates/alpha");
    assert_eq!(layout.crates[0].files.len(), 2);
    assert_eq!(layout.crates[0].files[0].dir_prefix.as_ref(), "src/");
    assert_eq!(layout.crates[0].files[0].file_name.as_ref(), "a.rs");
    assert_eq!(layout.crates[1].name.as_ref(), "beta");

    // All crates and files start collapsed by default.
    assert!(layout.crates.iter().all(|c| c.collapsed));
    assert!(
      layout
        .crates
        .iter()
        .all(|c| c.files.iter().all(|f| f.collapsed))
    );
    assert_eq!(layout.type_anchor(TypeId(0)), layout.type_anchor(TypeId(2)));
    assert_eq!(layout.type_anchor(TypeId(0)).map(|p| p.x), Some(0.0));
    assert_eq!(
      layout.collapsed_targets.get(&TypeId(2)),
      Some(&CollapsedTarget::Crate(0))
    );

    // Expanding crate 0 while its files remain collapsed shifts beta down and targets each file.
    let collapsed_crate_beta_y = layout.crates[1].position.y;
    assert!(layout.toggle_crate(0));
    assert!(layout.crates[1].position.y > collapsed_crate_beta_y);
    assert_eq!(layout.type_anchor(TypeId(0)), layout.type_anchor(TypeId(1)));
    assert_eq!(
      layout.collapsed_targets.get(&TypeId(0)),
      Some(&CollapsedTarget::File(0, 0))
    );
    assert_eq!(
      layout.collapsed_targets.get(&TypeId(2)),
      Some(&CollapsedTarget::File(0, 1))
    );

    // ensure_type_expanded expands the target file in O(1) and separates type anchors.
    let beta_y_with_collapsed_files = layout.crates[1].position.y;
    assert!(layout.ensure_type_expanded(TypeId(0)));
    assert!(!layout.ensure_type_expanded(TypeId(0)));
    assert!(layout.crates[1].position.y > beta_y_with_collapsed_files);
    assert_eq!(layout.collapsed_targets.get(&TypeId(0)), None);
    assert_ne!(layout.type_anchor(TypeId(0)), layout.type_anchor(TypeId(1)));
  }
}
