use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Definition of a custom cursor backed by SVG data.
#[derive(Clone)]
pub struct CustomCursorDef {
    /// Raw SVG bytes for the cursor image.
    pub svg_bytes: &'static [u8],
    /// Hotspot X position as a fraction (0.0–1.0) of the cursor width.
    pub hotspot_x: f32,
    /// Hotspot Y position as a fraction (0.0–1.0) of the cursor height.
    pub hotspot_y: f32,
}

static CUSTOM_CURSOR_REGISTRY: LazyLock<RwLock<HashMap<u16, CustomCursorDef>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Register a custom cursor with the given numeric ID.
///
/// Must be called before the cursor is used (typically at app startup).
/// SVG bytes should be embedded with `include_bytes!()` for static lifetime.
///
/// `hotspot_x` and `hotspot_y` are fractions (0.0–1.0) specifying the click
/// point relative to the cursor image size.
pub fn register_custom_cursor(
    id: u16,
    svg_bytes: &'static [u8],
    hotspot_x: f32,
    hotspot_y: f32,
) {
    let mut registry = CUSTOM_CURSOR_REGISTRY.write().unwrap();
    registry.insert(
        id,
        CustomCursorDef {
            svg_bytes,
            hotspot_x,
            hotspot_y,
        },
    );
}

/// Retrieve a custom cursor definition by its numeric ID.
pub fn get_custom_cursor(id: u16) -> Option<CustomCursorDef> {
    let registry = CUSTOM_CURSOR_REGISTRY.read().unwrap();
    registry.get(&id).cloned()
}
