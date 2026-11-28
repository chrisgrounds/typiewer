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

  pub fn reset(&mut self) {
    self.zoom = 1.0;
    self.pan = point2(DEFAULT_PAN_X, DEFAULT_PAN_Y);
  }
}
