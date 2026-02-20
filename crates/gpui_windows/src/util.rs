use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};

use ::util::ResultExt;
use anyhow::Context;
use windows::{
    UI::{
        Color,
        ViewManagement::{UIColorType, UISettings},
    },
    Win32::{
        Foundation::*,
        Graphics::{
            Dwm::*,
            Gdi::{
                CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC,
                ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
            },
        },
        System::LibraryLoader::LoadLibraryA,
        UI::WindowsAndMessaging::*,
    },
    core::{BOOL, PCSTR},
};

use crate::*;
use gpui::*;

pub(crate) trait HiLoWord {
    fn hiword(&self) -> u16;
    fn loword(&self) -> u16;
    fn signed_hiword(&self) -> i16;
    fn signed_loword(&self) -> i16;
}

impl HiLoWord for WPARAM {
    fn hiword(&self) -> u16 {
        ((self.0 >> 16) & 0xFFFF) as u16
    }

    fn loword(&self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }

    fn signed_hiword(&self) -> i16 {
        ((self.0 >> 16) & 0xFFFF) as i16
    }

    fn signed_loword(&self) -> i16 {
        (self.0 & 0xFFFF) as i16
    }
}

impl HiLoWord for LPARAM {
    fn hiword(&self) -> u16 {
        ((self.0 >> 16) & 0xFFFF) as u16
    }

    fn loword(&self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }

    fn signed_hiword(&self) -> i16 {
        ((self.0 >> 16) & 0xFFFF) as i16
    }

    fn signed_loword(&self) -> i16 {
        (self.0 & 0xFFFF) as i16
    }
}

pub(crate) unsafe fn get_window_long(hwnd: HWND, nindex: WINDOW_LONG_PTR_INDEX) -> isize {
    #[cfg(target_pointer_width = "64")]
    unsafe {
        GetWindowLongPtrW(hwnd, nindex)
    }
    #[cfg(target_pointer_width = "32")]
    unsafe {
        GetWindowLongW(hwnd, nindex) as isize
    }
}

pub(crate) unsafe fn set_window_long(
    hwnd: HWND,
    nindex: WINDOW_LONG_PTR_INDEX,
    dwnewlong: isize,
) -> isize {
    #[cfg(target_pointer_width = "64")]
    unsafe {
        SetWindowLongPtrW(hwnd, nindex, dwnewlong)
    }
    #[cfg(target_pointer_width = "32")]
    unsafe {
        SetWindowLongW(hwnd, nindex, dwnewlong as i32) as isize
    }
}

pub(crate) fn windows_credentials_target_name(url: &str) -> String {
    format!("zed:url={}", url)
}

pub(crate) fn load_cursor(style: CursorStyle) -> Option<HCURSOR> {
    static ARROW: OnceLock<SafeCursor> = OnceLock::new();
    static IBEAM: OnceLock<SafeCursor> = OnceLock::new();
    static CROSS: OnceLock<SafeCursor> = OnceLock::new();
    static HAND: OnceLock<SafeCursor> = OnceLock::new();
    static SIZEWE: OnceLock<SafeCursor> = OnceLock::new();
    static SIZENS: OnceLock<SafeCursor> = OnceLock::new();
    static SIZEALL: OnceLock<SafeCursor> = OnceLock::new();
    static SIZENWSE: OnceLock<SafeCursor> = OnceLock::new();
    static SIZENESW: OnceLock<SafeCursor> = OnceLock::new();
    static NO: OnceLock<SafeCursor> = OnceLock::new();
    let (lock, name) = match style {
        CursorStyle::IBeam | CursorStyle::IBeamCursorForVerticalLayout => (&IBEAM, IDC_IBEAM),
        CursorStyle::Crosshair => (&CROSS, IDC_CROSS),
        CursorStyle::PointingHand | CursorStyle::DragLink => (&HAND, IDC_HAND),
        // Windows에 grab/grabbing 커서가 없어 네방향 화살표로 대체
        CursorStyle::OpenHand | CursorStyle::ClosedHand => (&SIZEALL, IDC_SIZEALL),
        CursorStyle::ResizeLeft
        | CursorStyle::ResizeRight
        | CursorStyle::ResizeLeftRight
        | CursorStyle::ResizeColumn => (&SIZEWE, IDC_SIZEWE),
        CursorStyle::ResizeUp
        | CursorStyle::ResizeDown
        | CursorStyle::ResizeUpDown
        | CursorStyle::ResizeRow => (&SIZENS, IDC_SIZENS),
        CursorStyle::ResizeUpLeftDownRight => (&SIZENWSE, IDC_SIZENWSE),
        CursorStyle::ResizeUpRightDownLeft => (&SIZENESW, IDC_SIZENESW),
        CursorStyle::OperationNotAllowed => (&NO, IDC_NO),
        CursorStyle::None => return None,
        CursorStyle::Custom(id) => return load_custom_cursor(id),
        _ => (&ARROW, IDC_ARROW),
    };
    Some(
        *(*lock.get_or_init(|| {
            HCURSOR(
                unsafe { LoadImageW(None, name, IMAGE_CURSOR, 0, 0, LR_DEFAULTSIZE | LR_SHARED) }
                    .log_err()
                    .unwrap_or_default()
                    .0,
            )
            .into()
        })),
    )
}

