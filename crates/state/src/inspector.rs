use layout::{Point2, point2};

pub const INSPECTOR_WIDTH: f32 = 440.0;
const INSPECTOR_DEFAULT_RIGHT: f32 = 16.0;
const INSPECTOR_DEFAULT_TOP: f32 = 44.0;
const INSPECTOR_EDGE_MARGIN: f32 = 8.0;
const INSPECTOR_MIN_VISIBLE_HEIGHT: f32 = 48.0;

#[derive(Default)]
pub struct InspectorState {
  pub position: Option<Point2>,
  pub drag_offset: Option<Point2>,
}

impl InspectorState {
  pub fn is_dragging(&self) -> bool {
    self.drag_offset.is_some()
  }

  pub fn default_position(viewport_width: f32) -> Point2 {
    let x = (viewport_width - INSPECTOR_WIDTH - INSPECTOR_DEFAULT_RIGHT).max(INSPECTOR_EDGE_MARGIN);
    point2(x, INSPECTOR_DEFAULT_TOP)
  }

  pub fn clamp_position(pos: Point2, viewport_width: f32, viewport_height: f32) -> Point2 {
    let max_x =
      (viewport_width - INSPECTOR_WIDTH - INSPECTOR_EDGE_MARGIN).max(INSPECTOR_EDGE_MARGIN);
    let max_y = (viewport_height - INSPECTOR_MIN_VISIBLE_HEIGHT).max(INSPECTOR_EDGE_MARGIN);
    point2(
      pos.x.clamp(INSPECTOR_EDGE_MARGIN, max_x),
      pos.y.clamp(INSPECTOR_EDGE_MARGIN, max_y),
    )
  }

  pub fn resolved_position(&self, viewport_width: f32, viewport_height: f32) -> Point2 {
    let raw = self
      .position
      .unwrap_or_else(|| Self::default_position(viewport_width));
    Self::clamp_position(raw, viewport_width, viewport_height)
  }

  pub fn start_drag(&mut self, cursor: Point2, viewport_width: f32, viewport_height: f32) {
    let current = self.resolved_position(viewport_width, viewport_height);
    self.position = Some(current);
    self.drag_offset = Some(point2(cursor.x - current.x, cursor.y - current.y));
  }

  pub fn update_drag(&mut self, cursor: Point2, viewport_width: f32, viewport_height: f32) -> bool {
    let Some(offset) = self.drag_offset else {
      return false;
    };
    let next = Self::clamp_position(
      point2(cursor.x - offset.x, cursor.y - offset.y),
      viewport_width,
      viewport_height,
    );
    self.position = Some(next);
    true
  }

  pub fn end_drag(&mut self) -> bool {
    self.drag_offset.take().is_some()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_position_aligns_to_top_right_margin() {
    let pos = InspectorState::default_position(1080.0);
    assert_eq!(
      pos,
      point2(
        1080.0 - INSPECTOR_WIDTH - INSPECTOR_DEFAULT_RIGHT,
        INSPECTOR_DEFAULT_TOP
      )
    );
  }

  #[test]
  fn drag_moves_panel_and_clamps_to_viewport() {
    let mut state = InspectorState::default();
    let vw = 1080.0;
    let vh = 760.0;
    let initial = state.resolved_position(vw, vh);

    state.start_drag(point2(initial.x + 20.0, initial.y + 10.0), vw, vh);
    assert!(state.is_dragging());

    assert!(state.update_drag(point2(initial.x - 100.0, initial.y + 110.0), vw, vh));
    assert_eq!(
      state.resolved_position(vw, vh),
      point2(initial.x - 120.0, initial.y + 100.0)
    );

    // Dragging past top-left clamps to the minimum margin.
    assert!(state.update_drag(point2(-500.0, -500.0), vw, vh));
    assert_eq!(
      state.resolved_position(vw, vh),
      point2(INSPECTOR_EDGE_MARGIN, INSPECTOR_EDGE_MARGIN)
    );

    // Dragging past bottom-right keeps the panel reachable inside the viewport.
    assert!(state.update_drag(point2(5000.0, 5000.0), vw, vh));
    assert_eq!(
      state.resolved_position(vw, vh),
      point2(
        vw - INSPECTOR_WIDTH - INSPECTOR_EDGE_MARGIN,
        vh - INSPECTOR_MIN_VISIBLE_HEIGHT
      )
    );

    assert!(state.end_drag());
    assert!(!state.is_dragging());
    assert!(!state.update_drag(point2(200.0, 200.0), vw, vh));
  }

  #[test]
  fn clamp_handles_viewport_smaller_than_inspector() {
    let pos = InspectorState::clamp_position(point2(100.0, 100.0), 300.0, 30.0);
    assert_eq!(pos, point2(INSPECTOR_EDGE_MARGIN, INSPECTOR_EDGE_MARGIN));
  }
}
