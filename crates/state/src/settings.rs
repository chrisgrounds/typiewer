#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewerSettings {
  pub show_dot_grid: bool,
  pub show_edges: bool,
  pub show_zoom_badge: bool,
}

impl Default for ViewerSettings {
  fn default() -> Self {
    Self {
      show_dot_grid: true,
      show_edges: true,
      show_zoom_badge: true,
    }
  }
}
