use std::collections::BTreeMap;

use gpui::{
  App, Bounds, BoxShadow, Context, FocusHandle, FontWeight, KeyBinding, MouseButton,
  MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollEvent, TitlebarOptions,
  Window, WindowBounds, WindowOptions, actions, canvas, div, fill, linear_color_stop,
  linear_gradient, point, prelude::*, px, rgb, rgba, size,
};
use gpui_platform::application;

use crate::{Item, VecItems};

actions!(viewer, [ZoomIn, ZoomOut, ResetZoom]);

const BASE_REM_SIZE: f32 = 16.0;
const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 3.0;
const DEFAULT_PAN_X: f32 = 40.0;
const DEFAULT_PAN_Y: f32 = 56.0;

struct FileNode {
  filename: String,
  items: Vec<Item>,
  position: Point<f32>,
}

pub struct Application {
  nodes: Vec<FileNode>,
  pan: Point<f32>,
  zoom: f32,
  drag_last_pos: Option<Point<f32>>,
  focus_handle: FocusHandle,
}

impl Application {
  fn new(items: VecItems, cx: &mut Context<Self>) -> Self {
    let mut grouped: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    for item in items {
      grouped
        .entry(item.filename().to_owned())
        .or_default()
        .push(item);
    }

    let mut nodes = Vec::with_capacity(grouped.len());
    let mut cursor_y: f32 = 0.0;
    let gap_y: f32 = 28.0;

    for (filename, file_items) in grouped {
      let estimated_height = 56.0 + (file_items.len() as f32 * 24.0) + 24.0;
      nodes.push(FileNode {
        filename,
        items: file_items,
        position: point(0.0, cursor_y),
      });
      cursor_y += estimated_height + gap_y;
    }

    Self {
      nodes,
      pan: point(DEFAULT_PAN_X, DEFAULT_PAN_Y),
      zoom: 1.0,
      drag_last_pos: None,
      focus_handle: cx.focus_handle(),
    }
  }

  fn set_zoom_around(
    &mut self,
    new_zoom: f32,
    focal: Point<f32>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let clamped = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    if (clamped - self.zoom).abs() < f32::EPSILON {
      return;
    }

    let world_x = (focal.x - self.pan.x) / self.zoom;
    let world_y = (focal.y - self.pan.y) / self.zoom;

    self.zoom = clamped;
    self.pan.x = focal.x - world_x * self.zoom;
    self.pan.y = focal.y - world_y * self.zoom;

    window.set_rem_size(px(BASE_REM_SIZE * self.zoom));
    cx.notify();
  }

  fn zoom_in(&mut self, _: &ZoomIn, window: &mut Window, cx: &mut Context<Self>) {
    let viewport = window.viewport_size();
    let center = point(
      f32::from(viewport.width) * 0.5,
      f32::from(viewport.height) * 0.5,
    );
    self.set_zoom_around(self.zoom * 1.15, center, window, cx);
  }

  fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
    let viewport = window.viewport_size();
    let center = point(
      f32::from(viewport.width) * 0.5,
      f32::from(viewport.height) * 0.5,
    );
    self.set_zoom_around(self.zoom / 1.15, center, window, cx);
  }

  fn reset_zoom(&mut self, _: &ResetZoom, window: &mut Window, cx: &mut Context<Self>) {
    self.zoom = 1.0;
    self.pan = point(DEFAULT_PAN_X, DEFAULT_PAN_Y);
    window.set_rem_size(px(BASE_REM_SIZE));
    cx.notify();
  }

  fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
    window.focus(&self.focus_handle, cx);
    if event.button == MouseButton::Left || event.button == MouseButton::Middle {
      self.drag_last_pos = Some(point(
        f32::from(event.position.x),
        f32::from(event.position.y),
      ));
      cx.notify();
    }
  }

  fn on_mouse_move(
    &mut self,
    event: &MouseMoveEvent,
    _window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if let Some(last) = self.drag_last_pos {
      let current = point(f32::from(event.position.x), f32::from(event.position.y));
      self.pan.x += current.x - last.x;
      self.pan.y += current.y - last.y;
      self.drag_last_pos = Some(current);
      cx.notify();
    }
  }

  fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
    if self.drag_last_pos.take().is_some() {
      cx.notify();
    }
  }

  fn on_scroll(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
    let delta: Point<Pixels> = event.delta.pixel_delta(px(20.0));
    let dx = f32::from(delta.x);
    let dy = f32::from(delta.y);

    if event.modifiers.platform || event.modifiers.control {
      let factor = (1.0 + dy * 0.004).clamp(0.75, 1.25);
      let cursor = point(f32::from(event.position.x), f32::from(event.position.y));
      self.set_zoom_around(self.zoom * factor, cursor, window, cx);
    } else {
      self.pan.x += dx;
      self.pan.y += dy;
      cx.notify();
    }
  }
}

