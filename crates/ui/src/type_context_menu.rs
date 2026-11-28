use gpui::{
  AnyElement, BoxShadow, Context, FontWeight, MouseButton, Pixels, Size, Window, div, point,
  prelude::*, px, rgb, rgba,
};
use graph::{Graph, TypeId};
use layout::WorkspaceLayout;
use layout::metrics::TOP_BAR_HEIGHT;
use state::{Camera, Selection};

use crate::theme::SKY_TEXT;

const MENU_WIDTH_PX: f32 = 196.0;
const MENU_ESTIMATED_HEIGHT_PX: f32 = 84.0;
const MENU_GAP_PX: f32 = 12.0;
const VIEWPORT_MARGIN_PX: f32 = 16.0;

#[derive(Clone, Copy, Debug)]
pub enum TypeContextMenuEvent {
  AddComment(TypeId),
}

pub fn render_type_context_menu<V: 'static>(
  graph: &Graph,
  layout: &WorkspaceLayout,
  camera: Camera,
  selection: &Selection,
  viewport: Size<Pixels>,
  cx: &mut Context<V>,
  on_event: impl Fn(&mut V, TypeContextMenuEvent, &mut Window, &mut Context<V>) + Copy + 'static,
) -> Option<AnyElement> {
  let type_id = selection.selected_type?;
  let ty = graph.ty(type_id)?;
  let anchor = layout.type_right_anchor(type_id, camera.pan, camera.zoom, camera.rem_size())?;

  let viewport_w = f32::from(viewport.width);
  let viewport_h = f32::from(viewport.height);
  let max_x = (viewport_w - MENU_WIDTH_PX - VIEWPORT_MARGIN_PX).max(VIEWPORT_MARGIN_PX);
  let min_y = TOP_BAR_HEIGHT + 8.0;
  let max_y = (viewport_h - MENU_ESTIMATED_HEIGHT_PX - VIEWPORT_MARGIN_PX).max(min_y);

  let menu_x = (anchor.x + MENU_GAP_PX).clamp(VIEWPORT_MARGIN_PX, max_x);
  let menu_y = anchor.y.clamp(min_y, max_y);

  Some(
    div()
      .id("type-context-menu")
      .absolute()
      .left(px(menu_x))
      .top(px(menu_y))
      .w(px(MENU_WIDTH_PX))
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
      }])
      .on_mouse_down(
        MouseButton::Left,
        cx.listener(|_, _, _, cx| cx.stop_propagation()),
      )
      .on_mouse_down(
        MouseButton::Middle,
        cx.listener(|_, _, _, cx| cx.stop_propagation()),
      )
      .child(
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
          .child(format!("{} {}", ty.kind, ty.name)),
      )
      .child(
        div().p(px(5.0)).flex().flex_col().gap(px(2.0)).child(
          div()
            .id("type-context-add-comment")
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
                on_event(this, TypeContextMenuEvent::AddComment(type_id), window, cx);
              }),
            )
            .child(
              div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(SKY_TEXT))
                .child("+"),
            )
            .child("Add a comment"),
        ),
      )
      .into_any_element(),
  )
}
