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

const EDGE_HIT_SEGMENTS: usize = 32;

enum LspStatus {
  Initialising,
  Ready,
  Failed(String),
}

struct FileNode {
  dir_prefix: String,
  file_name: String,
  types: Vec<TypeNode>,
  position: Point<f32>,
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

pub struct Application {
  canonical_root: PathBuf,
  nodes: Vec<FileNode>,
  types: Vec<TypeNode>,
  functions: Vec<FunctionInfo>,
  edges: Vec<GraphEdge>,
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

    let header_height: f32 =
      (HEADER_PAD_Y_REM * 2.0 + HEADER_LINE_HEIGHT_REM) * BASE_REM_SIZE + 2.0;
    let body_padding_y: f32 = BODY_PAD_Y_REM * BASE_REM_SIZE;
    let type_block_pad_y: f32 = TYPE_BLOCK_PAD_Y_REM * BASE_REM_SIZE;
    let type_line_height: f32 = TYPE_LINE_HEIGHT_REM * BASE_REM_SIZE;
    let type_gap: f32 = TYPE_GAP_REM * BASE_REM_SIZE;
    let bottom_border: f32 = 1.0;

    for (file_path, file_types) in grouped {
      let mut local_y = header_height + body_padding_y;

      for (idx, type_node) in file_types.iter().enumerate() {
        let lines = type_node.body.lines().count().max(1) as f32;
        let block_height = type_block_pad_y * 2.0 + lines * type_line_height + 2.0;
        type_anchors.insert(
          type_node.id,
          point(0.0, cursor_y + local_y + block_height * 0.5),
        );
        local_y += block_height;
        if idx + 1 < file_types.len() {
          local_y += type_gap;
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

      let card_height = local_y + body_padding_y + bottom_border;
      nodes.push(FileNode {
        dir_prefix,
        file_name,
        types: file_types,
        position: point(0.0, cursor_y),
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

    Self {
      canonical_root,
      nodes,
      types: all_types,
      functions: graph.functions,
      edges: graph.edges,
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

  fn compute_edge_curves(&self, origin: Point<f32>) -> Vec<EdgeCurve> {
    let directed_pairs: HashSet<(TypeId, TypeId)> =
      self.edges.iter().map(|e| (e.from, e.to)).collect();
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
          if directed_pairs.contains(&(edge.to, edge.from)) && edge.from > edge.to {
            34.0 * zoom
          } else {
            0.0
          };
        let arc = (54.0 * zoom + dy * 0.30 + bidirectional_offset).clamp(54.0 * zoom, 460.0 * zoom);

        Some(EdgeCurve {
          edge_id: edge.id,
          p0: point(from_x, from_y),
          c1: point(from_x - arc, from_y),
          c2: point(to_x - arc, to_y),
          p3: point(to_x - arrow_len * 0.5, to_y),
          arrow_tip: point(to_x, to_y),
        })
      })
      .collect()
  }

  fn hit_test_edge(&self, screen_pos: Point<f32>) -> Option<EdgeId> {
    let curves = self.compute_edge_curves(point(0.0, 0.0));
    let threshold = (9.0 * self.zoom).clamp(7.0, 14.0);
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
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let pan = self.pan;
    let zoom = self.zoom;
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

    let edge_curves = self.compute_edge_curves(point(0.0, 0.0));
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
        let mut normal_strokes = PathBuilder::stroke(px(normal_width));
        let mut normal_arrows = PathBuilder::fill();
        let mut incoming_strokes = PathBuilder::stroke(px(highlight_width));
        let mut incoming_arrows = PathBuilder::fill();
        let mut outgoing_strokes = PathBuilder::stroke(px(highlight_width));
        let mut outgoing_arrows = PathBuilder::fill();
        let mut hovered_strokes = PathBuilder::stroke(px(highlight_width));
        let mut hovered_arrows = PathBuilder::fill();
        let mut selected_strokes = PathBuilder::stroke(px(selected_width));
        let mut selected_arrows = PathBuilder::fill();

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
            EdgeHighlight::Dimmed => (&mut dimmed_strokes, &mut dimmed_arrows),
            EdgeHighlight::Normal => {
              normal_dots.push((p0.x, p0.y));
              (&mut normal_strokes, &mut normal_arrows)
            }
            EdgeHighlight::Incoming => {
              incoming_dots.push((p0.x, p0.y));
              (&mut incoming_strokes, &mut incoming_arrows)
            }
            EdgeHighlight::Outgoing => {
              outgoing_dots.push((p0.x, p0.y));
              (&mut outgoing_strokes, &mut outgoing_arrows)
            }
            EdgeHighlight::Hovered => {
              hovered_dots.push((p0.x, p0.y));
              (&mut hovered_strokes, &mut hovered_arrows)
            }
            EdgeHighlight::Selected => {
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
                           strokes: PathBuilder,
                           arrows: PathBuilder,
                           dots: &[(f32, f32)],
                           stroke_color,
                           arrow_color| {
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
          dimmed_strokes,
          dimmed_arrows,
          &[],
          rgba(0x7dd3fc12),
          rgba(0x7dd3fc24),
        );
        paint_layer(
          window,
          normal_strokes,
          normal_arrows,
          &normal_dots,
          rgba(0x7dd3fc55),
          rgba(0x7dd3fccc),
        );
        paint_layer(
          window,
          incoming_strokes,
          incoming_arrows,
          &incoming_dots,
          rgba(0x34d399ee),
          rgba(0x34d399ff),
        );
        paint_layer(
          window,
          outgoing_strokes,
          outgoing_arrows,
          &outgoing_dots,
          rgba(0x38bdf8ee),
          rgba(0x38bdf8ff),
        );
        paint_layer(
          window,
          hovered_strokes,
          hovered_arrows,
          &hovered_dots,
          rgba(0xfbbf24dd),
          rgba(0xfbbf24ff),
        );
        paint_layer(
          window,
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
      .children(self.nodes.iter().map(|node| {
        let screen_x = node.position.x * zoom + pan.x;
        let screen_y = node.position.y * zoom + pan.y;

        div()
          .absolute()
          .left(px(screen_x))
          .top(px(screen_y))
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
              .children(node.types.iter().map(|type_node| {
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
              })),
          )
      }))
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
}
