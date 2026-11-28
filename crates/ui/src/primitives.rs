use gpui::{
  BoxShadow, Context, Div, ElementId, FontWeight, MouseButton, SharedString, Stateful, Window, div,
  point, prelude::*, px, rems, rgb, rgba,
};

use crate::theme::{GRAY_500, SKY_TEXT, SLATE_400};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceVariant {
  Popover,
  Inset,
  Note,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonVariant {
  Primary,
  Ghost,
}

pub fn surface(variant: SurfaceVariant) -> Div {
  match variant {
    SurfaceVariant::Popover => div()
      .flex()
      .flex_col()
      .overflow_hidden()
      .rounded(px(10.0))
      .bg(rgba(0x11131af2))
      .border_1()
      .border_color(rgba(0x38bdf844))
      .shadow(vec![BoxShadow {
        inset: false,
        color: rgba(0x000000b0).into(),
        offset: point(px(0.), px(12.)),
        blur_radius: px(28.),
        spread_radius: px(-4.),
      }]),
    SurfaceVariant::Inset => div()
      .flex()
      .flex_col()
      .overflow_hidden()
      .rounded_md()
      .bg(rgba(0x0a0c12d8))
      .border_1()
      .border_color(rgba(0x38bdf844)),
    SurfaceVariant::Note => div()
      .flex()
      .flex_col()
      .rounded_md()
      .bg(rgba(0x38bdf810))
      .border_1()
      .border_color(rgba(0x38bdf828)),
  }
}

pub fn popover_header(label: impl Into<SharedString>) -> Div {
  div()
    .px(px(12.0))
    .py(px(7.0))
    .border_b_1()
    .border_color(rgba(0xffffff10))
    .bg(rgba(0x38bdf80d))
    .text_size(px(11.0))
    .font_weight(FontWeight::MEDIUM)
    .text_color(rgb(SKY_TEXT))
    .whitespace_nowrap()
    .overflow_hidden()
    .child(label.into())
}

pub fn menu_item<V: 'static>(
  id: impl Into<ElementId>,
  icon: Option<&'static str>,
  label: impl Into<SharedString>,
  cx: &mut Context<V>,
  on_click: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> Stateful<Div> {
  div()
    .id(id)
    .cursor_pointer()
    .px(px(9.0))
    .py(px(7.0))
    .rounded(px(6.0))
    .flex()
    .items_center()
    .gap(px(8.0))
    .text_size(px(12.5))
    .text_color(rgb(0xe2e8f0))
    .hover(|el| el.bg(rgba(0x38bdf81c)).text_color(rgb(0xf8fafc)))
    .on_mouse_down(
      MouseButton::Left,
      cx.listener(move |this, _, window, cx| {
        cx.stop_propagation();
        on_click(this, window, cx);
      }),
    )
    .when_some(icon, |el, glyph| {
      el.child(
        div()
          .text_size(px(13.0))
          .font_weight(FontWeight::SEMIBOLD)
          .text_color(rgb(SKY_TEXT))
          .child(glyph),
      )
    })
    .child(label.into())
}

pub fn button<V: 'static>(
  id: impl Into<ElementId>,
  label: impl Into<SharedString>,
  variant: ButtonVariant,
  cx: &mut Context<V>,
  on_click: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> Stateful<Div> {
  let (bg, border, text_color, hover_bg, hover_text) = match variant {
    ButtonVariant::Primary => (
      rgba(0x38bdf826),
      rgba(0x38bdf855),
      rgb(SKY_TEXT),
      rgba(0x38bdf83d),
      rgb(0xe0f2fe),
    ),
    ButtonVariant::Ghost => (
      rgba(0xffffff06),
      rgba(0xffffff14),
      rgb(SLATE_400),
      rgba(0xffffff14),
      rgb(0xe2e8f0),
    ),
  };

  div()
    .id(id)
    .cursor_pointer()
    .px(rems(0.55))
    .py(rems(0.2))
    .rounded(rems(0.3))
    .bg(bg)
    .border_1()
    .border_color(border)
    .text_size(rems(0.72))
    .line_height(rems(1.1))
    .font_weight(FontWeight::MEDIUM)
    .text_color(text_color)
    .hover(move |el| el.bg(hover_bg).text_color(hover_text))
    .on_mouse_down(
      MouseButton::Left,
      cx.listener(move |this, _, window, cx| {
        cx.stop_propagation();
        on_click(this, window, cx);
      }),
    )
    .child(label.into())
}

pub fn text_input(
  id: impl Into<ElementId>,
  value: &str,
  placeholder: &'static str,
  focused: bool,
) -> Stateful<Div> {
  let border = if focused {
    rgba(0x38bdf877)
  } else {
    rgba(0xffffff18)
  };

  div()
    .id(id)
    .w_full()
    .px(rems(0.5))
    .py(rems(0.3))
    .rounded(rems(0.3))
    .bg(rgba(0x05070ce6))
    .border_1()
    .border_color(border)
    .flex()
    .items_center()
    .overflow_hidden()
    .text_size(rems(0.78))
    .line_height(rems(1.2))
    .child(if value.is_empty() {
      div()
        .flex()
        .items_center()
        .when(focused, |el| {
          el.child(div().w(px(1.5)).h(rems(0.95)).mr(px(2.0)).bg(rgb(SKY_TEXT)))
        })
        .child(div().text_color(rgb(GRAY_500)).child(placeholder))
    } else {
      div()
        .flex()
        .items_center()
        .text_color(rgb(0xf1f5f9))
        .child(SharedString::from(value.to_owned()))
        .when(focused, |el| {
          el.child(div().w(px(1.5)).h(rems(0.95)).ml(px(1.5)).bg(rgb(SKY_TEXT)))
        })
    })
}
