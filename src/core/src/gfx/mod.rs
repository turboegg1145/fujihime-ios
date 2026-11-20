//! 2D graphics subsystem (`CMath` LUTs, `Surface`, decoders, software compositor, transitions).

pub mod cmath;
pub mod compositor;
pub mod decode;
pub mod surface;
pub mod transition;

pub use compositor::{
    blit, blit_porter_duff_over, clip_rects, filter_darken_pct, filter_embos_tint,
    filter_fill_argb, filter_fill_rgb_keep_alpha, filter_grayscale, filter_invert_rgb,
    stretch_blit_srccopy, BlitParams, Canvas800x600, ClippedRegion, Rect,
};
pub use surface::{Surface, GAME_HEIGHT, GAME_WIDTH};
pub use transition::{focal_grid_point, zoom_crop_rect, TransitionKind, TransitionRunner};
