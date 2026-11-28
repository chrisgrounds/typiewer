use gpui::{
  AnyElement, BoxShadow, Context, FontWeight, MouseButton, SharedString, Window, div, point,
  prelude::*, px, rems, rgb, rgba,
};
use graph::{Graph, TypeId};
use layout::metrics::{
  BODY_PAD_Y_REM, CRATE_BODY_PAD_X_REM, CRATE_BODY_PAD_Y_REM, CRATE_HEADER_LINE_HEIGHT_REM,
  CRATE_HEADER_PAD_Y_REM, FILE_GAP_REM, HEADER_LINE_HEIGHT_REM, HEADER_PAD_Y_REM,
  TYPE_BLOCK_PAD_Y_REM, TYPE_GAP_REM, TYPE_LINE_HEIGHT_REM,
};
use layout::{FileNode, Rect, WorkspaceLayout};
use state::{Camera, CommentsState, Selection};

use crate::comment_box::{CommentBoxEvent, render_type_comments};
use crate::theme::{GRAY_500, MONO_FONT, SKY_TEXT, SLATE_400, SLATE_500, pluralize};

#[derive(Clone, Copy, Debug)]
pub enum FileCardEvent {
  SelectType(TypeId),
  ToggleCrate(usize),
  ToggleFile(usize, usize),
  SaveComment,
  CancelComment,
}

struct CardRenderCtx<'a> {
  graph: &'a Graph,
  camera: Camera,
  selection: &'a Selection,
  comments: &'a CommentsState,
  viewport_rect: Rect,
}

#[allow(clippy::too_many_arguments)]
pub fn render_visible_file_cards<V: 'static>(
  graph: &Graph,
  layout: &WorkspaceLayout,
  camera: Camera,
  selection: &Selection,
  comments: &CommentsState,
  viewport_rect: Rect,
  cx: &mut Context<V>,
  on_event: impl Fn(&mut V, FileCardEvent, &mut Window, &mut Context<V>) + Copy + 'static,
) -> Vec<AnyElement> {
  let pan = camera.pan;
  let zoom = camera.zoom;
  let rem_size = camera.rem_size();
  let vp_y = (viewport_rect.min_y, viewport_rect.max_y);
  let rctx = CardRenderCtx {
    graph,
    camera,
    selection,
    comments,
    viewport_rect,
  };

  let first_visible_crate = layout.crates.partition_point(|crate_node| {
    let screen_y = crate_node.position.y * zoom + pan.y;
    screen_y + crate_node.screen_height(rem_size) < viewport_rect.min_y
  });

  layout.crates[first_visible_crate..]
    .iter()
    .enumerate()
    .map(|(offset, crate_node)| (first_visible_crate + offset, crate_node))
    .take_while(|(_, crate_node)| crate_node.position.y * zoom + pan.y <= viewport_rect.max_y)
    .filter(|(_, crate_node)| {
      let screen_x = crate_node.position.x * zoom + pan.x;
      let screen_y = crate_node.position.y * zoom + pan.y;
      viewport_rect.intersects(Rect::from_xywh(
        screen_x,
        screen_y,
        crate_node.estimated_cull_width(rem_size),
        crate_node.screen_height(rem_size),
      ))
    })
    .map(|(crate_idx, crate_node)| {
      let screen_x = crate_node.position.x * zoom + pan.x;
      let screen_y = crate_node.position.y * zoom + pan.y;
      let is_collapsed = crate_node.collapsed;
      let chevron = if is_collapsed { "▸" } else { "▾" };
      let summary = format!(
        "{} · {}",
        pluralize(crate_node.files.len(), "file", "files"),
        pluralize(crate_node.total_type_count, "type", "types"),
      );

      let body = (!is_collapsed).then(|| {
        let crate_body_top_y = crate_node.body_content_top_y(screen_y, rem_size);
        let (visible_files, top_spacer_px, bottom_spacer_px) =
          crate_node.visible_files(crate_body_top_y, rem_size, vp_y);

        let rendered_files: Vec<AnyElement> = crate_node.files[visible_files.clone()]
          .iter()
          .enumerate()
          .map(|(offset, file_node)| {
            let file_idx = visible_files.start + offset;
            let file_screen_y = file_node.screen_top(crate_body_top_y, rem_size);
            render_file_card(
              crate_idx,
              file_idx,
              file_node,
              file_screen_y,
              &rctx,
              cx,
              on_event,
            )
          })
          .collect();

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
          })
      });

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
                on_event(this, FileCardEvent::ToggleCrate(crate_idx), window, cx);
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
                    .text_color(rgb(SKY_TEXT))
                    .child(chevron),
                )
                .child(
                  div()
                    .px(rems(0.4))
                    .rounded_sm()
                    .bg(rgba(0x38bdf81a))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(SKY_TEXT))
                    .child("crate"),
                )
                .child(
                  div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0xf8fafc))
                    .child(SharedString::from(crate_node.name.clone())),
                )
                .when(!crate_node.rel_dir.is_empty(), |el| {
                  el.child(
                    div()
                      .text_xs()
                      .text_color(rgb(GRAY_500))
                      .child(SharedString::from(crate_node.rel_dir.clone())),
                  )
                }),
            )
            .child(div().text_xs().text_color(rgb(SLATE_500)).child(summary)),
        )
        .when_some(body, |el, content| el.child(content))
        .into_any_element()
    })
    .collect()
}