/// Cache for custom SVG-based cursors, keyed by their numeric ID.
static CUSTOM_CURSORS: LazyLock<Mutex<HashMap<u16, SafeCursor>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Load a custom cursor from the global registry, rendering its SVG to an HCURSOR.
/// The result is cached so the SVG is only rendered once per cursor ID.
fn load_custom_cursor(id: u16) -> Option<HCURSOR> {
    // Check cache first
    {
        let cache = CUSTOM_CURSORS.lock().unwrap();
        if let Some(cursor) = cache.get(&id) {
            return Some(**cursor);
        }
    }

    let cursor_def = get_custom_cursor(id)?;

    // Query system cursor size (DPI-aware: 32 at 100%, 48 at 150%, etc.)
    let cursor_size = unsafe { GetSystemMetrics(SM_CXCURSOR) } as u32;
    let cursor_size = cursor_size.max(16); // Sanity minimum

    // Parse and render SVG
    let svg_tree = usvg::Tree::from_data(cursor_def.svg_bytes, &usvg::Options::default()).ok()?;
    let svg_size = svg_tree.size();
    let scale = cursor_size as f32 / svg_size.width().max(svg_size.height());

    let width = (svg_size.width() * scale).ceil() as u32;
    let height = (svg_size.height() * scale).ceil() as u32;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&svg_tree, transform, &mut pixmap.as_mut());

    // Convert RGBA premultiplied → BGRA non-premultiplied for Windows
    let mut bgra_data: Vec<u8> = pixmap.take();
    for pixel in bgra_data.chunks_exact_mut(4) {
        let a = pixel[3] as f32;
        if a > 0.0 {
            let inv_a = 255.0 / a;
            let r = (pixel[0] as f32 * inv_a).min(255.0) as u8;
            let g = (pixel[1] as f32 * inv_a).min(255.0) as u8;
            let b = (pixel[2] as f32 * inv_a).min(255.0) as u8;
            // RGBA → BGRA
            pixel[0] = b;
            pixel[1] = g;
            pixel[2] = r;
        } else {
            // Fully transparent pixel: swap R and B anyway
            pixel.swap(0, 2);
        }
    }

    // Calculate hotspot in pixel coordinates
    let hotspot_x = (cursor_def.hotspot_x * width as f32) as u32;
    let hotspot_y = (cursor_def.hotspot_y * height as f32) as u32;

    // Create HCURSOR via Windows API
    let hcursor = unsafe {
        let hdc_screen = GetDC(None);

        // Set up BITMAPINFO for a top-down 32-bit BGRA bitmap
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // Negative = top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let color_bitmap =
            CreateDIBSection(Some(hdc_mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;

        // Copy rendered pixel data into the bitmap
        std::ptr::copy_nonoverlapping(bgra_data.as_ptr(), bits as *mut u8, bgra_data.len());

        // Create a 1-bit mask bitmap (all zeros = fully visible with 32-bit color)
        let mask_bitmap = CreateBitmap(width as i32, height as i32, 1, 1, None);

        let icon_info = ICONINFO {
            fIcon: BOOL::from(false), // false = cursor
            xHotspot: hotspot_x,
            yHotspot: hotspot_y,
            hbmMask: mask_bitmap,
            hbmColor: color_bitmap,
        };

        let hicon = CreateIconIndirect(&icon_info).ok()?;

        // Clean up GDI objects
        let _ = DeleteObject(color_bitmap.into());
        let _ = DeleteObject(mask_bitmap.into());
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        HCURSOR(hicon.0)
    };

    // Cache the cursor
    {
        let mut cache = CUSTOM_CURSORS.lock().unwrap();
        cache.insert(id, hcursor.into());
    }

    Some(hcursor)
}

/// This function is used to configure the dark mode for the window built-in title bar.
pub(crate) fn configure_dwm_dark_mode(hwnd: HWND, appearance: WindowAppearance) {
    let dark_mode_enabled: BOOL = match appearance {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => true.into(),
        WindowAppearance::Light | WindowAppearance::VibrantLight => false.into(),
    };
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode_enabled as *const _ as _,
            std::mem::size_of::<BOOL>() as u32,
        )
        .log_err();
    }
}

#[inline]
pub(crate) fn logical_point(x: f32, y: f32, scale_factor: f32) -> Point<Pixels> {
    Point {
        x: px(x / scale_factor),
        y: px(y / scale_factor),
    }
}

// https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/apply-windows-themes
#[inline]
pub(crate) fn system_appearance() -> Result<WindowAppearance> {
    let ui_settings = UISettings::new()?;
    let foreground_color = ui_settings.GetColorValue(UIColorType::Foreground)?;
    // If the foreground is light, then is_color_light will evaluate to true,
    // meaning Dark mode is enabled.
    if is_color_light(&foreground_color) {
        Ok(WindowAppearance::Dark)
    } else {
        Ok(WindowAppearance::Light)
    }
}

#[inline(always)]
fn is_color_light(color: &Color) -> bool {
    ((5 * color.G as u32) + (2 * color.R as u32) + color.B as u32) > (8 * 128)
}

pub(crate) fn with_dll_library<R, F>(dll_name: PCSTR, f: F) -> Result<R>
where
    F: FnOnce(HMODULE) -> Result<R>,
{
    let library = unsafe {
        LoadLibraryA(dll_name).with_context(|| format!("Loading dll: {}", dll_name.display()))?
    };
    let result = f(library);
    unsafe {
        FreeLibrary(library)
            .with_context(|| format!("Freeing dll: {}", dll_name.display()))
            .log_err();
    }
    result
}
