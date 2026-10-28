//! What the display under a window offers for HDR output: whether Windows HDR is on, how bright
//! SDR white is and the panel's peak. [`DirectXRenderer::set_hdr_content`] reads it to choose
//! between the 8-bit sRGB swap chain and the FP16 scRGB one.
//!
//! [`DirectXRenderer::set_hdr_content`]: crate::DirectXRenderer::set_hdr_content

use windows::{
    Win32::{
        Devices::Display::{
            DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
            DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
            DISPLAYCONFIG_SDR_WHITE_LEVEL, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
            DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
            QueryDisplayConfig,
        },
        Foundation::{ERROR_SUCCESS, HWND, LUID},
        Graphics::{
            Dxgi::{
                Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_OUTPUT_DESC1,
                IDXGIFactory1, IDXGIOutput6,
            },
            Gdi::{HMONITOR, MONITOR_DEFAULTTONEAREST, MonitorFromWindow},
        },
    },
    core::Interface,
};

/// Luminance of scRGB `1.0`. DXGI's linear FP16 colour space pins SDR white here; Windows'
/// "SDR content brightness" slider then lifts the white every other window gets.
pub(crate) const SCRGB_WHITE_NITS: f32 = 80.0;

/// `DISPLAYCONFIG_SDR_WHITE_LEVEL::SDRWhiteLevel` counts thousandths of [`SCRGB_WHITE_NITS`].
const SDR_WHITE_LEVEL_PER_SCRGB_WHITE: f32 = 1000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HdrDisplay {
    /// Windows HDR is on for this display — its output colour space is BT.2100 PQ.
    pub hdr_enabled: bool,
    /// SDR white in nits: where UI white has to land in scRGB to match every other window.
    pub sdr_white_nits: f32,
    /// Peak luminance the panel reports, in nits (0 when it reports none).
    pub max_nits: f32,
}

impl HdrDisplay {
    /// The scRGB value of SDR white.
    pub fn sdr_white_scale(&self) -> f32 {
        self.sdr_white_nits / SCRGB_WHITE_NITS
    }
}

/// Reads the display `hwnd` is on, or `None` when no DXGI output matches its monitor.
///
/// `factory` must be current (`IDXGIFactory1::IsCurrent`): DXGI snapshots each output's colour
/// space when the factory is created, so a stale factory keeps reporting the HDR state from before
/// the user toggled it.
pub(crate) fn probe(factory: &IDXGIFactory1, hwnd: HWND) -> Option<HdrDisplay> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let desc = output_desc(factory, monitor)?;
    Some(HdrDisplay {
        hdr_enabled: desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
        // Without a reading, assume the scRGB default rather than guessing a brighter white.
        sdr_white_nits: sdr_white_nits(&desc.DeviceName).unwrap_or(SCRGB_WHITE_NITS),
        max_nits: desc.MaxLuminance,
    })
}

/// The DXGI output showing `monitor`, on whichever adapter drives it.
fn output_desc(factory: &IDXGIFactory1, monitor: HMONITOR) -> Option<DXGI_OUTPUT_DESC1> {
    for adapter_index in 0.. {
        // `DXGI_ERROR_NOT_FOUND` past the last adapter ends the search.
        let adapter = unsafe { factory.EnumAdapters1(adapter_index) }.ok()?;
        for output_index in 0.. {
            let Ok(output) = (unsafe { adapter.EnumOutputs(output_index) }) else {
                break;
            };
            let Ok(output) = output.cast::<IDXGIOutput6>() else {
                continue;
            };
            if let Ok(desc) = unsafe { output.GetDesc1() }
                && desc.Monitor == monitor
            {
                return Some(desc);
            }
        }
    }
    None
}

/// SDR white of the display whose GDI device name is `gdi_device_name` (`\\.\DISPLAYn`).
fn sdr_white_nits(gdi_device_name: &[u16]) -> Option<f32> {
    let mut path_count = 0;
    let mut mode_count = 0;
    unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count) }
        .ok()
        .ok()?;
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    }
    .ok()
    .ok()?;
    paths.truncate(path_count as usize);

    paths.iter().find_map(|path| {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: info_header::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
                DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                path.sourceInfo.adapterId,
                path.sourceInfo.id,
            ),
            ..Default::default()
        };
        if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != ERROR_SUCCESS.0 as i32
            || trim_nul(&source.viewGdiDeviceName) != trim_nul(gdi_device_name)
        {
            return None;
        }
        // The white level belongs to the target (the monitor), not the source.
        let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL {
            header: info_header::<DISPLAYCONFIG_SDR_WHITE_LEVEL>(
                DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                path.targetInfo.adapterId,
                path.targetInfo.id,
            ),
            SDRWhiteLevel: 0,
        };
        if unsafe { DisplayConfigGetDeviceInfo(&mut white.header) } != ERROR_SUCCESS.0 as i32
            || white.SDRWhiteLevel == 0
        {
            return None;
        }
        Some(white.SDRWhiteLevel as f32 / SDR_WHITE_LEVEL_PER_SCRGB_WHITE * SCRGB_WHITE_NITS)
    })
}

/// Header for a `DisplayConfigGetDeviceInfo` request packet of type `T`.
fn info_header<T>(
    kind: DISPLAYCONFIG_DEVICE_INFO_TYPE,
    adapter_id: LUID,
    id: u32,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: kind,
        size: std::mem::size_of::<T>() as u32,
        adapterId: adapter_id,
        id,
    }
}

fn trim_nul(name: &[u16]) -> &[u16] {
    &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())]
}
