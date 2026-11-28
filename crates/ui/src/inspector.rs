use std::path::Path;

use gpui::{
  AnyElement, BoxShadow, Context, FontWeight, MouseButton, MouseDownEvent, Pixels, SharedString,
  Size, Window, div, point, prelude::*, px, rgb, rgba,
};
use graph::{FunctionId, Graph, TypeId, relative_display_path};
use layout::Point2;
use state::{INSPECTOR_WIDTH, InspectorState, Selection};

use crate::theme::{
  AMBER_TEXT, EMERALD_TEXT, GRAY_500, MONO_FONT, SKY_TEXT, SLATE_400, pluralize, to_point2,
};

#[derive(Clone, Copy, Debug)]
pub enum InspectorEvent {
  StartDrag(Point2),
  SelectType(TypeId),
  Close,
}

#[derive(Clone, Copy)]
struct PillSpec {
  heading: &'static str,
  id_prefix: &'static str,
  bg: u32,
  border: u32,
  hover_bg: u32,
  fg: u32,
}

const INPUT_PILLS: PillSpec = PillSpec {
  heading: "Inputs:",
  id_prefix: "inspector-in",
  bg: 0x38bdf820,
  border: 0x38bdf844,
  hover_bg: 0x38bdf838,
  fg: SKY_TEXT,
};

const OUTPUT_PILLS: PillSpec = PillSpec {
  heading: "Outputs:",
  id_prefix: "inspector-out",
  bg: 0x34d39920,
  border: 0x34d39944,
  hover_bg: 0x34d39938,
  fg: EMERALD_TEXT,
};

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

fn render_header_type_badge<V: 'static>(
  id: &'static str,
  type_id: TypeId,
  name: String,
  bg_rgba: u32,
  fg_rgb: u32,
  cx: &mut Context<V>,
  on_event: impl Fn(&mut V, InspectorEvent, &mut Window, &mut Context<V>) + Copy + 'static,
) -> AnyElement {
  div()
    .id(id)
    .cursor_pointer()
    .px(px(7.0))
    .py(px(2.0))
    .rounded(px(5.0))
    .bg(rgba(bg_rgba))
    .text_size(px(12.0))
    .font_weight(FontWeight::MEDIUM)
    .text_color(rgb(fg_rgb))
    .on_mouse_down(
      MouseButton::Left,
      cx.listener(move |this, _, window, cx| {
        cx.stop_propagation();
        on_event(this, InspectorEvent::SelectType(type_id), window, cx);
      }),
    )
    .child(name)
    .into_any_element()
}

fn render_pill_row<V: 'static>(
  spec: PillSpec,
  func_id: FunctionId,
  type_ids: &[TypeId],
  graph: &Graph,
  cx: &mut Context<V>,
  on_event: impl Fn(&mut V, InspectorEvent, &mut Window, &mut Context<V>) + Copy + 'static,
) -> Option<AnyElement> {
  let pills: Vec<(TypeId, String)> = type_ids
    .iter()
    .filter_map(|&tid| graph.ty(tid).map(|t| (tid, format!("{} {}", t.kind, t.name))))
    .collect();

  if pills.is_empty() {
    return None;
  }

  Some(
    div()
      .flex()
      .flex_wrap()
      .items_center()
      .gap(px(6.0))
      .child(
        div()
          .text_size(px(11.0))
          .text_color(rgb(SLATE_400))
          .child(spec.heading),
      )
      .children(pills.into_iter().map(|(tid, label)| {
        div()
          .id(SharedString::from(format!(
            "{}:{}:{}",
            spec.id_prefix, func_id.0, tid.0
          )))
          .cursor_pointer()
          .px(px(8.0))
          .py(px(2.0))
          .rounded(px(6.0))
          .bg(rgba(spec.bg))
          .border_1()
          .border_color(rgba(spec.border))
          .text_size(px(11.0))
          .text_color(rgb(spec.fg))
          .hover(move |el| el.bg(rgba(spec.hover_bg)))
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
              cx.stop_propagation();
              on_event(this, InspectorEvent::SelectType(tid), window, cx);
            }),
          )
          .child(label)
      }))
      .into_any_element(),
  )
}

pub fn render_inspector_panel<V: 'static>(
  canonical_root: &Path,
  graph: &Graph,
  selection: &Selection,
  inspector: &InspectorState,
  viewport: Size<Pixels>,
  cx: &mut Context<V>,
  on_event: impl Fn(&mut V, InspectorEvent, &mut Window, &mut Context<V>) + Copy + 'static,
) -> Option<AnyElement> {
  let edge = selection.selected_edge.and_then(|id| graph.edge(id))?;
  let from_type = graph.ty(edge.from)?;
  let to_type = graph.ty(edge.to)?;
  let count_label = pluralize(edge.functions.len(), "function", "functions");

  let function_cards: Vec<_> = edge
    .functions
    .iter()
    .filter_map(|&fid| graph.func(fid))
    .map(|func| {
      let location = format!(
        "{}:{}",
        relative_display_path(canonical_root, &func.file_path),
        func.position.line + 1
      );
      let input_row = render_pill_row(INPUT_PILLS, func.id, &func.input_types, graph, cx, on_event);
      let output_row =
        render_pill_row(OUTPUT_PILLS, func.id, &func.output_types, graph, cx, on_event);

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
                .text_color(rgb(AMBER_TEXT))
                .child(format!("fn {}", func.name)),
            )
            .child(
              div()
                .text_size(px(11.0))
                .text_color(rgb(GRAY_500))
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
            .child(func.signature()),
        )
        .when_some(input_row, |el, row| el.child(row))
        .when_some(output_row, |el, row| el.child(row))
    })
    .collect();

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
              on_event(
                this,
                InspectorEvent::StartDrag(to_point2(event.position)),
                window,
                cx,
              );
            }),
          )
          .child(
            div()
              .flex()
              .flex_wrap()
              .items_center()
              .gap(px(6.0))
              .child(render_drag_grip())
              .child(render_header_type_badge(
                "inspector-from-type",
                from_type.id,
                from_type.name.clone(),
                0x38bdf822,
                SKY_TEXT,
                cx,
                on_event,
              ))
              .child(
                div()
                  .text_size(px(12.0))
                  .text_color(rgb(0xfbbf24))
                  .child("→"),
              )
              .child(render_header_type_badge(
                "inspector-to-type",
                to_type.id,
                to_type.name.clone(),
                0x34d39922,
                EMERALD_TEXT,
                cx,
                on_event,
              ))
              .child(
                div()
                  .text_size(px(11.0))
                  .text_color(rgb(SLATE_400))
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
              .text_color(rgb(SLATE_400))
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
