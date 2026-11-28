use gpui::{
  Context, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollEvent,
  Window, actions, point, px,
};

use crate::app::Application;
use crate::theme::{BASE_REM_SIZE, DEFAULT_PAN_X, DEFAULT_PAN_Y, MAX_ZOOM, MIN_ZOOM};

actions!(viewer, [ZoomIn, ZoomOut, ResetZoom, ClearSelection]);

#[derive(Clone, Copy, Debug)]
pub struct Camera {
  pub pan: Point<f32>,
  pub zoom: f32,
  pub drag_last_pos: Option<Point<f32>>,
}

impl Default for Camera {
  fn default() -> Self {
    Self {
      pan: point(DEFAULT_PAN_X, DEFAULT_PAN_Y),
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

  pub fn zoom_around(&mut self, new_zoom: f32, focal: Point<f32>) -> bool {
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
    self.pan = point(DEFAULT_PAN_X, DEFAULT_PAN_Y);
  }
}

impl Application {
  pub(crate) fn set_zoom_around(
    &mut self,
    new_zoom: f32,
    focal: Point<f32>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if self.camera.zoom_around(new_zoom, focal) {
      window.set_rem_size(px(self.camera.rem_size()));
      cx.notify();
    }
  }

  pub(crate) fn zoom_in(&mut self, _: &ZoomIn, window: &mut Window, cx: &mut Context<Self>) {
    let viewport = window.viewport_size();
    let center = point(
      f32::from(viewport.width) * 0.5,
      f32::from(viewport.height) * 0.5,
    );
    self.set_zoom_around(self.camera.zoom * 1.15, center, window, cx);
  }

  pub(crate) fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
    let viewport = window.viewport_size();
    let center = point(
      f32::from(viewport.width) * 0.5,
      f32::from(viewport.height) * 0.5,
    );
    self.set_zoom_around(self.camera.zoom / 1.15, center, window, cx);
  }

  pub(crate) fn reset_zoom(&mut self, _: &ResetZoom, window: &mut Window, cx: &mut Context<Self>) {
    self.camera.reset();
    window.set_rem_size(px(BASE_REM_SIZE));
    cx.notify();
  }

  pub(crate) fn clear_selection(
    &mut self,
    _: &ClearSelection,
    _window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let ended_drag = self.inspector.end_drag();
    if self.selection.clear() || ended_drag {
      cx.notify();
    }
  }

  pub(crate) fn on_mouse_down(
    &mut self,
    event: &MouseDownEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    window.focus(&self.focus_handle, cx);
    let pos = point(f32::from(event.position.x), f32::from(event.position.y));

    if event.button == MouseButton::Left
      && let Some(edge_id) = self.hit_test_edge(pos)
    {
      self.selection.toggle_edge(edge_id);
      cx.notify();
      return;
    }

    if event.button == MouseButton::Left || event.button == MouseButton::Middle {
      self.selection.clear();
      self.camera.drag_last_pos = Some(pos);
      cx.notify();
    }
  }

  pub(crate) fn on_mouse_move(
    &mut self,
    event: &MouseMoveEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let current = point(f32::from(event.position.x), f32::from(event.position.y));

    if self.inspector.is_dragging() {
      let viewport = window.viewport_size();
      if self.inspector.update_drag(
        current,
        f32::from(viewport.width),
        f32::from(viewport.height),
      ) {
        cx.notify();
      }
      return;
    }

    if let Some(last) = self.camera.drag_last_pos {
      self.camera.pan.x += current.x - last.x;
      self.camera.pan.y += current.y - last.y;
      self.camera.drag_last_pos = Some(current);
      cx.notify();
    } else {
      let hovered = self.hit_test_edge(current);
      if hovered != self.selection.hovered_edge {
        self.selection.hovered_edge = hovered;
        cx.notify();
      }
    }
  }

  pub(crate) fn on_mouse_up(
    &mut self,
    _event: &MouseUpEvent,
    _window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let inspector_ended = self.inspector.end_drag();
    let camera_ended = self.camera.drag_last_pos.take().is_some();
    if inspector_ended || camera_ended {
      cx.notify();
    }
  }

  pub(crate) fn on_scroll(
    &mut self,
    event: &ScrollEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let delta: Point<Pixels> = event.delta.pixel_delta(px(20.0));
    let dx = f32::from(delta.x);
    let dy = f32::from(delta.y);

    if event.modifiers.platform || event.modifiers.control {
      let factor = (1.0 + dy * 0.004).clamp(0.75, 1.25);
      let cursor = point(f32::from(event.position.x), f32::from(event.position.y));
      self.set_zoom_around(self.camera.zoom * factor, cursor, window, cx);
    } else {
      self.camera.pan.x += dx;
      self.camera.pan.y += dy;
      cx.notify();
    }
  }
}
