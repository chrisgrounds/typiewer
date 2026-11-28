use std::collections::{HashMap, HashSet};

use graph::{EdgeId, GraphEdge, TypeId};

use crate::geometry::{EdgeCurve, Point2, ViewportRect, min_dist_sq_to_cubic_bezier, point2};
use crate::metrics::EDGE_HIT_SEGMENTS;

pub fn iter_edge_curves<'a>(
  edges: &'a [GraphEdge],
  type_anchors: &'a HashMap<TypeId, Point2>,
  directed_pairs: &'a HashSet<(TypeId, TypeId)>,
  pan: Point2,
  zoom: f32,
  origin: Point2,
  cull_rect: Option<(ViewportRect, f32)>,
) -> impl Iterator<Item = EdgeCurve> + 'a {
  let arrow_len = (7.0 * zoom).clamp(4.0, 12.0);

  edges.iter().filter_map(move |edge| {
    let from = *type_anchors.get(&edge.from)?;
    let to = *type_anchors.get(&edge.to)?;

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

    let curve = EdgeCurve {
      edge_id: edge.id,
      p0: point2(from_x, from_y),
      c1: point2(from_x - arc, from_y),
      c2: point2(to_x - arc, to_y),
      p3: point2(to_x - arrow_len * 0.5, to_y),
      arrow_tip: point2(to_x, to_y),
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

pub fn hit_test_edge(
  edges: &[GraphEdge],
  type_anchors: &HashMap<TypeId, Point2>,
  directed_pairs: &HashSet<(TypeId, TypeId)>,
  pan: Point2,
  zoom: f32,
  screen_pos: Point2,
) -> Option<EdgeId> {
  let threshold = (9.0 * zoom).clamp(7.0, 14.0);
  let cursor_rect = ViewportRect {
    min_x: screen_pos.x,
    min_y: screen_pos.y,
    max_x: screen_pos.x,
    max_y: screen_pos.y,
  };
  let threshold_sq = threshold * threshold;

  let mut best: Option<(EdgeId, f32)> = None;
  for curve in iter_edge_curves(
    edges,
    type_anchors,
    directed_pairs,
    pan,
    zoom,
    point2(0.0, 0.0),
    Some((cursor_rect, threshold)),
  ) {
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
