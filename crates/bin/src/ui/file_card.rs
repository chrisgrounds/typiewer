use gpui::{
  AnyElement, BoxShadow, Context, FontWeight, MouseButton, div, point, prelude::*, px, rems, rgb,
  rgba,
};

use crate::app::Application;
use crate::geometry::ViewportRect;
use crate::layout::{FileNode, visible_file_slice, visible_type_slice};
use crate::theme::{
  BODY_PAD_Y_REM, CRATE_BODY_PAD_X_REM, CRATE_BODY_PAD_Y_REM, CRATE_HEADER_LINE_HEIGHT_REM,
  CRATE_HEADER_PAD_Y_REM, FILE_GAP_REM, HEADER_LINE_HEIGHT_REM, HEADER_PAD_Y_REM, MONO_FONT,
  TYPE_BLOCK_PAD_Y_REM, TYPE_GAP_REM, TYPE_LINE_HEIGHT_REM,
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

    let first_visible_crate = self.crates.partition_point(|crate_node| {
      let screen_y = crate_node.position.y * zoom + pan.y;
      screen_y + crate_node.screen_height(rem_size) < viewport_rect.min_y
    });

    self.crates[first_visible_crate..]
      .iter()
      .enumerate()
      .map(|(offset, crate_node)| (first_visible_crate + offset, crate_node))
      .take_while(|(_, crate_node)| crate_node.position.y * zoom + pan.y <= viewport_rect.max_y)
      .filter(|(_, crate_node)| {
        let screen_x = crate_node.position.x * zoom + pan.x;
        let screen_y = crate_node.position.y * zoom + pan.y;
        let est_width = crate_node.estimated_cull_width(rem_size);
        let crate_height = crate_node.screen_height(rem_size);
        viewport_rect.intersects(
          screen_x,
          screen_y,
          screen_x + est_width,
          screen_y + crate_height,
        )
      })
      .map(|(crate_idx, crate_node)| {
        let screen_x = crate_node.position.x * zoom + pan.x;
        let screen_y = crate_node.position.y * zoom + pan.y;
        let is_collapsed = crate_node.collapsed;
        let chevron = if is_collapsed { "▸" } else { "▾" };
        let file_count = crate_node.files.len();
        let file_label = if file_count == 1 { "file" } else { "files" };
        let type_label = if crate_node.total_type_count == 1 {
          "type"
        } else {
          "types"
        };
        let summary = format!(
          "{file_count} {file_label} · {} {type_label}",
          crate_node.total_type_count
        );

        let body = if is_collapsed {
          None
        } else {
          let crate_body_top_y = crate_node.body_content_top_y(screen_y, rem_size);
          let (visible_files, top_spacer_px, bottom_spacer_px) = visible_file_slice(
            &crate_node.files,
            crate_body_top_y,
            crate_node.body_content_rem,
            crate_node.body_content_px,
            rem_size,
            viewport_rect.min_y,
            viewport_rect.max_y,
          );

          let rendered_files: Vec<AnyElement> = crate_node.files[visible_files.clone()]
            .iter()
            .enumerate()
            .map(|(offset, file_node)| {
              let file_idx = visible_files.start + offset;
              let file_screen_y = file_node.screen_top(crate_body_top_y, rem_size);
              self.render_file_card(
                crate_idx,
                file_idx,
                file_node,
                file_screen_y,
                viewport_rect,
                cx,
              )
            })
            .collect();

          Some(
            div()
              .px(rems(CRATE_BODY_PAD_X_REM))
              .py(rems(CRATE_BODY_PAD_Y_REM))
              .flex()
              .flex_col()
              .gap(rems(FILE_GAP_REM))
              .when(top_spacer_px > 0.0, |el| {
                el.child(div().h(px(top_spacer_px)))
              })
              .children(rendered_files)
              .when(bottom_spacer_px > 0.0, |el| {
                el.child(div().h(px(bottom_spacer_px)))
              }),
          )
        };

        div()
          .absolute()
          .left(px(screen_x))
          .top(px(screen_y))
          .min_w(rems(crate_node.min_width_rem))
          .flex()
          .flex_col()
          .whitespace_nowrap()
          .overflow_hidden()
          .rounded_xl()
          .bg(rgba(0x0b0d14e6))
          .border_1()
          .border_color(rgba(0x38bdf82c))
          .shadow(vec![BoxShadow {
            inset: false,
            color: rgba(0x00000088).into(),
            offset: point(px(0.), px(10. * zoom)),
            blur_radius: px(24. * zoom),
            spread_radius: px(-4. * zoom),
          }])
          .child(
            div()
              .id(("crate-header", crate_idx))
              .cursor_pointer()
              .px(rems(1.0))
              .py(rems(CRATE_HEADER_PAD_Y_REM))
              .flex()
              .items_center()
              .justify_between()
              .gap(rems(1.25))
              .bg(rgba(0x38bdf80d))
              .hover(|el| el.bg(rgba(0x38bdf818)))
              .when(!is_collapsed, |el| {
                el.border_b_1().border_color(rgba(0x38bdf822))
              })
              .line_height(rems(CRATE_HEADER_LINE_HEIGHT_REM))
              .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                  cx.stop_propagation();
                  window.focus(&this.focus_handle, cx);
                  this.toggle_crate_collapsed(crate_idx);
                  cx.notify();
                }),
              )
              .child(
                div()
                  .flex()
                  .items_center()
                  .gap(rems(0.5))
                  .child(
                    div()
                      .w(rems(0.75))
                      .text_sm()
                      .text_color(rgb(0x7dd3fc))
                      .child(chevron),
                  )
                  .child(
                    div()
                      .px(rems(0.4))
                      .rounded_sm()
                      .bg(rgba(0x38bdf81a))
                      .text_xs()
                      .font_weight(FontWeight::MEDIUM)
                      .text_color(rgb(0x7dd3fc))
                      .child("crate"),
                  )
                  .child(
                    div()
                      .text_sm()
                      .font_weight(FontWeight::SEMIBOLD)
                      .text_color(rgb(0xf8fafc))
                      .child(crate_node.name.clone()),
                  )
                  .when(!crate_node.rel_dir.is_empty(), |el| {
                    el.child(
                      div()
                        .text_xs()
                        .text_color(rgb(0x6b7280))
                        .child(crate_node.rel_dir.clone()),
                    )
                  }),
              )
              .child(div().text_xs().text_color(rgb(0x64748b)).child(summary)),
          )
          .when_some(body, |el, content| el.child(content))
          .into_any_element()
      })
      .collect()
  }

  fn render_file_card(
    &self,
    crate_idx: usize,
    file_idx: usize,
    node: &FileNode,
    card_screen_y: f32,
    viewport_rect: ViewportRect,
    cx: &mut Context<Self>,
  ) -> AnyElement {
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

    let is_collapsed = node.collapsed;
    let chevron = if is_collapsed { "▸" } else { "▾" };
    let type_count = node.types.len();
    let count_label = if type_count == 1 {
      "1 type".to_owned()
    } else {
      format!("{type_count} types")
    };

    let body = if is_collapsed {
      None
    } else {
      let body_content_top_y = node.body_content_top_y(card_screen_y, rem_size);
      let (visible_types, top_spacer_px, bottom_spacer_px) = visible_type_slice(
        &node.type_layouts,
        body_content_top_y,
        node.body_content_rem,
        node.body_content_px,
        rem_size,
        viewport_rect.min_y,
        viewport_rect.max_y,
      );

      Some(
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
    };

    div()
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
        color: rgba(0x00000070).into(),
        offset: point(px(0.), px(6. * zoom)),
        blur_radius: px(16. * zoom),
        spread_radius: px(-4. * zoom),
      }])
      .child(
        div()
          .id(("file-header", crate_idx * 65_536 + file_idx))
          .cursor_pointer()
          .px(rems(1.0))
          .py(rems(HEADER_PAD_Y_REM))
          .flex()
          .items_center()
          .justify_between()
          .gap(rems(1.0))
          .bg(rgba(0xffffff05))
          .hover(|el| el.bg(rgba(0xffffff0d)))
          .when(!is_collapsed, |el| {
            el.border_b_1().border_color(rgba(0xffffff0e))
          })
          .text_sm()
          .line_height(rems(HEADER_LINE_HEIGHT_REM))
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
              cx.stop_propagation();
              window.focus(&this.focus_handle, cx);
              this.toggle_file_collapsed(crate_idx, file_idx);
              cx.notify();
            }),
          )
          .child(
            div()
              .flex()
              .items_center()
              .child(div().w(rems(0.9)).text_color(rgb(0x94a3b8)).child(chevron))
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
          .child(div().text_xs().text_color(rgb(0x64748b)).child(count_label)),
      )
      .when_some(body, |el, content| el.child(content))
      .into_any_element()
  }
}
