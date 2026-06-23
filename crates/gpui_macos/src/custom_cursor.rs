//! macOS 커스텀 이미지 커서 — RGBA → PNG → NSImage → NSCursor.
//!
//! gpui core 의 [`gpui::CustomCursorImage`] (RGBA8) 를 받아 NSCursor 를 만들어 전역 맵에 보관하고,
//! 커서 설정 시 id 로 조회한다. 커서 생성/사용은 모두 메인 스레드(AppKit)에서 일어난다.
//!
//! ⚠️ 이 파일은 `cfg(target_os = "macos")` 로만 컴파일되어 Windows 개발 환경에선 검증 불가.
//! macOS 포팅 시 컴파일/동작 확인 필요.

use std::collections::HashMap;
use std::ffi::c_void;
use std::io::Cursor;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

use cocoa::base::{id, nil};
use cocoa::foundation::{NSData, NSPoint, NSUInteger};
use gpui::CustomCursorImage;
use image::{DynamicImage, ImageFormat, RgbaImage};
use objc::{class, msg_send, sel, sel_impl};

/// NSCursor 포인터를 전역 맵에 보관하기 위한 래퍼.
/// 커서 생성·조회·사용이 모두 메인 스레드라 Send 가 안전하다.
struct SendCursor(id);
unsafe impl Send for SendCursor {}

static REGISTRY: LazyLock<Mutex<HashMap<u32, SendCursor>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// RGBA 이미지로 NSCursor 를 만들어 등록하고 id 를 반환한다. 실패 시 `None`.
/// 메인 스레드에서 호출해야 한다 (앱 시작 시 1회 등록 용도).
pub(crate) fn register(image: &CustomCursorImage) -> Option<u32> {
    let cursor = unsafe { make_cursor(image)? };
    let id_num = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    REGISTRY.lock().unwrap().insert(id_num, SendCursor(cursor));
    Some(id_num)
}

/// 등록된 NSCursor 를 id 로 조회한다.
pub(crate) fn get(id_num: u32) -> Option<id> {
    REGISTRY.lock().unwrap().get(&id_num).map(|c| c.0)
}

/// RGBA8 → PNG → NSImage → NSCursor. NSData/NSImage 경유라 픽셀 포맷을 직접 다루지 않아 견고.
unsafe fn make_cursor(image: &CustomCursorImage) -> Option<id> {
    let rgba = RgbaImage::from_raw(image.width, image.height, image.rgba.clone())?;
    let mut png: Vec<u8> = Vec::new();
    DynamicImage::ImageRgba8(rgba)
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .ok()?;

    let data: id = NSData::dataWithBytes_length_(
        nil,
        png.as_ptr() as *const c_void,
        png.len() as NSUInteger,
    );
    if data == nil {
        return None;
    }
    let ns_image: id = msg_send![class!(NSImage), alloc];
    let ns_image: id = msg_send![ns_image, initWithData: data];
    if ns_image == nil {
        return None;
    }
    // NSCursor 핫스팟은 이미지 좌상단 기준 포인트. hot_x/hot_y 는 top-down 픽셀이라 그대로 사용.
    let hotspot = NSPoint::new(image.hot_x as f64, image.hot_y as f64);
    let cursor: id = msg_send![class!(NSCursor), alloc];
    let cursor: id = msg_send![cursor, initWithImage: ns_image hotSpot: hotspot];
    if cursor == nil {
        return None;
    }
    Some(cursor)
}
