use x11::xrandr;
use std::slice;

use crate::crtc::Rotation;
use crate::Xid;

#[derive(Debug, Clone)]
pub struct Mode {
    pub xid: Xid,
    pub width: u32,
    pub height: u32,
    pub dot_clock: u64,
    pub hsync_tart: u32,
    pub hsync_end: u32,
    pub htotal: u32,
    pub hskew: u32,
    pub vsync_start: u32,
    pub vsync_end: u32,
    pub vtotal: u32,
    pub name: String,
    pub flags: u64,
    pub rate: f64,
}

impl Mode {
    // Width and height, accounting for rotation
    pub fn rot_size(&self, rot: Rotation) -> (i32, i32) {
        let (w, h) = ( self.width as i32, self.height as i32);

        match rot {
            Rotation::Normal | Rotation::Inverted   => (w, h),
            Rotation::Left | Rotation::Right        => (h, w),
        }
    }
}


const RR_INTERLACE: u64 = 0x00000010;
const RR_DOUBLE_SCAN: u64 = 0x00000020;

fn rate_from_mode(mode: &xrandr::XRRModeInfo) -> f64 {
    let v_total = 
        if mode.modeFlags & RR_DOUBLE_SCAN != 0 { mode.vTotal * 2 }
        else if mode.modeFlags & RR_INTERLACE != 0 { mode.vTotal / 2 }
        else { mode.vTotal };

    assert!(mode.hTotal != 0 && mode.vTotal != 0);

    mode.dotClock as f64 / (mode.hTotal as f64* v_total as f64)
}


impl From<&xrandr::XRRModeInfo> for Mode {
    fn from(x_mode: &xrandr::XRRModeInfo) -> Self {
        let name_b = unsafe {
            slice::from_raw_parts(
                x_mode.name as *const u8,
                x_mode.nameLength as usize,
            )
        };

        Self {
            xid: x_mode.id,
            name: String::from_utf8_lossy(name_b).into_owned(),
            width: x_mode.width,
            height: x_mode.height,
            dot_clock: x_mode.dotClock,
            hsync_tart: x_mode.hSyncStart,
            hsync_end: x_mode.hSyncEnd,
            htotal: x_mode.hTotal,
            hskew: x_mode.hSkew,
            vsync_start: x_mode.vSyncStart,
            vsync_end: x_mode.vSyncEnd,
            vtotal: x_mode.vTotal,
            rate: rate_from_mode(x_mode),
            flags: x_mode.modeFlags,
        }
    }
}