fn render_file_card<V: 'static>(
  crate_idx: usize,
  file_idx: usize,
  node: &FileNode,
  card_screen_y: f32,
  rctx: &CardRenderCtx<'_>,
  cx: &mut Context<V>,
  on_event: impl Fn(&mut V, FileCardEvent, &mut Window, &mut Context<V>) + Copy + 'static,
) -> AnyElement {
  let zoom = rctx.camera.zoom;
  let rem_size = rctx.camera.rem_size();

  let selected_type = rctx.selection.selected_type;
  let selected_edge_ref = rctx.selection.selected_edge.and_then(|id| rctx.graph.edge(id));
  let selected_edge_from = selected_edge_ref.map(|e| e.from);
  let selected_edge_to = selected_edge_ref.map(|e| e.to);
  let connected_types = &rctx.selection.connected_types;

  let is_collapsed = node.collapsed;
  let chevron = if is_collapsed { "▸" } else { "▾" };
  let count_label = pluralize(node.types.len(), "type", "types");

  let body = (!is_collapsed).then(|| {
    let body_content_top_y = node.body_content_top_y(card_screen_y, rem_size);
    let (visible_types, top_spacer_px, bottom_spacer_px) = node.visible_types(
      body_content_top_y,
      rem_size,
      (rctx.viewport_rect.min_y, rctx.viewport_rect.max_y),
    );

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
        let (bg_color, border_color) = if selected_type == Some(id) {
          (rgba(0x38bdf826), rgba(0x38bdf877))
        } else if selected_edge_from == Some(id) {
          (rgba(0x38bdf820), rgba(0x38bdf866))
        } else if selected_edge_to == Some(id) {
          (rgba(0x34d39920), rgba(0x34d39966))
        } else if connected_types.contains(&id) {
          (rgba(0x7dd3fc12), rgba(0x7dd3fc33))
        } else {
          (rgba(0xffffff04), rgba(0xffffff0d))
        };

        let comments_section = render_type_comments(
          id,
          rctx.comments.comments_for(id),
          rctx.comments.active_draft_for(id),
          cx,
          move |this, ev, window, cx| {
            let mapped = match ev {
              CommentBoxEvent::Save => FileCardEvent::SaveComment,
              CommentBoxEvent::Cancel => FileCardEvent::CancelComment,
            };
            on_event(this, mapped, window, cx);
          },
        );

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
              on_event(this, FileCardEvent::SelectType(id), window, cx);
            }),
          )
          .flex()
          .flex_col()
          .child(
            div()
              .h(rems(TYPE_LINE_HEIGHT_REM))
              .line_height(rems(TYPE_LINE_HEIGHT_REM))
              .text_color(rgb(SKY_TEXT))
              .child(SharedString::from(type_block.header_line.clone())),
          )
          .when_some(type_block.rest_body.as_ref(), |el, rest| {
            el.child(
              div()
                .h(rems(
                  TYPE_LINE_HEIGHT_REM * type_block.rest_line_count as f32,
                ))
                .line_height(rems(TYPE_LINE_HEIGHT_REM))
                .text_color(rgb(0xd1d5db))
                .child(SharedString::from(rest.clone())),
            )
          })
          .when_some(comments_section, |el, section| el.child(section))
      }))
      .when(bottom_spacer_px > 0.0, |el| {
        el.child(div().h(px(bottom_spacer_px)))
      })
  });

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
        .id(SharedString::from(format!("file-header:{crate_idx}:{file_idx}")))
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
            on_event(
              this,
              FileCardEvent::ToggleFile(crate_idx, file_idx),
              window,
              cx,
            );
          }),
        )
        .child(
          div()
            .flex()
            .items_center()
            .child(div().w(rems(0.9)).text_color(rgb(SLATE_400)).child(chevron))
            .when(!node.dir_prefix.is_empty(), |el| {
              el.child(
                div()
                  .text_color(rgb(GRAY_500))
                  .child(SharedString::from(node.dir_prefix.clone())),
              )
            })
            .child(
              div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(0xf3f4f6))
                .child(SharedString::from(node.file_name.clone())),
            ),
        )
        .child(div().text_xs().text_color(rgb(SLATE_500)).child(count_label)),
    )
    .when_some(body, |el, content| el.child(content))
    .into_any_element()
}
