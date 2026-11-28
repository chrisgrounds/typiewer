use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
  App, Bounds, BoxShadow, Context, FocusHandle, FontWeight, KeyBinding, MouseButton,
  MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, ScrollEvent, Task,
  TitlebarOptions, Window, WindowBounds, WindowOptions, actions, canvas, div, fill,
  linear_color_stop, linear_gradient, point, prelude::*, px, rems, rgb, rgba, size,
};
use gpui_platform::application;
use lsp::LspClient;

use crate::{EdgeId, FunctionInfo, Graph, GraphEdge, GraphResolution, TypeId, TypeNode};

actions!(viewer, [ZoomIn, ZoomOut, ResetZoom, ClearSelection]);

const BASE_REM_SIZE: f32 = 16.0;
const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 3.0;
const DEFAULT_PAN_X: f32 = 300.0;
const DEFAULT_PAN_Y: f32 = 56.0;

// Layout constants in `rems` (matching `render()` so edge anchors align with each type block).
const HEADER_PAD_Y_REM: f32 = 0.5;
const HEADER_LINE_HEIGHT_REM: f32 = 1.5;
const BODY_PAD_Y_REM: f32 = 0.75;
const TYPE_BLOCK_PAD_Y_REM: f32 = 0.5;
const TYPE_LINE_HEIGHT_REM: f32 = 1.35;
const TYPE_GAP_REM: f32 = 0.625;
const CARD_GAP_Y: f32 = 36.0;

// Viewport culling overscan margins (in screen pixels).
// Increase to pre-render further outside the window; set to 0.0 to cull strictly at viewport edges.
const VIEWPORT_OVERSCAN_X_PX: f32 = 64.0;
const VIEWPORT_OVERSCAN_Y_PX: f32 = 64.0;
const EDGE_CULL_PADDING_PX: f32 = 24.0;

// Approximate Menlo monospace advance width in rems at 0.82rem body / 0.875rem header,
// used to stabilize card widths during intra-card type culling and for horizontal culling.
const BODY_CHAR_WIDTH_REM: f32 = 0.6001 * 0.82;
const HEADER_CHAR_WIDTH_REM: f32 = 0.54;
const CARD_BODY_HORIZONTAL_PAD_REM: f32 = 3.75;
const CARD_HEADER_HORIZONTAL_PAD_REM: f32 = 2.25;
const CARD_WIDTH_CULL_SAFETY_FACTOR: f32 = 1.15;

const EDGE_HIT_SEGMENTS: usize = 32;

#[derive(Clone, Copy, Debug)]
struct ViewportRect {
  min_x: f32,
  min_y: f32,
  max_x: f32,
  max_y: f32,
}

impl ViewportRect {
  fn from_size(width: f32, height: f32, overscan_x: f32, overscan_y: f32) -> Self {
    Self {
      min_x: -overscan_x,
      min_y: -overscan_y,
      max_x: width + overscan_x,
      max_y: height + overscan_y,
    }
  }

  fn intersects(&self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> bool {
    max_x >= self.min_x && min_x <= self.max_x && max_y >= self.min_y && min_y <= self.max_y
  }
}

#[derive(Clone, Copy, Debug)]
struct TypeLayout {
  /// Offset from the top of the card body's inner content area (excluding body top padding).
  offset_rem: f32,
  offset_px: f32,
  height_rem: f32,
  border_px: f32,
}

impl TypeLayout {
  fn screen_top(&self, body_content_top_y: f32, rem_size: f32) -> f32 {
    body_content_top_y + self.offset_rem * rem_size + self.offset_px
  }

  fn screen_height(&self, rem_size: f32) -> f32 {
    self.height_rem * rem_size + self.border_px
  }
}

struct FileNode {
  dir_prefix: String,
  file_name: String,
  types: Vec<TypeNode>,
  type_layouts: Vec<TypeLayout>,
  position: Point<f32>,
  /// Total height of the card split into rem-scaled and fixed-px (border) components.
  height_rem: f32,
  fixed_border_px: f32,
  /// Total height of the card body's inner content area (all type blocks + gaps).
  body_content_rem: f32,
  body_content_px: f32,
  /// Stable minimum width in rems so culling wide off-screen types doesn't cause horizontal jitter.
  min_width_rem: f32,
}

impl FileNode {
  fn screen_height(&self, rem_size: f32) -> f32 {
    self.height_rem * rem_size + self.fixed_border_px
  }

  fn estimated_cull_width(&self, rem_size: f32) -> f32 {
    (self.min_width_rem * CARD_WIDTH_CULL_SAFETY_FACTOR + 4.0) * rem_size
  }