impl Render for Application {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let pan = self.pan;
    let zoom = self.zoom;
    let is_dragging = self.drag_last_pos.is_some();

    let dot_grid = canvas(
      |_bounds, _window, _cx| {},
      move |bounds, _, window, _cx| {
        let mut step = 32.0 * zoom;
        while step < 14.0 {
          step *= 2.0;
        }
        let dot_size = (2.0 * zoom).clamp(1.0, 3.0);

        let origin_x = f32::from(bounds.origin.x);
        let origin_y = f32::from(bounds.origin.y);
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);

        let start_x = origin_x + (pan.x - origin_x).rem_euclid(step);
        let start_y = origin_y + (pan.y - origin_y).rem_euclid(step);

        let dot_color = rgba(0xffffff14);
        let mut y = start_y;
        while y < origin_y + height {
          let mut x = start_x;
          while x < origin_x + width {
            window.paint_quad(fill(
              Bounds::new(point(px(x), px(y)), size(px(dot_size), px(dot_size))),
              dot_color,
            ));
            x += step;
          }
          y += step;
        }
      },
    )
    .absolute()
    .inset_0()
    .size_full();

    let hud_zoom_label = format!("{}%", (zoom * 100.0).round() as i32);

    div()
      .id("canvas-root")
      .track_focus(&self.focus_handle)
      .on_action(cx.listener(Self::zoom_in))
      .on_action(cx.listener(Self::zoom_out))
      .on_action(cx.listener(Self::reset_zoom))
      .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
      .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
      .on_mouse_move(cx.listener(Self::on_mouse_move))
      .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
      .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
      .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
      .on_scroll(cx.listener(Self::on_scroll))
      .relative()
      .size_full()
      .overflow_hidden()
      .when(is_dragging, |el| el.cursor_grabbing())
      .when(!is_dragging, |el| el.cursor_grab())
      .bg(linear_gradient(
        180.,
        linear_color_stop(rgb(0x0d0e14), 0.0),
        linear_color_stop(rgb(0x040406), 1.0),
      ))
      .text_color(rgb(0xf4f6fb))
      .child(dot_grid)
      .children(self.nodes.iter().map(|node| {
        let screen_x = node.position.x * zoom + pan.x;
        let screen_y = node.position.y * zoom + pan.y;

        div()
          .absolute()
          .left(px(screen_x))
          .top(px(screen_y))
          .flex()
          .flex_col()
          .overflow_hidden()
          .rounded_xl()
          .bg(linear_gradient(
            180.,
            linear_color_stop(rgb(0x181a22), 0.0),
            linear_color_stop(rgb(0x0a0b0f), 1.0),
          ))
          .border_1()
          .border_color(rgba(0xffffff24))
          .shadow(vec![
            BoxShadow {
              inset: false,
              color: rgba(0x000000b0).into(),
              offset: point(px(0.), px(10. * zoom)),
              blur_radius: px(24. * zoom),
              spread_radius: px(-2. * zoom),
            },
            BoxShadow {
              inset: true,
              color: rgba(0xffffff20).into(),
              offset: point(px(0.), px(1.)),
              blur_radius: px(0.),
              spread_radius: px(0.),
            },
          ])
          .child(
            div()
              .px_4()
              .py_2()
              .bg(linear_gradient(
                180.,
                linear_color_stop(rgba(0xffffff1a), 0.0),
                linear_color_stop(rgba(0xffffff05), 1.0),
              ))
              .border_b_1()
              .border_color(rgba(0xffffff16))
              .font_weight(FontWeight::SEMIBOLD)
              .text_color(rgb(0xf5f7fc))
              .child(node.filename.clone()),
          )
          .child(
            div()
              .px_4()
              .py_3()
              .flex()
              .flex_col()
              .gap_1()
              .children(node.items.iter().map(|item| {
                let color = match item {
                  Item::Type { .. } => rgb(0x7dd3fc),
                  Item::Function { .. } => rgb(0xd4d8e2),
                };
                div().text_color(color).child(item.label())
              })),
          )
      }))
      .child(
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
          .child(hud_zoom_label),
      )
  }
}

pub fn run(items: VecItems) {
  application().run(|cx: &mut App| {
    cx.bind_keys([
      KeyBinding::new("cmd-=", ZoomIn, None),
      KeyBinding::new("cmd-+", ZoomIn, None),
      KeyBinding::new("cmd--", ZoomOut, None),
      KeyBinding::new("cmd-0", ResetZoom, None),
    ]);

    cx.on_window_closed(|cx, _window_id| {
      if cx.windows().is_empty() {
        cx.quit();
      }
    })
    .detach();

    let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
    if let Err(err) = cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
          appears_transparent: true,
          ..Default::default()
        }),
        ..Default::default()
      },
      |window, cx| {
        let view = cx.new(|cx| Application::new(items, cx));
        let focus = view.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
        view
      },
    ) {
      eprintln!("Failed to open window: {err}");
    }

    cx.activate(true);
  });
}
