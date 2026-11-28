use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use graph::{TypeId, TypeNode};

use crate::geometry::{Point2, point2};
use crate::metrics::{
  BASE_REM_SIZE, BODY_CHAR_WIDTH_REM, BODY_PAD_Y_REM, CARD_BODY_HORIZONTAL_PAD_REM, CARD_GAP_Y,
  CARD_HEADER_HORIZONTAL_PAD_REM, CARD_WIDTH_CULL_SAFETY_FACTOR, HEADER_CHAR_WIDTH_REM,
  HEADER_LINE_HEIGHT_REM, HEADER_PAD_Y_REM, TYPE_BLOCK_PAD_Y_REM, TYPE_GAP_REM,
  TYPE_LINE_HEIGHT_REM,
};

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
  pub header_line: Arc<str>,
  pub rest_body: Option<Arc<str>>,
  pub rest_line_count: usize,
}

pub struct FileNode {
  pub dir_prefix: Arc<str>,
  pub file_name: Arc<str>,
  pub types: Vec<TypeBlock>,
  pub type_layouts: Vec<TypeLayout>,
  pub position: Point2,
  /// Total height of the card split into rem-scaled and fixed-px (border) components.
  pub height_rem: f32,
  pub fixed_border_px: f32,
  /// Total height of the card body's inner content area (all type blocks + gaps).
  pub body_content_rem: f32,
  pub body_content_px: f32,
  /// Stable minimum width in rems so culling wide off-screen types doesn't cause horizontal jitter.
  pub min_width_rem: f32,
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

impl FileNode {
  pub fn screen_height(&self, rem_size: f32) -> f32 {
    self.height_rem * rem_size + self.fixed_border_px
  }

  pub fn estimated_cull_width(&self, rem_size: f32) -> f32 {
    (self.min_width_rem * CARD_WIDTH_CULL_SAFETY_FACTOR + 4.0) * rem_size
  }

  pub fn body_content_top_y(&self, card_screen_y: f32, rem_size: f32) -> f32 {
    let header_and_top_border = (HEADER_PAD_Y_REM * 2.0 + HEADER_LINE_HEIGHT_REM) * rem_size + 2.0;
    card_screen_y + header_and_top_border + BODY_PAD_Y_REM * rem_size
  }
}

/// Groups parsed types by file path and computes stacked card layouts and left-edge anchor points.
pub fn build_file_nodes(
  canonical_root: &Path,
  types: Vec<TypeNode>,
) -> (Vec<FileNode>, HashMap<TypeId, Point2>) {
  let mut grouped: BTreeMap<PathBuf, Vec<TypeNode>> = BTreeMap::new();
  for ty in types {
    grouped.entry(ty.file_path.clone()).or_default().push(ty);
  }

  let mut nodes = Vec::with_capacity(grouped.len());
  let mut type_anchors = HashMap::new();
  let mut cursor_y: f32 = 0.0;

  let header_rem: f32 = HEADER_PAD_Y_REM * 2.0 + HEADER_LINE_HEIGHT_REM;
  let header_height: f32 = header_rem * BASE_REM_SIZE + 2.0;
  let body_padding_y: f32 = BODY_PAD_Y_REM * BASE_REM_SIZE;
  let type_block_pad_y: f32 = TYPE_BLOCK_PAD_Y_REM * BASE_REM_SIZE;
  let type_line_height: f32 = TYPE_LINE_HEIGHT_REM * BASE_REM_SIZE;
  let type_gap: f32 = TYPE_GAP_REM * BASE_REM_SIZE;
  let bottom_border: f32 = 1.0;

  for (file_path, file_types) in grouped {
    let mut local_y = header_height + body_padding_y;
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
      let block_height = type_block_pad_y * 2.0 + lines * type_line_height + block_border_px;

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

      type_anchors.insert(
        type_node.id,
        point2(0.0, cursor_y + local_y + block_height * 0.5),
      );
      local_y += block_height;
      offset_rem += block_height_rem;
      offset_px += block_border_px;
      if idx + 1 < file_types.len() {
        local_y += type_gap;
        offset_rem += TYPE_GAP_REM;
      }
    }

    let rel_path = file_path.strip_prefix(canonical_root).unwrap_or(&file_path);
    let file_name = rel_path
      .file_name()
      .and_then(|n| n.to_str())
      .unwrap_or_default()
      .to_owned();
    let dir_prefix = rel_path
      .parent()
      .and_then(|p| p.to_str())
      .filter(|s| !s.is_empty())
      .map(|s| format!("{s}/"))
      .unwrap_or_default();

    let header_chars = dir_prefix.chars().count() + file_name.chars().count();
    let body_min_w_rem =
      max_body_line_chars as f32 * BODY_CHAR_WIDTH_REM + CARD_BODY_HORIZONTAL_PAD_REM;
    let header_min_w_rem =
      header_chars as f32 * HEADER_CHAR_WIDTH_REM + CARD_HEADER_HORIZONTAL_PAD_REM;
    let min_width_rem = body_min_w_rem.max(header_min_w_rem);

    let body_content_rem = offset_rem;
    let body_content_px = offset_px;
    let height_rem = header_rem + BODY_PAD_Y_REM * 2.0 + body_content_rem;
    let fixed_border_px = 2.0 + body_content_px + bottom_border;
    let card_height = local_y + body_padding_y + bottom_border;
    nodes.push(FileNode {
      dir_prefix: Arc::from(dir_prefix),
      file_name: Arc::from(file_name),
      types: type_blocks,
      type_layouts,
      position: point2(0.0, cursor_y),
      height_rem,
      fixed_border_px,
      body_content_rem,
      body_content_px,
      min_width_rem,
    });
    cursor_y += card_height + CARD_GAP_Y;
  }

  (nodes, type_anchors)
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
  use super::*;

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
}
