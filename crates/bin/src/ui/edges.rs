use gpui::{
  Bounds, PathBuilder, Point, Rgba, Window, canvas, fill, point, prelude::*, px, rgba, size,
};
use graph::EdgeId;

use crate::app::Application;
use crate::geometry::{EdgeCurve, ViewportRect, min_dist_sq_to_cubic_bezier};
use crate::theme::{EDGE_CULL_PADDING_PX, EDGE_HIT_SEGMENTS};

#[derive(Clone, Copy)]
enum EdgeHighlight {
  Normal,
  Dimmed,
  Outgoing,
  Incoming,
  Hovered,
  Selected,
}

struct EdgeBatch {
  strokes: PathBuilder,
  arrows: PathBuilder,
  dots: Vec<(f32, f32)>,
  has_edges: bool,
  record_dots: bool,
}

impl EdgeBatch {
  fn new(stroke_width: f32, record_dots: bool) -> Self {
    Self {
      strokes: PathBuilder::stroke(px(stroke_width)),
      arrows: PathBuilder::fill(),
      dots: Vec::new(),
      has_edges: false,
      record_dots,
    }
  }

  fn push(&mut self, curve: EdgeCurve, origin: Point<f32>, arrow_len: f32, arrow_half: f32) {
    let p0 = point(origin.x + curve.p0.x, origin.y + curve.p0.y);
    let c1 = point(origin.x + curve.c1.x, origin.y + curve.c1.y);
    let c2 = point(origin.x + curve.c2.x, origin.y + curve.c2.y);
    let p3 = point(origin.x + curve.p3.x, origin.y + curve.p3.y);
    let tip = point(origin.x + curve.arrow_tip.x, origin.y + curve.arrow_tip.y);

    self.has_edges = true;
    if self.record_dots {
      self.dots.push((p0.x, p0.y));
    }

    self.strokes.move_to(point(px(p0.x), px(p0.y)));
    self.strokes.cubic_bezier_to(
      point(px(p3.x), px(p3.y)),
      point(px(c1.x), px(c1.y)),
      point(px(c2.x), px(c2.y)),
    );

    self.arrows.move_to(point(px(tip.x), px(tip.y)));
    self
      .arrows
      .line_to(point(px(tip.x - arrow_len), px(tip.y - arrow_half)));
    self
      .arrows
      .line_to(point(px(tip.x - arrow_len), px(tip.y + arrow_half)));
    self.arrows.close();
  }

  fn paint(self, window: &mut Window, dot_size: f32, stroke_color: Rgba, arrow_color: Rgba) {
    if !self.has_edges {
      return;
    }
    if let Ok(path) = self.strokes.build() {
      window.paint_path(path, stroke_color);
    }
    if let Ok(arrow_path) = self.arrows.build() {
      window.paint_path(arrow_path, arrow_color);
    }
    for (x, y) in self.dots {
      window.paint_quad(fill(
        Bounds::new(
          point(px(x - dot_size * 0.5), px(y - dot_size * 0.5)),
          size(px(dot_size), px(dot_size)),
        ),
        arrow_color,
      ));
    }
  }
}

impl Application {
  pub(crate) fn iter_edge_curves(
    &self,
    origin: Point<f32>,
    cull_rect: Option<(ViewportRect, f32)>,
  ) -> impl Iterator<Item = EdgeCurve> + '_ {
    let zoom = self.camera.zoom;
    let pan = self.camera.pan;
    let arrow_len = (7.0 * zoom).clamp(4.0, 12.0);

    self.edges.iter().filter_map(move |edge| {
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
  }

  pub(crate) fn hit_test_edge(&self, screen_pos: Point<f32>) -> Option<EdgeId> {
    let threshold = (9.0 * self.camera.zoom).clamp(7.0, 14.0);
    let cursor_rect = ViewportRect {
      min_x: screen_pos.x,
      min_y: screen_pos.y,
      max_x: screen_pos.x,
      max_y: screen_pos.y,
    };
    let threshold_sq = threshold * threshold;

    let mut best: Option<(EdgeId, f32)> = None;
    for curve in self.iter_edge_curves(point(0.0, 0.0), Some((cursor_rect, threshold))) {
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

  pub(crate) fn render_edges_layer(&self, viewport_rect: ViewportRect) -> impl IntoElement {
    let zoom = self.camera.zoom;
    let selected_type = self.selection.selected_type;
    let selected_edge_id = self.selection.selected_edge;
    let hovered_edge_id = self.selection.hovered_edge;

    let edge_cull_padding = EDGE_CULL_PADDING_PX * zoom.max(1.0);
    let styled_curves: Vec<(EdgeCurve, EdgeHighlight)> = self
      .iter_edge_curves(point(0.0, 0.0), Some((viewport_rect, edge_cull_padding)))
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

    canvas(
      |_bounds, _window, _cx| {},
      move |bounds, _, window, _cx| {
        if styled_curves.is_empty() {
          return;
        }

        let origin = point(f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let normal_width = (1.5 * zoom).clamp(1.0, 3.0);
        let highlight_width = (2.4 * zoom).clamp(1.5, 4.5);
        let selected_width = (3.0 * zoom).clamp(2.0, 5.5);
        let arrow_len = (7.0 * zoom).clamp(4.0, 12.0);
        let arrow_half = (3.5 * zoom).clamp(2.0, 6.0);
        let dot_size = (5.0 * zoom).clamp(3.0, 8.0);

        let mut dimmed = EdgeBatch::new(normal_width, false);
        let mut normal = EdgeBatch::new(normal_width, true);
        let mut incoming = EdgeBatch::new(highlight_width, true);
        let mut outgoing = EdgeBatch::new(highlight_width, true);
        let mut hovered = EdgeBatch::new(highlight_width, true);
        let mut selected = EdgeBatch::new(selected_width, true);

        for &(curve, highlight) in &styled_curves {
          let batch = match highlight {
            EdgeHighlight::Dimmed => &mut dimmed,
            EdgeHighlight::Normal => &mut normal,
            EdgeHighlight::Incoming => &mut incoming,
            EdgeHighlight::Outgoing => &mut outgoing,
            EdgeHighlight::Hovered => &mut hovered,
            EdgeHighlight::Selected => &mut selected,
          };
          batch.push(curve, origin, arrow_len, arrow_half);
        }

        dimmed.paint(window, dot_size, rgba(0x7dd3fc12), rgba(0x7dd3fc24));
        normal.paint(window, dot_size, rgba(0x7dd3fc55), rgba(0x7dd3fccc));
        incoming.paint(window, dot_size, rgba(0x34d399ee), rgba(0x34d399ff));
        outgoing.paint(window, dot_size, rgba(0x38bdf8ee), rgba(0x38bdf8ff));
        hovered.paint(window, dot_size, rgba(0xfbbf24dd), rgba(0xfbbf24ff));
        selected.paint(window, dot_size, rgba(0xfbbf24ff), rgba(0xfde68aff));
      },
    )
    .absolute()
    .inset_0()
    .size_full()
  }
}