  fn body_content_top_y(&self, card_screen_y: f32, rem_size: f32) -> f32 {
    let header_and_top_border =
      (HEADER_PAD_Y_REM * 2.0 + HEADER_LINE_HEIGHT_REM) * rem_size + 2.0;
    card_screen_y + header_and_top_border + BODY_PAD_Y_REM * rem_size
  }
}

#[derive(Clone, Copy)]
struct EdgeCurve {
  edge_id: EdgeId,
  p0: Point<f32>,
  c1: Point<f32>,
  c2: Point<f32>,
  p3: Point<f32>,
  arrow_tip: Point<f32>,
}

impl EdgeCurve {
  fn aabb(&self, padding: f32) -> (f32, f32, f32, f32) {
    let min_x = self
      .p0
      .x
      .min(self.c1.x)
      .min(self.c2.x)
      .min(self.p3.x)
      .min(self.arrow_tip.x)
      - padding;
    let max_x = self
      .p0
      .x
      .max(self.c1.x)
      .max(self.c2.x)
      .max(self.p3.x)
      .max(self.arrow_tip.x)
      + padding;
    let min_y = self
      .p0
      .y
      .min(self.c1.y)
      .min(self.c2.y)
      .min(self.p3.y)
      .min(self.arrow_tip.y)
      - padding;
    let max_y = self
      .p0
      .y
      .max(self.c1.y)
      .max(self.c2.y)
      .max(self.p3.y)
      .max(self.arrow_tip.y)
      + padding;
    (min_x, min_y, max_x, max_y)
  }
}

/// Computes the contiguous range of visible type indices inside a card, plus top/bottom spacer
/// heights (in screen px) that preserve exact vertical alignment with edge anchors.
fn visible_type_slice(
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

enum LspStatus {
  Initialising,
  Ready,
  Failed(String),
}

pub struct Application {
  canonical_root: PathBuf,
  nodes: Vec<FileNode>,
  types: Vec<TypeNode>,
  functions: Vec<FunctionInfo>,
  edges: Vec<GraphEdge>,
  directed_pairs: HashSet<(TypeId, TypeId)>,
  type_anchors: HashMap<TypeId, Point<f32>>,
  selected_type: Option<TypeId>,
  selected_edge: Option<EdgeId>,
  hovered_edge: Option<EdgeId>,
  lsp_status: LspStatus,
  _lsp_task: Task<()>,
  pan: Point<f32>,
  zoom: f32,
  drag_last_pos: Option<Point<f32>>,
  focus_handle: FocusHandle,
}

impl Application {
  fn new(workspace_root: PathBuf, graph: Graph, cx: &mut Context<Self>) -> Self {
    let canonical_root =
      std::fs::canonicalize(&workspace_root).unwrap_or_else(|_| workspace_root.clone());
    let types_for_lsp = graph.types.clone();
    let functions_for_lsp = graph.functions.clone();
    let all_types = graph.types.clone();

    let mut grouped: BTreeMap<PathBuf, Vec<TypeNode>> = BTreeMap::new();
    for ty in graph.types {
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
      let mut type_layouts = Vec::with_capacity(file_types.len());
      let mut offset_rem: f32 = 0.0;
      let mut offset_px: f32 = 0.0;
      let mut max_body_line_chars: usize = 0;

      for (idx, type_node) in file_types.iter().enumerate() {
        let mut line_count: usize = 0;
        for line in type_node.body.lines() {
          line_count += 1;
          max_body_line_chars = max_body_line_chars.max(line.chars().count());
        }
        let lines = line_count.max(1) as f32;
        let block_height_rem = TYPE_BLOCK_PAD_Y_REM * 2.0 + lines * TYPE_LINE_HEIGHT_REM;
        let block_border_px = 2.0;
        let block_height = type_block_pad_y * 2.0 + lines * type_line_height + block_border_px;

        type_layouts.push(TypeLayout {
          offset_rem,
          offset_px,
          height_rem: block_height_rem,
          border_px: block_border_px,
        });

        type_anchors.insert(
          type_node.id,
          point(0.0, cursor_y + local_y + block_height * 0.5),
        );
        local_y += block_height;
        offset_rem += block_height_rem;
        offset_px += block_border_px;
        if idx + 1 < file_types.len() {
          local_y += type_gap;
          offset_rem += TYPE_GAP_REM;
        }
      }

      let rel_path = file_path
        .strip_prefix(&canonical_root)
        .unwrap_or(&file_path);
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
        dir_prefix,
        file_name,
        types: file_types,
        type_layouts,
        position: point(0.0, cursor_y),
        height_rem,
        fixed_border_px,
        body_content_rem,
        body_content_px,
        min_width_rem,
      });
      cursor_y += card_height + CARD_GAP_Y;
    }

    let lsp_task = cx.spawn(async move |this, cx| {
      let result = cx
        .background_executor()
        .spawn(async move {
          let lsp = LspClient::start_rust_analyzer(&workspace_root)?;
          lsp.wait_until_ready(Duration::from_secs(60))?;
          let resolution = Graph::resolve_edges(&types_for_lsp, &functions_for_lsp, &lsp);
          let _ = lsp.shutdown();
          Ok::<GraphResolution, lsp::LspError>(resolution)
        })
        .await;

      let _ = this.update(cx, |app, cx| {
        match result {
          Ok(resolution) => {
            app.directed_pairs = resolution.edges.iter().map(|e| (e.from, e.to)).collect();
            app.edges = resolution.edges;
            app.functions = resolution.functions;
            app.lsp_status = LspStatus::Ready;
          }
          Err(err) => {
            app.lsp_status = LspStatus::Failed(err.to_string());
          }
        }
        cx.notify();
      });
    });

    let directed_pairs = graph.edges.iter().map(|e| (e.from, e.to)).collect();

    Self {
      canonical_root,
      nodes,
      types: all_types,
      functions: graph.functions,
      edges: graph.edges,
      directed_pairs,
      type_anchors,
      selected_type: None,
      selected_edge: None,
      hovered_edge: None,
      lsp_status: LspStatus::Initialising,
      _lsp_task: lsp_task,
      pan: point(DEFAULT_PAN_X, DEFAULT_PAN_Y),
      zoom: 1.0,
      drag_last_pos: None,
      focus_handle: cx.focus_handle(),
    }
  }

  fn relative_display_path(&self, path: &Path) -> String {
    path
      .strip_prefix(&self.canonical_root)
      .unwrap_or(path)
      .display()
      .to_string()
  }

  fn compute_edge_curves(
    &self,
    origin: Point<f32>,
    cull_rect: Option<(ViewportRect, f32)>,
  ) -> Vec<EdgeCurve> {
    let zoom = self.zoom;
    let pan = self.pan;
    let arrow_len = (7.0 * zoom).clamp(4.0, 12.0);

    self
      .edges
      .iter()
      .filter_map(|edge| {
        let from = *self.type_anchors.get(&edge.from)?;
        let to = *self.type_anchors.get(&edge.to)?;

        let from_x = origin.x + from.x * zoom + pan.x;
        let from_y = origin.y + from.y * zoom + pan.y;
        let to_x = origin.x + to.x * zoom + pan.x;
        let to_y = origin.y + to.y * zoom + pan.y;

        let dy = (to_y - from_y).abs();
        let bidirectional_offset =
          if self.directed_pairs.contains(&(edge.to, edge.from)) && edge.from > edge.to {
            34.0 * zoom
          } else {
            0.0
          };
        let arc = (54.0 * zoom + dy * 0.30 + bidirectional_offset).clamp(54.0 * zoom, 460.0 * zoom);

        let curve = EdgeCurve {
          edge_id: edge.id,
          p0: point(from_x, from_y),
          c1: point(from_x - arc, from_y),
          c2: point(to_x - arc, to_y),
          p3: point(to_x - arrow_len * 0.5, to_y),
          arrow_tip: point(to_x, to_y),
        };

        if let Some((rect, padding)) = cull_rect {
          let (min_x, min_y, max_x, max_y) = curve.aabb(padding);
          if !rect.intersects(min_x, min_y, max_x, max_y) {
            return None;
          }
        }

        Some(curve)
      })
      .collect()
  }

