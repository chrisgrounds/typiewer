use gpui::{Point, point};
use graph::EdgeId;

#[derive(Clone, Copy, Debug)]
pub struct ViewportRect {
  pub min_x: f32,
  pub min_y: f32,
  pub max_x: f32,
  pub max_y: f32,
}

impl ViewportRect {
  pub fn from_size(width: f32, height: f32, overscan_x: f32, overscan_y: f32) -> Self {
    Self {
      min_x: -overscan_x,
      min_y: -overscan_y,
      max_x: width + overscan_x,
      max_y: height + overscan_y,
    }
  }

  pub fn intersects(&self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> bool {
    max_x >= self.min_x && min_x <= self.max_x && max_y >= self.min_y && min_y <= self.max_y
  }
}

#[derive(Clone, Copy)]
pub struct EdgeCurve {
  pub edge_id: EdgeId,
  pub p0: Point<f32>,
  pub c1: Point<f32>,
  pub c2: Point<f32>,
  pub p3: Point<f32>,
  pub arrow_tip: Point<f32>,
}

impl EdgeCurve {
  pub fn aabb(&self, padding: f32) -> (f32, f32, f32, f32) {
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

pub fn eval_cubic_bezier(
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

pub fn point_to_segment_dist_sq(p: Point<f32>, a: Point<f32>, b: Point<f32>) -> f32 {
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

pub fn min_dist_sq_to_cubic_bezier(
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::theme::EDGE_CULL_PADDING_PX;

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
}
