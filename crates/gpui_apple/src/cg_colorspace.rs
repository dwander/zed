//! Core Graphics colour-space handles for the Metal layer.
//!
//! Kept out of `metal_renderer.rs` on purpose: the build script runs cbindgen over that file to
//! generate the shader header, and a raw `extern "C"` block there ends up in the Metal source.

use core_foundation::string::CFStringRef;
use std::ffi::c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateWithName(name: CFStringRef) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    static kCGColorSpaceExtendedSRGB: CFStringRef;
}

/// A new `CGColorSpaceRef` for extended sRGB (values outside 0..1 allowed, sRGB transfer curve).
/// The caller owns it; pass it to [`release`] once the layer has retained it. Null on failure.
pub(crate) fn extended_srgb() -> *mut c_void {
    // Safety: plain Core Graphics calls with a constant name; the result follows the Create rule.
    unsafe { CGColorSpaceCreateWithName(kCGColorSpaceExtendedSRGB) }
}

/// Release a colour space obtained from [`extended_srgb`].
///
/// # Safety
/// `space` must come from [`extended_srgb`] and not be used afterwards.
pub(crate) unsafe fn release(space: *mut c_void) {
    unsafe { CGColorSpaceRelease(space) }
}
