use gpui::{
  AnyElement, Context, MouseButton, SharedString, Window, div, prelude::*, rems, rgb, rgba,
};
use graph::TypeId;
use layout::metrics::{
  COMMENT_COMPOSER_HEIGHT_REM, COMMENT_GAP_REM, COMMENT_ITEM_PAD_Y_REM, COMMENT_LINE_HEIGHT_REM,
  COMMENT_SECTION_GAP_REM,
};

use crate::primitives::{ButtonVariant, SurfaceVariant, button, surface, text_input};
use crate::theme::SLATE_500;

#[derive(Clone, Copy, Debug)]
pub enum CommentBoxEvent {
  Save,
  Cancel,
}

pub fn render_type_comments<V: 'static>(
  type_id: TypeId,
  comments: &[String],
  active_draft: Option<&str>,
  cx: &mut Context<V>,
  on_event: impl Fn(&mut V, CommentBoxEvent, &mut Window, &mut Context<V>) + Copy + 'static,
) -> Option<AnyElement> {
  if comments.is_empty() && active_draft.is_none() {
    return None;
  }

  let saved_items = comments.iter().map(|comment| {
    let line_count = comment.lines().count().max(1) as f32;
    surface(SurfaceVariant::Note)
      .px(rems(0.6))
      .py(rems(COMMENT_ITEM_PAD_Y_REM))
      .child(
        div()
          .h(rems(COMMENT_LINE_HEIGHT_REM * line_count))
          .line_height(rems(COMMENT_LINE_HEIGHT_REM))
          .text_size(rems(0.78))
          .text_color(rgb(0xe2e8f0))
          .child(SharedString::from(comment.clone())),
      )
  });

  let composer = active_draft.map(|draft_text| {
    surface(SurfaceVariant::Inset)
      .h(rems(COMMENT_COMPOSER_HEIGHT_REM))
      .px(rems(0.55))
      .py(rems(0.4))
      .justify_between()
      .child(text_input(
        ("comment-input", type_id.0),
        draft_text,
        "Add a comment...",
        true,
      ))
      .child(
        div()
          .flex()
          .items_center()
          .justify_between()
          .gap(rems(0.5))
          .child(
            div()
              .text_size(rems(0.66))
              .text_color(rgb(SLATE_500))
              .child("Enter to save · Esc to cancel"),
          )
          .child(
            div()
              .flex()
              .items_center()
              .gap(rems(0.35))
              .child(button(
                ("comment-cancel", type_id.0),
                "Cancel",
                ButtonVariant::Ghost,
                cx,
                move |this, window, cx| on_event(this, CommentBoxEvent::Cancel, window, cx),
              ))
              .child(button(
                ("comment-save", type_id.0),
                "Save",
                ButtonVariant::Primary,
                cx,
                move |this, window, cx| on_event(this, CommentBoxEvent::Save, window, cx),
              )),
          ),
      )
  });

  Some(
    div()
      .id(("type-comments", type_id.0))
      .mt(rems(COMMENT_SECTION_GAP_REM))
      .pt(rems(COMMENT_SECTION_GAP_REM))
      .border_t_1()
      .border_color(rgba(0xffffff12))
      .flex()
      .flex_col()
      .gap(rems(COMMENT_GAP_REM))
      .on_mouse_down(
        MouseButton::Left,
        cx.listener(|_, _, _, cx| cx.stop_propagation()),
      )
      .children(saved_items)
      .when_some(composer, |el, box_el| el.child(box_el))
      .into_any_element(),
  )
}
