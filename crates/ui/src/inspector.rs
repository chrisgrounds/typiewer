use std::path::Path;

use gpui::{
  AnyElement, BoxShadow, Context, FontWeight, MouseButton, MouseDownEvent, Pixels, Size, Window,
  div, point, prelude::*, px, rgb, rgba,
};
use graph::{FunctionInfo, GraphEdge, TypeId, TypeNode};
use layout::{Point2, point2};
use state::{INSPECTOR_WIDTH, InspectorState, Selection};

use crate::theme::MONO_FONT;

#[derive(Clone, Copy, Debug)]
pub enum InspectorEvent {
  StartDrag(Point2),
  SelectType(TypeId),
  Close,
}

fn relative_display_path(canonical_root: &Path, path: &Path) -> String {
  path
    .strip_prefix(canonical_root)
    .unwrap_or(path)
    .display()
    .to_string()
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

#[allow(clippy::too_many_arguments)]
pub fn render_inspector_panel<V: 'static>(
  canonical_root: &Path,
  types: &[TypeNode],
  functions: &[FunctionInfo],
  edges: &[GraphEdge],
  selection: &Selection,
  inspector: &InspectorState,
  viewport: Size<Pixels>,
  cx: &mut Context<V>,
  on_event: impl Fn(&mut V, InspectorEvent, &mut Window, &mut Context<V>) + Copy + 'static,
) -> Option<AnyElement> {
  let edge = selection.selected_edge.and_then(|id| edges.get(id.0))?;
  let from_type = types.get(edge.from.0)?;
  let to_type = types.get(edge.to.0)?;
  let fn_count = edge.functions.len();
  let count_label = if fn_count == 1 {
    "1 function".to_owned()
  } else {
    format!("{fn_count} functions")
  };

  let function_cards: Vec<_> = edge
    .functions
    .iter()
    .filter_map(|fid| functions.get(fid.0))
    .map(|func| {
      let sig = func.signature();
      let location = format!(
        "{}:{}",
        relative_display_path(canonical_root, &func.file_path),
        func.position.line + 1
      );
      let input_pills: Vec<(TypeId, String)> = func
        .input_types
        .iter()
        .filter_map(|tid| {
          types
            .get(tid.0)
            .map(|t| (*tid, format!("{} {}", t.kind, t.name)))
        })
        .collect();
      let output_pills: Vec<(TypeId, String)> = func
        .output_types
        .iter()
        .filter_map(|tid| {
          types
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
                      on_event(this, InspectorEvent::SelectType(tid), window, cx);
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
                      on_event(this, InspectorEvent::SelectType(tid), window, cx);
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
  let is_dragging = inspector.is_dragging();
  let panel_pos = inspector.resolved_position(f32::from(viewport.width), f32::from(viewport.height));

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
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
              cx.stop_propagation();
              let cursor = point2(f32::from(event.position.x), f32::from(event.position.y));
              on_event(this, InspectorEvent::StartDrag(cursor), window, cx);
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
                      on_event(this, InspectorEvent::SelectType(from_id), window, cx);
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
                      on_event(this, InspectorEvent::SelectType(to_id), window, cx);
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
                cx.listener(move |this, _, window, cx| {
                  cx.stop_propagation();
                  on_event(this, InspectorEvent::Close, window, cx);
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
