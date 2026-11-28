use graph::{EdgeId, Graph};

use crate::file_layout::WorkspaceLayout;
use crate::geometry::{EdgeCurve, Point2, Rect, min_dist_sq_to_cubic_bezier, point2};
use crate::metrics::EDGE_HIT_SEGMENTS;

pub fn arrow_len(zoom: f32) -> f32 {
  (7.0 * zoom).clamp(4.0, 12.0)
}

pub fn iter_edge_curves<'a>(
  graph: &'a Graph,
  layout: &'a WorkspaceLayout,
  pan: Point2,
  zoom: f32,
  cull_rect: Option<(Rect, f32)>,
) -> impl Iterator<Item = EdgeCurve> + 'a {
  let tip_inset = arrow_len(zoom) * 0.5;

  graph.edges.iter().filter_map(move |edge| {
    if let (Some(from_target), Some(to_target)) = (
      layout.collapsed_targets.get(&edge.from),
      layout.collapsed_targets.get(&edge.to),
    ) && from_target == to_target
    {
      return None;
    }

    let from = *layout.type_anchors.get(&edge.from)?;
    let to = *layout.type_anchors.get(&edge.to)?;

    let from_x = from.x * zoom + pan.x;
    let from_y = from.y * zoom + pan.y;
    let to_x = to.x * zoom + pan.x;
    let to_y = to.y * zoom + pan.y;

    let dy = (to_y - from_y).abs();
    let bidirectional_offset =
      if graph.directed_pairs.contains(&(edge.to, edge.from)) && edge.from > edge.to {
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
      p3: point2(to_x - tip_inset, to_y),
      arrow_tip: point2(to_x, to_y),
    };

    if let Some((rect, padding)) = cull_rect
      && !rect.intersects(curve.aabb(padding))
    {
      return None;
    }

    Some(curve)
  })
}

pub fn hit_test_edge(
  graph: &Graph,
  layout: &WorkspaceLayout,
  pan: Point2,
  zoom: f32,
  screen_pos: Point2,
) -> Option<EdgeId> {
  let threshold = (9.0 * zoom).clamp(7.0, 14.0);
  let cursor_rect = Rect::from_point(screen_pos);
  let threshold_sq = threshold * threshold;

  let mut best: Option<(EdgeId, f32)> = None;
  for curve in iter_edge_curves(graph, layout, pan, zoom, Some((cursor_rect, threshold))) {
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
