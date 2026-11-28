use gpui::{AnyElement, div, prelude::*, px, rgb, rgba};

use crate::app::LspStatus;

pub fn render_lsp_status(status: &LspStatus) -> Option<AnyElement> {
  let (dot_color, label) = match status {
    LspStatus::Initialising => (rgba(0x38bdf8ff), "Initialising LSP...".to_owned()),
    LspStatus::Failed(err) => (rgba(0xf87171ff), format!("LSP failed: {err}")),
    LspStatus::Ready => return None,
  };

  Some(
    div()
      .absolute()
      .left(px(16.0))
      .bottom(px(16.0))
      .px(px(12.0))
      .py(px(6.0))
      .rounded(px(8.0))
      .bg(rgba(0x14161ee0))
      .border_1()
      .border_color(rgba(0xffffff20))
      .flex()
      .items_center()
      .gap(px(8.0))
      .text_size(px(12.0))
      .text_color(rgb(0xd8dde8))
      .child(div().w(px(7.0)).h(px(7.0)).rounded_full().bg(dot_color))
      .child(label)
      .into_any_element(),
  )
}

pub fn render_zoom_badge(zoom: f32) -> impl IntoElement {
  let label = format!("{}%", (zoom * 100.0).round() as i32);

  div()
    .absolute()
    .right(px(16.0))
    .bottom(px(16.0))
    .px(px(10.0))
    .py(px(5.0))
    .rounded(px(8.0))
    .bg(rgba(0x14161ee0))
    .border_1()
    .border_color(rgba(0xffffff20))
    .text_size(px(12.0))
    .text_color(rgb(0xc8cdd8))
    .child(label)
}
