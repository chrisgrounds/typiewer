use gpui::{Pixels, Point};
use layout::{Point2, point2};

pub const MONO_FONT: &str = "Menlo";

pub const SKY_TEXT: u32 = 0x7dd3fc;
pub const EMERALD_TEXT: u32 = 0x6ee7b7;
pub const AMBER_TEXT: u32 = 0xfde68a;
pub const SLATE_400: u32 = 0x94a3b8;
pub const SLATE_500: u32 = 0x64748b;
pub const GRAY_500: u32 = 0x6b7280;

pub fn to_point2(p: Point<Pixels>) -> Point2 {
  point2(f32::from(p.x), f32::from(p.y))
}

pub fn pluralize(count: usize, singular: &str, plural: &str) -> String {
  if count == 1 {
    format!("1 {singular}")
  } else {
    format!("{count} {plural}")
  }
}
