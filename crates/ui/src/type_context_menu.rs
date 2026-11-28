use gpui::{AnyElement, Context, MouseButton, Pixels, Size, Window, div, prelude::*, px};
use graph::{Graph, TypeId};
use layout::WorkspaceLayout;
use layout::metrics::TOP_BAR_HEIGHT;
use state::{Camera, Selection};

use crate::primitives::{SurfaceVariant, menu_item, popover_header, surface};

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
    surface(SurfaceVariant::Popover)
      .id("type-context-menu")
      .absolute()
      .left(px(menu_x))
      .top(px(menu_y))
      .w(px(MENU_WIDTH_PX))
      .on_mouse_down(
        MouseButton::Left,
        cx.listener(|_, _, _, cx| cx.stop_propagation()),
      )
      .on_mouse_down(
        MouseButton::Middle,
        cx.listener(|_, _, _, cx| cx.stop_propagation()),
      )
      .child(popover_header(format!("{} {}", ty.kind, ty.name)))
      .child(
        div().p(px(5.0)).flex().flex_col().gap(px(2.0)).child(menu_item(
          "type-context-add-comment",
          Some("+"),
          "Add a comment",
          cx,
          move |this, window, cx| {
            on_event(this, TypeContextMenuEvent::AddComment(type_id), window, cx);
          },
        )),
      )
      .into_any_element(),
  )
}
