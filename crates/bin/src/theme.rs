// Camera & zoom constants.
pub const BASE_REM_SIZE: f32 = 16.0;
pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 3.0;
pub const DEFAULT_PAN_X: f32 = 300.0;
pub const DEFAULT_PAN_Y: f32 = 56.0;

// Layout constants in `rems` (shared between `layout` and `ui::file_card` so edge anchors align).
pub const CRATE_HEADER_PAD_Y_REM: f32 = 0.625;
pub const CRATE_HEADER_LINE_HEIGHT_REM: f32 = 1.5;
pub const CRATE_BODY_PAD_X_REM: f32 = 1.0;
pub const CRATE_BODY_PAD_Y_REM: f32 = 0.875;
pub const FILE_GAP_REM: f32 = 1.5;
pub const CRATE_GAP_Y: f32 = 40.0;

pub const HEADER_PAD_Y_REM: f32 = 0.5;
pub const HEADER_LINE_HEIGHT_REM: f32 = 1.5;
pub const BODY_PAD_Y_REM: f32 = 0.75;
pub const TYPE_BLOCK_PAD_Y_REM: f32 = 0.5;
pub const TYPE_LINE_HEIGHT_REM: f32 = 1.35;
pub const TYPE_GAP_REM: f32 = 0.625;

// Viewport culling overscan margins (in screen pixels).
pub const VIEWPORT_OVERSCAN_X_PX: f32 = 64.0;
pub const VIEWPORT_OVERSCAN_Y_PX: f32 = 64.0;
pub const EDGE_CULL_PADDING_PX: f32 = 24.0;

// Approximate Menlo monospace advance width in rems at 0.82rem body / 0.875rem header,
// used to stabilize card widths during intra-card type culling and for horizontal culling.
pub const BODY_CHAR_WIDTH_REM: f32 = 0.6001 * 0.82;
pub const HEADER_CHAR_WIDTH_REM: f32 = 0.54;
pub const CARD_BODY_HORIZONTAL_PAD_REM: f32 = 3.75;
pub const CARD_HEADER_HORIZONTAL_PAD_REM: f32 = 2.25;
pub const CARD_WIDTH_CULL_SAFETY_FACTOR: f32 = 1.15;

pub const EDGE_HIT_SEGMENTS: usize = 32;

pub const MONO_FONT: &str = "Menlo";
