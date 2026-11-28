pub mod edges;
pub mod file_layout;
pub mod geometry;
pub mod metrics;

pub use edges::{hit_test_edge, iter_edge_curves};
pub use file_layout::{FileNode, TypeBlock, TypeLayout, build_file_nodes, visible_type_slice};
pub use geometry::{
  EdgeCurve, Point2, ViewportRect, eval_cubic_bezier, min_dist_sq_to_cubic_bezier, point2,
  point_to_segment_dist_sq,
};