  fn hit_test_edge(&self, screen_pos: Point<f32>) -> Option<EdgeId> {
    let threshold = (9.0 * self.zoom).clamp(7.0, 14.0);
    let cursor_rect = ViewportRect {
      min_x: screen_pos.x,
      min_y: screen_pos.y,
      max_x: screen_pos.x,
      max_y: screen_pos.y,
    };
    let curves = self.compute_edge_curves(point(0.0, 0.0), Some((cursor_rect, threshold)));
    let threshold_sq = threshold * threshold;

    let mut best: Option<(EdgeId, f32)> = None;
    for curve in &curves {
      let dist_sq = min_dist_sq_to_cubic_bezier(
        screen_pos,
        curve.p0,
        curve.c1,
        curve.c2,
        curve.p3,
        EDGE_HIT_SEGMENTS,
      );
      if dist_sq <= threshold_sq && best.is_none_or(|(_, best_d)| dist_sq < best_d) {
        best = Some((curve.edge_id, dist_sq));
      }
    }

    best.map(|(id, _)| id)
  }

  fn set_zoom_around(
    &mut self,
    new_zoom: f32,
    focal: Point<f32>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let clamped = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    if (clamped - self.zoom).abs() < f32::EPSILON {
      return;
    }

    let world_x = (focal.x - self.pan.x) / self.zoom;
    let world_y = (focal.y - self.pan.y) / self.zoom;

    self.zoom = clamped;
    self.pan.x = focal.x - world_x * self.zoom;
    self.pan.y = focal.y - world_y * self.zoom;

    window.set_rem_size(px(BASE_REM_SIZE * self.zoom));
    cx.notify();
  }

  fn zoom_in(&mut self, _: &ZoomIn, window: &mut Window, cx: &mut Context<Self>) {
    let viewport = window.viewport_size();
    let center = point(
      f32::from(viewport.width) * 0.5,
      f32::from(viewport.height) * 0.5,
    );
    self.set_zoom_around(self.zoom * 1.15, center, window, cx);
  }

  fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
    let viewport = window.viewport_size();
    let center = point(
      f32::from(viewport.width) * 0.5,
      f32::from(viewport.height) * 0.5,
    );
    self.set_zoom_around(self.zoom / 1.15, center, window, cx);
  }

  fn reset_zoom(&mut self, _: &ResetZoom, window: &mut Window, cx: &mut Context<Self>) {
    self.zoom = 1.0;
    self.pan = point(DEFAULT_PAN_X, DEFAULT_PAN_Y);
    window.set_rem_size(px(BASE_REM_SIZE));
    cx.notify();
  }

  fn clear_selection(&mut self, _: &ClearSelection, _window: &mut Window, cx: &mut Context<Self>) {
    if self.selected_type.is_some() || self.selected_edge.is_some() {
      self.selected_type = None;
      self.selected_edge = None;
      cx.notify();
    }
  }

  fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
    window.focus(&self.focus_handle, cx);
    let pos = point(f32::from(event.position.x), f32::from(event.position.y));

    if event.button == MouseButton::Left
      && let Some(edge_id) = self.hit_test_edge(pos)
    {
      self.selected_type = None;
      self.selected_edge = if self.selected_edge == Some(edge_id) {
        None
      } else {
        Some(edge_id)
      };
      cx.notify();
      return;
    }

    if event.button == MouseButton::Left || event.button == MouseButton::Middle {
      self.selected_type = None;
      self.selected_edge = None;
      self.drag_last_pos = Some(pos);
      cx.notify();
    }
  }

  fn on_mouse_move(
    &mut self,
    event: &MouseMoveEvent,
    _window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let current = point(f32::from(event.position.x), f32::from(event.position.y));

    if let Some(last) = self.drag_last_pos {
      self.pan.x += current.x - last.x;
      self.pan.y += current.y - last.y;
      self.drag_last_pos = Some(current);
      cx.notify();
    } else {
      let hovered = self.hit_test_edge(current);
      if hovered != self.hovered_edge {
        self.hovered_edge = hovered;
        cx.notify();
      }
    }
  }

  fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
    if self.drag_last_pos.take().is_some() {
      cx.notify();
    }
  }

  fn on_scroll(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
    let delta: Point<Pixels> = event.delta.pixel_delta(px(20.0));
    let dx = f32::from(delta.x);
    let dy = f32::from(delta.y);

    if event.modifiers.platform || event.modifiers.control {
      let factor = (1.0 + dy * 0.004).clamp(0.75, 1.25);
      let cursor = point(f32::from(event.position.x), f32::from(event.position.y));
      self.set_zoom_around(self.zoom * factor, cursor, window, cx);
    } else {
      self.pan.x += dx;
      self.pan.y += dy;
      cx.notify();
    }
  }
}

fn eval_cubic_bezier(
  p0: Point<f32>,
  c1: Point<f32>,
  c2: Point<f32>,
  p3: Point<f32>,
  t: f32,
) -> Point<f32> {
  let u = 1.0 - t;
  let uu = u * u;
  let tt = t * t;
  let uuu = uu * u;
  let ttt = tt * t;
  point(
    uuu * p0.x + 3.0 * uu * t * c1.x + 3.0 * u * tt * c2.x + ttt * p3.x,
    uuu * p0.y + 3.0 * uu * t * c1.y + 3.0 * u * tt * c2.y + ttt * p3.y,
  )
}

fn point_to_segment_dist_sq(p: Point<f32>, a: Point<f32>, b: Point<f32>) -> f32 {
  let ab_x = b.x - a.x;
  let ab_y = b.y - a.y;
  let len_sq = ab_x * ab_x + ab_y * ab_y;
  if len_sq <= f32::EPSILON {
    let dx = p.x - a.x;
    let dy = p.y - a.y;
    return dx * dx + dy * dy;
  }

  let t = (((p.x - a.x) * ab_x + (p.y - a.y) * ab_y) / len_sq).clamp(0.0, 1.0);
  let proj_x = a.x + t * ab_x;
  let proj_y = a.y + t * ab_y;
  let dx = p.x - proj_x;
  let dy = p.y - proj_y;
  dx * dx + dy * dy
}

