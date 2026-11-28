use gpui::{
  AnyElement, BoxShadow, Context, FontWeight, MouseButton, MouseDownEvent, Pixels, Point, Size,
  div, point, prelude::*, px, rgb, rgba,
};
use graph::TypeId;

use crate::app::Application;
use crate::theme::MONO_FONT;

const INSPECTOR_WIDTH: f32 = 440.0;
const INSPECTOR_DEFAULT_RIGHT: f32 = 16.0;
const INSPECTOR_DEFAULT_TOP: f32 = 44.0;
const INSPECTOR_EDGE_MARGIN: f32 = 8.0;
const INSPECTOR_MIN_VISIBLE_HEIGHT: f32 = 48.0;

#[derive(Default)]
pub struct InspectorState {
  pub position: Option<Point<f32>>,
  pub drag_offset: Option<Point<f32>>,
}

impl InspectorState {
  pub fn is_dragging(&self) -> bool {
    self.drag_offset.is_some()
  }

  pub fn default_position(viewport_width: f32) -> Point<f32> {
    let x = (viewport_width - INSPECTOR_WIDTH - INSPECTOR_DEFAULT_RIGHT).max(INSPECTOR_EDGE_MARGIN);
    point(x, INSPECTOR_DEFAULT_TOP)
  }

  pub fn clamp_position(pos: Point<f32>, viewport_width: f32, viewport_height: f32) -> Point<f32> {
    let max_x =
      (viewport_width - INSPECTOR_WIDTH - INSPECTOR_EDGE_MARGIN).max(INSPECTOR_EDGE_MARGIN);
    let max_y = (viewport_height - INSPECTOR_MIN_VISIBLE_HEIGHT).max(INSPECTOR_EDGE_MARGIN);
    point(
      pos.x.clamp(INSPECTOR_EDGE_MARGIN, max_x),
      pos.y.clamp(INSPECTOR_EDGE_MARGIN, max_y),
    )
  }

  pub fn resolved_position(&self, viewport_width: f32, viewport_height: f32) -> Point<f32> {
    let raw = self
      .position
      .unwrap_or_else(|| Self::default_position(viewport_width));
    Self::clamp_position(raw, viewport_width, viewport_height)
  }

  pub fn start_drag(&mut self, cursor: Point<f32>, viewport_width: f32, viewport_height: f32) {
    let current = self.resolved_position(viewport_width, viewport_height);
    self.position = Some(current);
    self.drag_offset = Some(point(cursor.x - current.x, cursor.y - current.y));
  }

  pub fn update_drag(
    &mut self,
    cursor: Point<f32>,
    viewport_width: f32,
    viewport_height: f32,
  ) -> bool {
    let Some(offset) = self.drag_offset else {
      return false;
    };
    let next = Self::clamp_position(
      point(cursor.x - offset.x, cursor.y - offset.y),
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

fn render_drag_grip() -> impl IntoElement {
  let dot = || {
    div()
      .w(px(2.5))
      .h(px(2.5))
      .rounded_full()
      .bg(rgba(0x94a3b866))
  };
  let col = || {
    div()
      .flex()
      .flex_col()
      .gap(px(2.5))
      .child(dot())
      .child(dot())
      .child(dot())
  };

  div()
    .flex()
    .items_center()
    .gap(px(2.5))
    .pr(px(2.0))
    .child(col())
    .child(col())
}

impl Application {
  pub(crate) fn render_inspector_panel(
    &self,
    viewport: Size<Pixels>,
    cx: &mut Context<Self>,
  ) -> Option<AnyElement> {
    let edge = self
      .selection
      .selected_edge
      .and_then(|id| self.edges.get(id.0))?;
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
              .font_family(MONO_FONT)
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
                        this.selection.select_type(tid, &this.edges);
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
                        this.selection.select_type(tid, &this.edges);
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
    let is_dragging = self.inspector.is_dragging();
    let panel_pos = self
      .inspector
      .resolved_position(f32::from(viewport.width), f32::from(viewport.height));

    Some(
      div()
        .id("edge-inspector")
        .absolute()
        .left(px(panel_pos.x))
        .top(px(panel_pos.y))
        .w(px(INSPECTOR_WIDTH))
        .max_h(px(540.0))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(12.0))
        .bg(rgba(0x11131af2))
        .border_1()
        .border_color(rgba(0xfbbf2444))
        .when(is_dragging, |el| el.cursor_grabbing())
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
            .id("inspector-header")
            .px(px(14.0))
            .py(px(10.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .bg(rgba(0xffffff06))
            .border_b_1()
            .border_color(rgba(0xffffff12))
            .when(is_dragging, |el| el.cursor_grabbing())
            .when(!is_dragging, |el| el.cursor_grab())
            .on_mouse_down(
              MouseButton::Left,
              cx.listener(|this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&this.focus_handle, cx);
                let viewport = window.viewport_size();
                let cursor = point(f32::from(event.position.x), f32::from(event.position.y));
                this.selection.hovered_edge = None;
                this.inspector.start_drag(
                  cursor,
                  f32::from(viewport.width),
                  f32::from(viewport.height),
                );
                cx.notify();
              }),
            )
            .child(
              div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.0))
                .child(render_drag_grip())
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
                        this.inspector.end_drag();
                        this.selection.select_type(from_id, &this.edges);
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
                        this.inspector.end_drag();
                        this.selection.select_type(to_id, &this.edges);
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
                    this.inspector.end_drag();
                    this.selection.selected_edge = None;
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
        )
        .into_any_element(),
    )
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
      point(
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

    state.start_drag(point(initial.x + 20.0, initial.y + 10.0), vw, vh);
    assert!(state.is_dragging());

    assert!(state.update_drag(point(initial.x - 100.0, initial.y + 110.0), vw, vh));
    assert_eq!(
      state.resolved_position(vw, vh),
      point(initial.x - 120.0, initial.y + 100.0)
    );

    // Dragging past top-left clamps to the minimum margin.
    assert!(state.update_drag(point(-500.0, -500.0), vw, vh));
    assert_eq!(
      state.resolved_position(vw, vh),
      point(INSPECTOR_EDGE_MARGIN, INSPECTOR_EDGE_MARGIN)
    );

    // Dragging past bottom-right keeps the panel reachable inside the viewport.
    assert!(state.update_drag(point(5000.0, 5000.0), vw, vh));
    assert_eq!(
      state.resolved_position(vw, vh),
      point(
        vw - INSPECTOR_WIDTH - INSPECTOR_EDGE_MARGIN,
        vh - INSPECTOR_MIN_VISIBLE_HEIGHT
      )
    );

    assert!(state.end_drag());
    assert!(!state.is_dragging());
    assert!(!state.update_drag(point(200.0, 200.0), vw, vh));
  }

  #[test]
  fn clamp_handles_viewport_smaller_than_inspector() {
    let pos = InspectorState::clamp_position(point(100.0, 100.0), 300.0, 30.0);
    assert_eq!(pos, point(INSPECTOR_EDGE_MARGIN, INSPECTOR_EDGE_MARGIN));
  }
}
