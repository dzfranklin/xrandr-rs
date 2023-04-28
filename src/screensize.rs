use x11::{xrandr,xlib};

use crate::XHandle;
use crate::crtc::Crtc;

// The amount of milimeters in an inch, needed for dpi calculation
const INCH_MM: f32 = 25.4;

pub struct ScreenSize {
    width: i32,
    width_mm: i32,
    height: i32,
    height_mm: i32,
}

impl ScreenSize {
    /// Sets the screen size in the x backend
    pub fn set(&self, handle: &mut XHandle) {
        unsafe {
            xrandr::XRRSetScreenSize(
                handle.sys.as_ptr(),
                handle.root(),
                self.width,
                self.height,
                self.width_mm,
                self.height_mm,
            );
        }
    }

    /// True iff the given crtc fits on a screen of this size
    #[must_use] pub fn fits_crtc(&self, crtc: &Crtc) -> bool {
        let (max_x, max_y) = crtc.max_coordinates();
        max_x as i32 <= self.width && max_y as i32 <= self.height

    }

    /// Calculates the screen size that (snugly) fits a set of crtcs
    pub(crate) fn fitting_crtcs(
        handle: &mut XHandle, crtcs: &[Crtc]) 
    -> Self 
    {
        assert!(!crtcs.is_empty()); // see also: following unwraps

        let width = crtcs.iter()
            .map(|p| p.max_coordinates().0)
            .max()
            .unwrap() as i32;
        let height = crtcs.iter()
            .map(|p| p.max_coordinates().1)
            .max()
            .unwrap() as i32;

        // Get the old sizes to calculate the dpi
        let c_h = unsafe { xlib::XDisplayHeight(handle.sys.as_ptr(), 0) };
        let c_h_mm = unsafe { xlib::XDisplayHeightMM(handle.sys.as_ptr(), 0) };
        
        // Calculate the new physical size with the dpi and px count
        let dpi: f32 = (INCH_MM * c_h as f32) / c_h_mm as f32;

        let width_mm = ((INCH_MM * width as f32) / dpi ) as i32;
		let height_mm = ((INCH_MM * height as f32) / dpi ) as i32;

        ScreenSize{ width, width_mm, height, height_mm }
    }
}

