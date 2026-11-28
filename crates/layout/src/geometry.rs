use std::ops::{Add, AddAssign, Sub};

use graph::EdgeId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point2 {
  pub x: f32,
  pub y: f32,
}

impl Point2 {
  pub const fn new(x: f32, y: f32) -> Self {
    Self { x, y }
  }
}

pub const fn point2(x: f32, y: f32) -> Point2 {
  Point2 { x, y }
}

/// A 1D layout measurement combining a zoom-scaled `rem` component and a fixed `px` component
/// (e.g. 1px borders that do not scale with `rem_size`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RemPx {
  pub rem: f32,
  pub px: f32,
}

impl RemPx {
  pub const ZERO: Self = Self { rem: 0.0, px: 0.0 };

  pub const fn new(rem: f32, px: f32) -> Self {
    Self { rem, px }
  }

  pub const fn rems(rem: f32) -> Self {
    Self { rem, px: 0.0 }
  }

  pub fn to_px(self, rem_size: f32) -> f32 {
    self.rem * rem_size + self.px
  }
}

impl Add for RemPx {
  type Output = Self;

  fn add(self, rhs: Self) -> Self {
    Self {
      rem: self.rem + rhs.rem,
      px: self.px + rhs.px,
    }
  }
}

impl AddAssign for RemPx {
  fn add_assign(&mut self, rhs: Self) {
    self.rem += rhs.rem;
    self.px += rhs.px;
  }
}

impl Sub for RemPx {
  type Output = Self;

  fn sub(self, rhs: Self) -> Self {
    Self {
      rem: self.rem - rhs.rem,
      px: self.px - rhs.px,
    }
  }
}

/// Vertical position and height of a stacked child within its parent's body content area.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VerticalSpan {
  pub offset: RemPx,
  pub height: RemPx,
}

impl VerticalSpan {
  pub fn screen_top(self, container_top_y: f32, rem_size: f32) -> f32 {
    container_top_y + self.offset.to_px(rem_size)
  }

  pub fn screen_height(self, rem_size: f32) -> f32 {
    self.height.to_px(rem_size)
  }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
  pub min_x: f32,
  pub min_y: f32,
  pub max_x: f32,
  pub max_y: f32,
}

impl Rect {
  pub fn from_viewport(width: f32, height: f32, overscan_x: f32, overscan_y: f32) -> Self {
    Self {
      min_x: -overscan_x,
      min_y: -overscan_y,
      max_x: width + overscan_x,
      max_y: height + overscan_y,
    }
  }

  pub const fn from_point(p: Point2) -> Self {
    Self {
      min_x: p.x,
      min_y: p.y,
      max_x: p.x,
      max_y: p.y,
    }
  }

  pub fn from_xywh(x: f32, y: f32, width: f32, height: f32) -> Self {
    Self {
      min_x: x,
      min_y: y,
      max_x: x + width,
      max_y: y + height,
    }
  }

  pub fn intersects(&self, other: Rect) -> bool {
    other.max_x >= self.min_x
      && other.min_x <= self.max_x
      && other.max_y >= self.min_y
      && other.min_y <= self.max_y
  }
}

#[derive(Clone, Copy)]
pub struct EdgeCurve {
  pub edge_id: EdgeId,
  pub p0: Point2,
  pub c1: Point2,
  pub c2: Point2,
  pub p3: Point2,
  pub arrow_tip: Point2,
}

impl EdgeCurve {
  pub fn aabb(&self, padding: f32) -> Rect {
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
    Rect {
      min_x,
      min_y,
      max_x,
      max_y,
    }
  }
}

pub fn eval_cubic_bezier(p0: Point2, c1: Point2, c2: Point2, p3: Point2, t: f32) -> Point2 {
  let u = 1.0 - t;
  let uu = u * u;
  let tt = t * t;
  let uuu = uu * u;
  let ttt = tt * t;
  point2(
    uuu * p0.x + 3.0 * uu * t * c1.x + 3.0 * u * tt * c2.x + ttt * p3.x,
    uuu * p0.y + 3.0 * uu * t * c1.y + 3.0 * u * tt * c2.y + ttt * p3.y,
  )
}