fn min_dist_sq_to_cubic_bezier(
  p: Point<f32>,
  p0: Point<f32>,
  c1: Point<f32>,
  c2: Point<f32>,
  p3: Point<f32>,
  segments: usize,
) -> f32 {
  let steps = segments.max(1);
  let mut prev = p0;
  let mut min_sq = f32::INFINITY;

  for i in 1..=steps {
    let t = i as f32 / steps as f32;
    let curr = eval_cubic_bezier(p0, c1, c2, p3, t);
    let d_sq = point_to_segment_dist_sq(p, prev, curr);
    if d_sq < min_sq {
      min_sq = d_sq;
    }
    prev = curr;
  }

  min_sq
}

#[derive(Clone, Copy)]
enum EdgeHighlight {
  Normal,
  Dimmed,
  Outgoing,
  Incoming,
  Hovered,
  Selected,
}

impl Render for Application {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let pan = self.pan;
    let zoom = self.zoom;
    let rem_size = BASE_REM_SIZE * zoom;
    let viewport = window.viewport_size();
    let viewport_rect = ViewportRect::from_size(
      f32::from(viewport.width),
      f32::from(viewport.height),
      VIEWPORT_OVERSCAN_X_PX,
      VIEWPORT_OVERSCAN_Y_PX,
    );
    let is_dragging = self.drag_last_pos.is_some();
    let is_hovering_edge = self.hovered_edge.is_some();

