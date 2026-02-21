/// Re-exports engine utilities that are frontend-agnostic.
pub use rustale_engine::util::*;

/// GUI-specific icon constants (SVG strings).
pub mod icons;

/// GUI-specific image downloading and caching utilities.
pub mod image_cache;

/// Converts an SVG string (from the icon constants) into an
/// `iced::widget::svg::Handle` suitable for rendering.
///
/// This is the bridge between the renderer-agnostic `&'static str`
/// SVG data and Iced's typed Handle.
///
/// # Example
/// ```rust
/// use crate::util;
/// use iced::widget::svg;
///
/// let handle = util::svg_handle(util::icons::PLAY);
/// let widget = svg(handle).width(20).height(20);
/// ```
pub fn svg_handle(svg_str: &'static str) -> iced::widget::svg::Handle {
    iced::widget::svg::Handle::from_memory(svg_str.as_bytes().to_vec())
}
