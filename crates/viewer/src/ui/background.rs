use gpui::{Bounds, canvas, fill, point, prelude::*, px, rgba, size};

use crate::camera::Camera;

pub fn render_dot_grid(camera: Camera) -> impl IntoElement {
  let pan = camera.pan;
  let zoom = camera.zoom;

  canvas(
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
  .size_full()
}