    let dot_grid = canvas(
      |_bounds, _window, _cx| {},
      move |bounds, _, window, _cx| {
        let mut step = 32.0 * zoom;
        while step < 14.0 {
          step *= 2.0;
        }
        let dot_size = (2.0 * zoom).clamp(1.0, 3.0);

        let origin_x = f32::from(bounds.origin.x);
        let origin_y = f32::from(bounds.origin.y);
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);

        let start_x = origin_x + (pan.x - origin_x).rem_euclid(step);
        let start_y = origin_y + (pan.y - origin_y).rem_euclid(step);

        let dot_color = rgba(0xffffff14);
        let mut y = start_y;
        while y < origin_y + height {
          let mut x = start_x;
          while x < origin_x + width {
            window.paint_quad(fill(
              Bounds::new(point(px(x), px(y)), size(px(dot_size), px(dot_size))),
              dot_color,
            ));
            x += step;
          }
          y += step;
        }
      },
    )
    .absolute()
    .inset_0()
    .size_full();

    let selected_type = self.selected_type;
    let selected_edge_id = self.selected_edge;
    let hovered_edge_id = self.hovered_edge;

    let selected_edge_ref = selected_edge_id.and_then(|id| self.edges.get(id.0));
    let selected_edge_from = selected_edge_ref.map(|e| e.from);
    let selected_edge_to = selected_edge_ref.map(|e| e.to);

    let connected_types: HashSet<TypeId> = match selected_type {
      Some(sel) => self
        .edges
        .iter()
        .filter_map(|edge| {
          if edge.from == sel {
            Some(edge.to)
          } else if edge.to == sel {
            Some(edge.from)
          } else {
            None
          }
        })
        .collect(),
      None => HashSet::new(),
    };

    let edge_cull_padding = EDGE_CULL_PADDING_PX * zoom.max(1.0);
    let edge_curves =
      self.compute_edge_curves(point(0.0, 0.0), Some((viewport_rect, edge_cull_padding)));
    let styled_curves: Vec<(EdgeCurve, EdgeHighlight)> = edge_curves
      .into_iter()
      .filter_map(|curve| {
        let edge = self.edges.get(curve.edge_id.0)?;
        let highlight = if selected_edge_id == Some(edge.id) {
          EdgeHighlight::Selected
        } else if hovered_edge_id == Some(edge.id) {
          EdgeHighlight::Hovered
        } else if selected_edge_id.is_some() {
          EdgeHighlight::Dimmed
        } else {
          match selected_type {
            None => EdgeHighlight::Normal,
            Some(sel) if edge.from == sel => EdgeHighlight::Outgoing,
            Some(sel) if edge.to == sel => EdgeHighlight::Incoming,
            Some(_) => EdgeHighlight::Dimmed,
          }
        };
        Some((curve, highlight))
      })
      .collect();

    let edges_layer = canvas(
      |_bounds, _window, _cx| {},
      move |bounds, _, window, _cx| {
        if styled_curves.is_empty() {
          return;
        }

        let origin_x = f32::from(bounds.origin.x);
        let origin_y = f32::from(bounds.origin.y);
        let normal_width = (1.5 * zoom).clamp(1.0, 3.0);
        let highlight_width = (2.4 * zoom).clamp(1.5, 4.5);
        let selected_width = (3.0 * zoom).clamp(2.0, 5.5);
        let arrow_len = (7.0 * zoom).clamp(4.0, 12.0);
        let arrow_half = (3.5 * zoom).clamp(2.0, 6.0);
        let dot_size = (5.0 * zoom).clamp(3.0, 8.0);

        let mut dimmed_strokes = PathBuilder::stroke(px(normal_width));
        let mut dimmed_arrows = PathBuilder::fill();
        let mut has_dimmed = false;
        let mut normal_strokes = PathBuilder::stroke(px(normal_width));
        let mut normal_arrows = PathBuilder::fill();
        let mut has_normal = false;
        let mut incoming_strokes = PathBuilder::stroke(px(highlight_width));
        let mut incoming_arrows = PathBuilder::fill();
        let mut has_incoming = false;
        let mut outgoing_strokes = PathBuilder::stroke(px(highlight_width));
        let mut outgoing_arrows = PathBuilder::fill();
        let mut has_outgoing = false;
        let mut hovered_strokes = PathBuilder::stroke(px(highlight_width));
        let mut hovered_arrows = PathBuilder::fill();
        let mut has_hovered = false;
        let mut selected_strokes = PathBuilder::stroke(px(selected_width));
        let mut selected_arrows = PathBuilder::fill();
        let mut has_selected = false;

        let mut normal_dots = Vec::new();
        let mut incoming_dots = Vec::new();
        let mut outgoing_dots = Vec::new();
        let mut hovered_dots = Vec::new();
        let mut selected_dots = Vec::new();

        for &(curve, highlight) in &styled_curves {
          let p0 = point(origin_x + curve.p0.x, origin_y + curve.p0.y);
          let c1 = point(origin_x + curve.c1.x, origin_y + curve.c1.y);
          let c2 = point(origin_x + curve.c2.x, origin_y + curve.c2.y);
          let p3 = point(origin_x + curve.p3.x, origin_y + curve.p3.y);
          let tip = point(origin_x + curve.arrow_tip.x, origin_y + curve.arrow_tip.y);

          let (strokes, arrows) = match highlight {
            EdgeHighlight::Dimmed => {
              has_dimmed = true;
              (&mut dimmed_strokes, &mut dimmed_arrows)
            }
            EdgeHighlight::Normal => {
              has_normal = true;
              normal_dots.push((p0.x, p0.y));
              (&mut normal_strokes, &mut normal_arrows)
            }
            EdgeHighlight::Incoming => {
              has_incoming = true;
              incoming_dots.push((p0.x, p0.y));
              (&mut incoming_strokes, &mut incoming_arrows)
            }
            EdgeHighlight::Outgoing => {
              has_outgoing = true;
              outgoing_dots.push((p0.x, p0.y));
              (&mut outgoing_strokes, &mut outgoing_arrows)
            }
            EdgeHighlight::Hovered => {
              has_hovered = true;
              hovered_dots.push((p0.x, p0.y));
              (&mut hovered_strokes, &mut hovered_arrows)
            }
            EdgeHighlight::Selected => {
              has_selected = true;
              selected_dots.push((p0.x, p0.y));
              (&mut selected_strokes, &mut selected_arrows)
            }
          };

          strokes.move_to(point(px(p0.x), px(p0.y)));
          strokes.cubic_bezier_to(
            point(px(p3.x), px(p3.y)),
            point(px(c1.x), px(c1.y)),
            point(px(c2.x), px(c2.y)),
          );

          arrows.move_to(point(px(tip.x), px(tip.y)));
          arrows.line_to(point(px(tip.x - arrow_len), px(tip.y - arrow_half)));
          arrows.line_to(point(px(tip.x - arrow_len), px(tip.y + arrow_half)));
          arrows.close();
        }

        let paint_layer = |window: &mut Window,
                           has_edges: bool,
                           strokes: PathBuilder,
                           arrows: PathBuilder,
                           dots: &[(f32, f32)],
                           stroke_color,
                           arrow_color| {
          if !has_edges {
            return;
          }
          if let Ok(path) = strokes.build() {
            window.paint_path(path, stroke_color);
          }
          if let Ok(arrow_path) = arrows.build() {
            window.paint_path(arrow_path, arrow_color);
          }
          for &(x, y) in dots {
            window.paint_quad(fill(
              Bounds::new(
                point(px(x - dot_size * 0.5), px(y - dot_size * 0.5)),
                size(px(dot_size), px(dot_size)),
              ),
              arrow_color,
            ));
          }
        };

        paint_layer(
          window,
          has_dimmed,
          dimmed_strokes,
          dimmed_arrows,
          &[],
          rgba(0x7dd3fc12),
          rgba(0x7dd3fc24),
        );
        paint_layer(
          window,
          has_normal,
          normal_strokes,
          normal_arrows,
          &normal_dots,
          rgba(0x7dd3fc55),
          rgba(0x7dd3fccc),
        );
        paint_layer(
          window,
          has_incoming,
          incoming_strokes,
          incoming_arrows,
          &incoming_dots,
          rgba(0x34d399ee),
          rgba(0x34d399ff),
        );
        paint_layer(
          window,
          has_outgoing,
          outgoing_strokes,
          outgoing_arrows,
          &outgoing_dots,
          rgba(0x38bdf8ee),
          rgba(0x38bdf8ff),
        );
        paint_layer(
          window,
          has_hovered,
          hovered_strokes,
          hovered_arrows,
          &hovered_dots,
          rgba(0xfbbf24dd),
          rgba(0xfbbf24ff),
        );
        paint_layer(
          window,
          has_selected,
          selected_strokes,
          selected_arrows,
          &selected_dots,
          rgba(0xfbbf24ff),
          rgba(0xfde68aff),
        );
      },
    )
    .absolute()
    .inset_0()
    .size_full();

    let inspector_panel = selected_edge_ref.and_then(|edge| {
      let from_type = self.types.get(edge.from.0)?;
      let to_type = self.types.get(edge.to.0)?;
      let fn_count = edge.functions.len();
      let count_label = if fn_count == 1 {
        "1 function".to_owned()
      } else {
        format!("{fn_count} functions")
      };

      let function_cards: Vec<_> = edge
        .functions
        .iter()
        .filter_map(|fid| self.functions.get(fid.0))
        .map(|func| {
          let sig = func.signature();
          let location = format!(
            "{}:{}",
            self.relative_display_path(&func.file_path),
            func.position.line + 1
          );
          let input_pills: Vec<(TypeId, String)> = func
            .input_types
            .iter()
            .filter_map(|tid| {
              self
                .types
                .get(tid.0)
                .map(|t| (*tid, format!("{} {}", t.kind, t.name)))
            })
            .collect();
          let output_pills: Vec<(TypeId, String)> = func
            .output_types
            .iter()
            .filter_map(|tid| {
              self
                .types
                .get(tid.0)
                .map(|t| (*tid, format!("{} {}", t.kind, t.name)))
            })
            .collect();

          div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .rounded(px(8.0))
            .bg(rgba(0xffffff06))
            .border_1()
            .border_color(rgba(0xffffff10))
            .child(
              div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                  div()
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0xfde68a))
                    .child(format!("fn {}", func.name)),
                )
                .child(
                  div()
                    .text_size(px(11.0))
                    .text_color(rgb(0x6b7280))
                    .child(location),
                ),
            )
            .child(
              div()
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(6.0))
                .bg(rgba(0x00000048))
                .border_1()
                .border_color(rgba(0xffffff0a))
                .font_family("Menlo")
                .text_size(px(12.0))
                .text_color(rgb(0xe2e8f0))
                .child(sig),
            )
            .when(!input_pills.is_empty(), |el| {
              el.child(
                div()
                  .flex()
                  .flex_wrap()
                  .items_center()
                  .gap(px(6.0))
                  .child(
                    div()
                      .text_size(px(11.0))
                      .text_color(rgb(0x94a3b8))
                      .child("Inputs:"),
                  )
                  .children(input_pills.into_iter().map(|(tid, label)| {
                    div()
                      .id(("inspector-in", func.id.0 * 65_536 + tid.0))
                      .cursor_pointer()
                      .px(px(8.0))
                      .py(px(2.0))
                      .rounded(px(6.0))
                      .bg(rgba(0x38bdf820))
                      .border_1()
                      .border_color(rgba(0x38bdf844))
                      .text_size(px(11.0))
                      .text_color(rgb(0x7dd3fc))
                      .hover(|el| el.bg(rgba(0x38bdf838)))
                      .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                          cx.stop_propagation();
                          window.focus(&this.focus_handle, cx);
                          this.selected_edge = None;
                          this.selected_type = Some(tid);
                          cx.notify();
                        }),
                      )
                      .child(label)
                  })),
              )
            })
            .when(!output_pills.is_empty(), |el| {
              el.child(
                div()
                  .flex()
                  .flex_wrap()
                  .items_center()
                  .gap(px(6.0))
                  .child(
                    div()
                      .text_size(px(11.0))
                      .text_color(rgb(0x94a3b8))
                      .child("Outputs:"),
                  )
                  .children(output_pills.into_iter().map(|(tid, label)| {
                    div()
                      .id(("inspector-out", func.id.0 * 65_536 + tid.0))
                      .cursor_pointer()
                      .px(px(8.0))
                      .py(px(2.0))
                      .rounded(px(6.0))
                      .bg(rgba(0x34d39920))
                      .border_1()
                      .border_color(rgba(0x34d39944))
                      .text_size(px(11.0))
                      .text_color(rgb(0x6ee7b7))
                      .hover(|el| el.bg(rgba(0x34d39938)))
                      .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                          cx.stop_propagation();
                          window.focus(&this.focus_handle, cx);
                          this.selected_edge = None;
                          this.selected_type = Some(tid);
                          cx.notify();
                        }),
                      )
                      .child(label)
                  })),
              )
            })
        })
        .collect();

      let from_id = from_type.id;
      let to_id = to_type.id;

      Some(
        div()
          .id("edge-inspector")
          .absolute()
          .right(px(16.0))
          .top(px(44.0))
          .w(px(440.0))
          .max_h(px(540.0))
          .flex()
          .flex_col()
          .overflow_hidden()
          .rounded(px(12.0))
          .bg(rgba(0x11131af2))
          .border_1()
          .border_color(rgba(0xfbbf2444))
          .shadow(vec![BoxShadow {
            inset: false,
            color: rgba(0x00000099).into(),
            offset: point(px(0.0), px(12.0)),
            blur_radius: px(28.0),
            spread_radius: px(-4.0),
          }])
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_this, _, _window, cx| {
              cx.stop_propagation();
            }),
          )
          .on_scroll(cx.listener(|_this, _, _window, cx| {
            cx.stop_propagation();
          }))
          .child(
            div()
              .px(px(14.0))
              .py(px(10.0))
              .flex()
              .items_center()
              .justify_between()
              .gap(px(8.0))
              .bg(rgba(0xffffff06))
              .border_b_1()
              .border_color(rgba(0xffffff12))
              .child(
                div()
                  .flex()
                  .flex_wrap()
                  .items_center()
                  .gap(px(6.0))
                  .child(
                    div()
                      .id("inspector-from-type")
                      .cursor_pointer()
                      .px(px(7.0))
                      .py(px(2.0))
                      .rounded(px(5.0))
                      .bg(rgba(0x38bdf822))
                      .text_size(px(12.0))
                      .font_weight(FontWeight::MEDIUM)
                      .text_color(rgb(0x7dd3fc))
                      .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                          cx.stop_propagation();
                          window.focus(&this.focus_handle, cx);
                          this.selected_edge = None;
                          this.selected_type = Some(from_id);
                          cx.notify();
                        }),
                      )
                      .child(from_type.name.clone()),
                  )
                  .child(
                    div()
                      .text_size(px(12.0))
                      .text_color(rgb(0xfbbf24))
                      .child("→"),
                  )
                  .child(
                    div()
                      .id("inspector-to-type")
                      .cursor_pointer()
                      .px(px(7.0))
                      .py(px(2.0))
                      .rounded(px(5.0))
                      .bg(rgba(0x34d39922))
                      .text_size(px(12.0))
                      .font_weight(FontWeight::MEDIUM)
                      .text_color(rgb(0x6ee7b7))
                      .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                          cx.stop_propagation();
                          window.focus(&this.focus_handle, cx);
                          this.selected_edge = None;
                          this.selected_type = Some(to_id);
                          cx.notify();
                        }),
                      )
                      .child(to_type.name.clone()),
                  )
                  .child(
                    div()
                      .text_size(px(11.0))
                      .text_color(rgb(0x94a3b8))
                      .child(format!("· {count_label}")),
                  ),
              )
              .child(
                div()
                  .id("close-inspector")
                  .cursor_pointer()
                  .px(px(6.0))
                  .py(px(2.0))
                  .rounded(px(5.0))
                  .text_size(px(14.0))
                  .text_color(rgb(0x94a3b8))
                  .hover(|el| el.bg(rgba(0xffffff14)).text_color(rgb(0xf8fafc)))
                  .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _window, cx| {
                      cx.stop_propagation();
                      this.selected_edge = None;
                      cx.notify();
                    }),
                  )
                  .child("×"),
              ),
          )
          .child(
            div()
              .id("inspector-scroll")
              .max_h(px(480.0))
              .overflow_y_scroll()
              .p(px(12.0))
              .flex()
              .flex_col()
              .gap(px(10.0))
              .children(function_cards),
          ),
      )
    });

    let hud_zoom_label = format!("{}%", (zoom * 100.0).round() as i32);

    let first_visible_node = self.nodes.partition_point(|node| {
      let screen_y = node.position.y * zoom + pan.y;
      screen_y + node.screen_height(rem_size) < viewport_rect.min_y
    });

    div()
      .id("canvas-root")
      .track_focus(&self.focus_handle)
      .on_action(cx.listener(Self::zoom_in))
      .on_action(cx.listener(Self::zoom_out))
      .on_action(cx.listener(Self::reset_zoom))
      .on_action(cx.listener(Self::clear_selection))
      .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
      .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
      .on_mouse_move(cx.listener(Self::on_mouse_move))
      .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
      .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
      .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
      .on_scroll(cx.listener(Self::on_scroll))
      .relative()
      .size_full()
      .overflow_hidden()
      .when(is_dragging, |el| el.cursor_grabbing())
      .when(!is_dragging && is_hovering_edge, |el| el.cursor_pointer())
      .when(!is_dragging && !is_hovering_edge, |el| el.cursor_grab())
      .bg(linear_gradient(
        180.,
        linear_color_stop(rgb(0x0d0e14), 0.0),
        linear_color_stop(rgb(0x040406), 1.0),
      ))
      .text_color(rgb(0xf4f6fb))
      .child(dot_grid)
      .children(
        self.nodes[first_visible_node..]
          .iter()
          .take_while(|node| node.position.y * zoom + pan.y <= viewport_rect.max_y)
          .filter(|node| {
            let screen_x = node.position.x * zoom + pan.x;
            let screen_y = node.position.y * zoom + pan.y;
            let est_width = node.estimated_cull_width(rem_size);
            let card_height = node.screen_height(rem_size);
            viewport_rect.intersects(
              screen_x,
              screen_y,
              screen_x + est_width,
              screen_y + card_height,
            )
          })
          .map(|node| {
            let screen_x = node.position.x * zoom + pan.x;
            let screen_y = node.position.y * zoom + pan.y;
            let body_content_top_y = node.body_content_top_y(screen_y, rem_size);
            let (visible_types, top_spacer_px, bottom_spacer_px) = visible_type_slice(
              &node.type_layouts,
              body_content_top_y,
              node.body_content_rem,
              node.body_content_px,
              rem_size,
              viewport_rect.min_y,
              viewport_rect.max_y,
            );

            div()
              .absolute()
              .left(px(screen_x))
              .top(px(screen_y))
              .min_w(rems(node.min_width_rem))
              .flex()
              .flex_col()
              .whitespace_nowrap()
              .overflow_hidden()
              .rounded_lg()
              .bg(rgb(0x11131a))
              .border_1()
              .border_color(rgba(0xffffff14))
              .shadow(vec![BoxShadow {
                inset: false,
                color: rgba(0x00000080).into(),
                offset: point(px(0.), px(8. * zoom)),
                blur_radius: px(20. * zoom),
                spread_radius: px(-4. * zoom),
              }])
              .child(
                div()
                  .px(rems(1.0))
                  .py(rems(HEADER_PAD_Y_REM))
                  .flex()
                  .items_center()
                  .bg(rgba(0xffffff05))
                  .border_b_1()
                  .border_color(rgba(0xffffff0e))
                  .text_sm()
                  .line_height(rems(HEADER_LINE_HEIGHT_REM))
                  .when(!node.dir_prefix.is_empty(), |el| {
                    el.child(
                      div()
                        .text_color(rgb(0x6b7280))
                        .child(node.dir_prefix.clone()),
                    )
                  })
                  .child(
                    div()
                      .font_weight(FontWeight::MEDIUM)
                      .text_color(rgb(0xf3f4f6))
                      .child(node.file_name.clone()),
                  ),
              )
              .child(
                div()
                  .px(rems(1.0))
                  .py(rems(BODY_PAD_Y_REM))
                  .flex()
                  .flex_col()
                  .gap(rems(TYPE_GAP_REM))
                  .when(top_spacer_px > 0.0, |el| el.child(div().h(px(top_spacer_px))))
                  .children(node.types[visible_types].iter().map(|type_node| {
                    let id = type_node.id;
                    let is_selected = selected_type == Some(id);
                    let is_edge_from = selected_edge_from == Some(id);
                    let is_edge_to = selected_edge_to == Some(id);
                    let is_connected = connected_types.contains(&id);

                    let (bg_color, border_color) = if is_selected {
                      (rgba(0x38bdf826), rgba(0x38bdf877))
                    } else if is_edge_from {
                      (rgba(0x38bdf820), rgba(0x38bdf866))
                    } else if is_edge_to {
                      (rgba(0x34d39920), rgba(0x34d39966))
                    } else if is_connected {
                      (rgba(0x7dd3fc12), rgba(0x7dd3fc33))
                    } else {
                      (rgba(0xffffff04), rgba(0xffffff0d))
                    };

                    let body_lines: Vec<String> = type_node
                      .body
                      .lines()
                      .map(|line| {
                        if line.is_empty() {
                          " ".to_owned()
                        } else {
                          line.to_owned()
                        }
                      })
                      .collect();

                    div()
                      .id(("type", id.0))
                      .cursor_pointer()
                      .px(rems(0.75))
                      .py(rems(TYPE_BLOCK_PAD_Y_REM))
                      .rounded_md()
                      .bg(bg_color)
                      .border_1()
                      .border_color(border_color)
                      .font_family("Menlo")
                      .text_size(rems(0.82))
                      .hover(|el| el.bg(rgba(0xffffff0c)))
                      .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                          cx.stop_propagation();
                          window.focus(&this.focus_handle, cx);
                          this.selected_edge = None;
                          this.selected_type = if this.selected_type == Some(id) {
                            None
                          } else {
                            Some(id)
                          };
                          cx.notify();
                        }),
                      )
                      .flex()
                      .flex_col()
                      .children(body_lines.into_iter().enumerate().map(|(line_idx, line)| {
                        let color = if line_idx == 0 {
                          rgb(0x7dd3fc)
                        } else {
                          rgb(0xd1d5db)
                        };
                        div()
                          .h(rems(TYPE_LINE_HEIGHT_REM))
                          .line_height(rems(TYPE_LINE_HEIGHT_REM))
                          .text_color(color)
                          .child(line)
                      }))
                  }))
                  .when(bottom_spacer_px > 0.0, |el| {
                    el.child(div().h(px(bottom_spacer_px)))
                  }),
              )
          }),
      )
      .child(edges_layer)
      .when_some(inspector_panel, |el, panel| el.child(panel))
      .when_some(
        match &self.lsp_status {
          LspStatus::Initialising => Some((rgba(0x38bdf8ff), "Initialising LSP...".to_owned())),
          LspStatus::Failed(err) => Some((rgba(0xf87171ff), format!("LSP failed: {err}"))),
          LspStatus::Ready => None,
        },
        |el, (dot_color, label)| {
          el.child(
            div()
              .absolute()
              .left(px(16.0))
              .bottom(px(16.0))
              .px(px(12.0))
              .py(px(6.0))
              .rounded(px(8.0))
              .bg(rgba(0x14161ee0))
              .border_1()
              .border_color(rgba(0xffffff20))
              .flex()
              .items_center()
              .gap(px(8.0))
              .text_size(px(12.0))
              .text_color(rgb(0xd8dde8))
              .child(div().w(px(7.0)).h(px(7.0)).rounded_full().bg(dot_color))
              .child(label),
          )
        },
      )
      .child(
        div()
          .absolute()
          .right(px(16.0))
          .bottom(px(16.0))
          .px(px(10.0))
          .py(px(5.0))
          .rounded(px(8.0))
          .bg(rgba(0x14161ee0))
          .border_1()
          .border_color(rgba(0xffffff20))
          .text_size(px(12.0))
          .text_color(rgb(0xc8cdd8))
          .child(hud_zoom_label),
      )
  }
}

