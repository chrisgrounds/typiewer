use layout::metrics::{BASE_REM_SIZE, DEFAULT_PAN_X, DEFAULT_PAN_Y, MAX_ZOOM, MIN_ZOOM};
use layout::{Point2, point2};

#[derive(Clone, Copy, Debug)]
pub struct Camera {
  pub pan: Point2,
  pub zoom: f32,
  pub drag_last_pos: Option<Point2>,
}

impl Default for Camera {
  fn default() -> Self {
    Self {
      pan: point2(DEFAULT_PAN_X, DEFAULT_PAN_Y),
      zoom: 1.0,
      drag_last_pos: None,
    }
  }
}

impl Camera {
  pub fn rem_size(&self) -> f32 {
    BASE_REM_SIZE * self.zoom
  }

  pub fn is_dragging(&self) -> bool {
    self.drag_last_pos.is_some()
  }

  pub fn zoom_around(&mut self, new_zoom: f32, focal: Point2) -> bool {
    let clamped = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    if (clamped - self.zoom).abs() < f32::EPSILON {
      return false;
    }

    let world_x = (focal.x - self.pan.x) / self.zoom;
    let world_y = (focal.y - self.pan.y) / self.zoom;

    self.zoom = clamped;
    self.pan.x = focal.x - world_x * self.zoom;
    self.pan.y = focal.y - world_y * self.zoom;
    true
  }

  pub fn focus_on(&mut self, world_anchor: Point2, viewport_height: f32) {
    self.drag_last_pos = None;
    self.pan.x = DEFAULT_PAN_X - world_anchor.x * self.zoom;
    self.pan.y = viewport_height * 0.5 - world_anchor.y * self.zoom;
  }

  pub fn reset(&mut self) {
    self.zoom = 1.0;
    self.pan = point2(DEFAULT_PAN_X, DEFAULT_PAN_Y);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn focus_on_centers_world_anchor_vertically_at_default_pan_x() {
    let mut camera = Camera {
      pan: point2(10.0, 20.0),
      zoom: 1.5,
      drag_last_pos: Some(point2(1.0, 2.0)),
    };

    camera.focus_on(point2(0.0, 400.0), 800.0);

    assert!(camera.drag_last_pos.is_none());
    assert!((camera.pan.x - DEFAULT_PAN_X).abs() < 1e-4);
    assert!((camera.pan.y - (400.0 - 400.0 * 1.5)).abs() < 1e-4);
  }
}
