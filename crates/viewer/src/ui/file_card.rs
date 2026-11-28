use gpui::{
  AnyElement, BoxShadow, Context, FontWeight, MouseButton, div, point, prelude::*, px, rems, rgb,
  rgba,
};

use crate::app::Application;
use crate::geometry::ViewportRect;
use crate::layout::visible_type_slice;
use crate::theme::{
  BODY_PAD_Y_REM, HEADER_LINE_HEIGHT_REM, HEADER_PAD_Y_REM, MONO_FONT, TYPE_BLOCK_PAD_Y_REM,
  TYPE_GAP_REM, TYPE_LINE_HEIGHT_REM,
};

impl Application {
  pub(crate) fn render_visible_file_cards(
    &self,
    viewport_rect: ViewportRect,
    cx: &mut Context<Self>,
  ) -> Vec<AnyElement> {
    let pan = self.camera.pan;
    let zoom = self.camera.zoom;
    let rem_size = self.camera.rem_size();

    let selected_type = self.selection.selected_type;
    let selected_edge_ref = self
      .selection
      .selected_edge
      .and_then(|id| self.edges.get(id.0));
    let selected_edge_from = selected_edge_ref.map(|e| e.from);
    let selected_edge_to = selected_edge_ref.map(|e| e.to);
    let connected_types = &self.selection.connected_types;

    let first_visible_node = self.nodes.partition_point(|node| {
      let screen_y = node.position.y * zoom + pan.y;
      screen_y + node.screen_height(rem_size) < viewport_rect.min_y
    });

    self.nodes[first_visible_node..]
      .iter()
      .take_while(|node| node.position.y * zoom + pan.y <= viewport_rect.max_y)
      .filter(|node| {
        let screen_x = node.position.x * zoom + pan.x;
        let screen_y = node.position.y * zoom + pan.y;
        let est_width = node.estimated_cull_width(rem_size);
        let card_height = node.screen_height(rem_size);
        viewport_rect.intersects(
          screen_x,
          screen_y,
          screen_x + est_width,
          screen_y + card_height,
        )
      })
      .map(|node| {
        let screen_x = node.position.x * zoom + pan.x;
        let screen_y = node.position.y * zoom + pan.y;
        let body_content_top_y = node.body_content_top_y(screen_y, rem_size);
        let (visible_types, top_spacer_px, bottom_spacer_px) = visible_type_slice(
          &node.type_layouts,
          body_content_top_y,
          node.body_content_rem,
          node.body_content_px,
          rem_size,
          viewport_rect.min_y,
          viewport_rect.max_y,
        );

        div()
          .absolute()
          .left(px(screen_x))
          .top(px(screen_y))
          .min_w(rems(node.min_width_rem))
          .flex()
          .flex_col()
          .whitespace_nowrap()
          .overflow_hidden()
          .rounded_lg()
          .bg(rgb(0x11131a))
          .border_1()
          .border_color(rgba(0xffffff14))
          .shadow(vec![BoxShadow {
            inset: false,
            color: rgba(0x00000080).into(),
            offset: point(px(0.), px(8. * zoom)),
            blur_radius: px(20. * zoom),
            spread_radius: px(-4. * zoom),
          }])
          .child(
            div()
              .px(rems(1.0))
              .py(rems(HEADER_PAD_Y_REM))
              .flex()
              .items_center()
              .bg(rgba(0xffffff05))
              .border_b_1()
              .border_color(rgba(0xffffff0e))
              .text_sm()
              .line_height(rems(HEADER_LINE_HEIGHT_REM))
              .when(!node.dir_prefix.is_empty(), |el| {
                el.child(
                  div()
                    .text_color(rgb(0x6b7280))
                    .child(node.dir_prefix.clone()),
                )
              })
              .child(
                div()
                  .font_weight(FontWeight::MEDIUM)
                  .text_color(rgb(0xf3f4f6))
                  .child(node.file_name.clone()),
              ),
          )
          .child(
            div()
              .px(rems(1.0))
              .py(rems(BODY_PAD_Y_REM))
              .flex()
              .flex_col()
              .gap(rems(TYPE_GAP_REM))
              .when(top_spacer_px > 0.0, |el| {
                el.child(div().h(px(top_spacer_px)))
              })
              .children(node.types[visible_types].iter().map(|type_block| {
                let id = type_block.id;
                let is_selected = selected_type == Some(id);
                let is_edge_from = selected_edge_from == Some(id);
                let is_edge_to = selected_edge_to == Some(id);
                let is_connected = connected_types.contains(&id);

                let (bg_color, border_color) = if is_selected {
                  (rgba(0x38bdf826), rgba(0x38bdf877))
                } else if is_edge_from {
                  (rgba(0x38bdf820), rgba(0x38bdf866))
                } else if is_edge_to {
                  (rgba(0x34d39920), rgba(0x34d39966))
                } else if is_connected {
                  (rgba(0x7dd3fc12), rgba(0x7dd3fc33))
                } else {
                  (rgba(0xffffff04), rgba(0xffffff0d))
                };

                div()
                  .id(("type", id.0))
                  .cursor_pointer()
                  .px(rems(0.75))
                  .py(rems(TYPE_BLOCK_PAD_Y_REM))
                  .rounded_md()
                  .bg(bg_color)
                  .border_1()
                  .border_color(border_color)
                  .font_family(MONO_FONT)
                  .text_size(rems(0.82))
                  .hover(|el| el.bg(rgba(0xffffff0c)))
                  .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                      cx.stop_propagation();
                      window.focus(&this.focus_handle, cx);
                      this.selection.toggle_type(id, &this.edges);
                      cx.notify();
                    }),
                  )
                  .flex()
                  .flex_col()
                  .child(
                    div()
                      .h(rems(TYPE_LINE_HEIGHT_REM))
                      .line_height(rems(TYPE_LINE_HEIGHT_REM))
                      .text_color(rgb(0x7dd3fc))
                      .child(type_block.header_line.clone()),
                  )
                  .when_some(type_block.rest_body.as_ref(), |el, rest| {
                    el.child(
                      div()
                        .h(rems(
                          TYPE_LINE_HEIGHT_REM * type_block.rest_line_count as f32,
                        ))
                        .line_height(rems(TYPE_LINE_HEIGHT_REM))
                        .text_color(rgb(0xd1d5db))
                        .child(rest.clone()),
                    )
                  })
              }))
              .when(bottom_spacer_px > 0.0, |el| {
                el.child(div().h(px(bottom_spacer_px)))
              }),
          )
          .into_any_element()
      })
      .collect()
  }
}