pub fn run(workspace_root: PathBuf, graph: Graph) {
  application().run(move |cx: &mut App| {
    cx.bind_keys([
      KeyBinding::new("cmd-=", ZoomIn, None),
      KeyBinding::new("cmd-+", ZoomIn, None),
      KeyBinding::new("cmd--", ZoomOut, None),
      KeyBinding::new("cmd-0", ResetZoom, None),
      KeyBinding::new("escape", ClearSelection, None),
    ]);

    cx.on_window_closed(|cx, _window_id| {
      if cx.windows().is_empty() {
        cx.quit();
      }
    })
    .detach();

    let bounds = Bounds::centered(None, size(px(1080.0), px(760.0)), cx);
    if let Err(err) = cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
          appears_transparent: true,
          ..Default::default()
        }),
        ..Default::default()
      },
      |window, cx| {
        let view = cx.new(|cx| Application::new(workspace_root, graph, cx));
        let focus = view.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
        view
      },
    ) {
      eprintln!("Failed to open window: {err}");
    }

    cx.activate(true);
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn point_to_segment_distance_computes_exact_perpendicular() {
    let d_sq = point_to_segment_dist_sq(point(5.0, 3.0), point(0.0, 0.0), point(10.0, 0.0));
    assert!((d_sq - 9.0).abs() < 1e-4);
  }

  #[test]
  fn cubic_bezier_hit_detects_midpoint_and_rejects_far_point() {
    let p0 = point(200.0, 100.0);
    let c1 = point(100.0, 100.0);
    let c2 = point(100.0, 200.0);
    let p3 = point(200.0, 200.0);
    let mid = eval_cubic_bezier(p0, c1, c2, p3, 0.5);

    let near = min_dist_sq_to_cubic_bezier(point(mid.x + 2.0, mid.y), p0, c1, c2, p3, 32);
    assert!(near < 9.0);

    let far = min_dist_sq_to_cubic_bezier(point(mid.x + 40.0, mid.y), p0, c1, c2, p3, 32);
    assert!(far > 1000.0);
  }

  #[test]
  fn viewport_rect_respects_overscan_and_rejects_outside_boxes() {
    let vp = ViewportRect::from_size(1000.0, 800.0, 64.0, 32.0);
    // Inside overscan margin on left/top
    assert!(vp.intersects(-100.0, -50.0, -64.0, -32.0));
    // Just outside overscan margin on left
    assert!(!vp.intersects(-200.0, 100.0, -65.0, 200.0));
    // Just outside overscan margin on right
    assert!(!vp.intersects(1065.0, 100.0, 1200.0, 200.0));
    // Spanning vertically across the viewport
    assert!(vp.intersects(100.0, -500.0, 300.0, 1500.0));
  }

  #[test]
  fn edge_curve_aabb_keeps_spanning_edges_and_culls_offscreen() {
    let vp = ViewportRect::from_size(1000.0, 800.0, 64.0, 64.0);

    // Long vertical edge starting above viewport and ending below viewport
    let spanning = EdgeCurve {
      edge_id: EdgeId(0),
      p0: point(300.0, -200.0),
      c1: point(100.0, -200.0),
      c2: point(100.0, 1100.0),
      p3: point(296.0, 1100.0),
      arrow_tip: point(300.0, 1100.0),
    };
    let (min_x, min_y, max_x, max_y) = spanning.aabb(EDGE_CULL_PADDING_PX);
    assert!(vp.intersects(min_x, min_y, max_x, max_y));

    // Edge completely above the viewport
    let above = EdgeCurve {
      edge_id: EdgeId(1),
      p0: point(300.0, -500.0),
      c1: point(200.0, -500.0),
      c2: point(200.0, -300.0),
      p3: point(296.0, -300.0),
      arrow_tip: point(300.0, -300.0),
    };
    let (min_x, min_y, max_x, max_y) = above.aabb(EDGE_CULL_PADDING_PX);
    assert!(!vp.intersects(min_x, min_y, max_x, max_y));

    // Edge completely to the right of the viewport
    let right = EdgeCurve {
      edge_id: EdgeId(2),
      p0: point(1400.0, 200.0),
      c1: point(1300.0, 200.0),
      c2: point(1300.0, 300.0),
      p3: point(1396.0, 300.0),
      arrow_tip: point(1400.0, 300.0),
    };
    let (min_x, min_y, max_x, max_y) = right.aabb(EDGE_CULL_PADDING_PX);
    assert!(!vp.intersects(min_x, min_y, max_x, max_y));
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
      let reconstructed: f32 = child_heights.iter().sum::<f32>()
        + (child_heights.len().saturating_sub(1) as f32) * gap_px;

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