pub fn point_to_segment_dist_sq(p: Point2, a: Point2, b: Point2) -> f32 {
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
  p: Point2,
  p0: Point2,
  c1: Point2,
  c2: Point2,
  p3: Point2,
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
  use crate::metrics::EDGE_CULL_PADDING_PX;

  #[test]
  fn point_to_segment_distance_computes_exact_perpendicular() {
    let d_sq = point_to_segment_dist_sq(point2(5.0, 3.0), point2(0.0, 0.0), point2(10.0, 0.0));
    assert!((d_sq - 9.0).abs() < 1e-4);
  }

  #[test]
  fn cubic_bezier_hit_detects_midpoint_and_rejects_far_point() {
    let p0 = point2(200.0, 100.0);
    let c1 = point2(100.0, 100.0);
    let c2 = point2(100.0, 200.0);
    let p3 = point2(200.0, 200.0);
    let mid = eval_cubic_bezier(p0, c1, c2, p3, 0.5);

    let near = min_dist_sq_to_cubic_bezier(point2(mid.x + 2.0, mid.y), p0, c1, c2, p3, 32);
    assert!(near < 9.0);

    let far = min_dist_sq_to_cubic_bezier(point2(mid.x + 40.0, mid.y), p0, c1, c2, p3, 32);
    assert!(far > 1000.0);
  }

  #[test]
  fn rect_respects_overscan_and_rejects_outside_boxes() {
    let vp = Rect::from_viewport(1000.0, 800.0, 64.0, 32.0);
    assert!(vp.intersects(Rect {
      min_x: -100.0,
      min_y: -50.0,
      max_x: -64.0,
      max_y: -32.0,
    }));
    assert!(!vp.intersects(Rect {
      min_x: -200.0,
      min_y: 100.0,
      max_x: -65.0,
      max_y: 200.0,
    }));
    assert!(!vp.intersects(Rect {
      min_x: 1065.0,
      min_y: 100.0,
      max_x: 1200.0,
      max_y: 200.0,
    }));
    assert!(vp.intersects(Rect {
      min_x: 100.0,
      min_y: -500.0,
      max_x: 300.0,
      max_y: 1500.0,
    }));
  }

  #[test]
  fn edge_curve_aabb_keeps_spanning_edges_and_culls_offscreen() {
    let vp = Rect::from_viewport(1000.0, 800.0, 64.0, 64.0);

    let spanning = EdgeCurve {
      edge_id: EdgeId(0),
      p0: point2(300.0, -200.0),
      c1: point2(100.0, -200.0),
      c2: point2(100.0, 1100.0),
      p3: point2(296.0, 1100.0),
      arrow_tip: point2(300.0, 1100.0),
    };
    assert!(vp.intersects(spanning.aabb(EDGE_CULL_PADDING_PX)));

    let above = EdgeCurve {
      edge_id: EdgeId(1),
      p0: point2(300.0, -500.0),
      c1: point2(200.0, -500.0),
      c2: point2(200.0, -300.0),
      p3: point2(296.0, -300.0),
      arrow_tip: point2(300.0, -300.0),
    };
    assert!(!vp.intersects(above.aabb(EDGE_CULL_PADDING_PX)));

    let right = EdgeCurve {
      edge_id: EdgeId(2),
      p0: point2(1400.0, 200.0),
      c1: point2(1300.0, 200.0),
      c2: point2(1300.0, 300.0),
      p3: point2(1396.0, 300.0),
      arrow_tip: point2(1400.0, 300.0),
    };
    assert!(!vp.intersects(right.aabb(EDGE_CULL_PADDING_PX)));
  }
}
