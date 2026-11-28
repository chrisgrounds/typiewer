use gpui::{Context, MouseButton, Window, WindowControlArea, div, prelude::*, px, rgba};
use layout::metrics::TOP_BAR_HEIGHT;

#[derive(Clone, Copy, Debug)]
pub enum TopBarEvent {
  MouseDown,
  MouseUp,
  MouseMove,
}

pub fn render_top_bar<V: 'static>(
  cx: &mut Context<V>,
  on_event: fn(&mut V, TopBarEvent, &mut Window, &mut Context<V>),
) -> impl IntoElement + use<V> {
  div()
    .id("top-bar")
    .window_control_area(WindowControlArea::Drag)
    .absolute()
    .top_0()
    .left_0()
    .right_0()
    .h(px(TOP_BAR_HEIGHT))
    .bg(rgba(0x0d0e14e6))
    .border_b_1()
    .border_color(rgba(0xffffff18))
    .cursor_default()
    .on_mouse_down(
      MouseButton::Left,
      cx.listener(move |this, _event, window, cx| {
        on_event(this, TopBarEvent::MouseDown, window, cx);
      }),
    )
    .on_mouse_down(
      MouseButton::Middle,
      cx.listener(|_this, _event, _window, cx| {
        cx.stop_propagation();
      }),
    )
    .on_mouse_up(
      MouseButton::Left,
      cx.listener(move |this, _event, window, cx| {
        on_event(this, TopBarEvent::MouseUp, window, cx);
      }),
    )
    .on_mouse_down_out(cx.listener(move |this, _event, window, cx| {
      on_event(this, TopBarEvent::MouseUp, window, cx);
    }))
    .on_mouse_move(cx.listener(move |this, _event, window, cx| {
      on_event(this, TopBarEvent::MouseMove, window, cx);
    }))
    .on_scroll(cx.listener(|_this, _event, _window, cx| {
      cx.stop_propagation();
    }))
    .on_click(|event, window, _cx| {
      if event.click_count() == 2 {
        #[cfg(target_os = "macos")]
        window.titlebar_double_click();
        #[cfg(not(target_os = "macos"))]
        window.zoom_window();
      }
    })
}
