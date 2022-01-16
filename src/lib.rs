#![warn(clippy::cargo)]

//! This crate aims to provide safe bindings to libxrandr. It currently supports reading most
//! monitor properties.
//!
//! ```
//! # use xrandr::XHandle;
//! let monitors = XHandle::open()?
//!     .monitors()?;
//!
//! println!("{:#?}", monitors);
//! # Ok::<_, xrandr::XrandrError>(())
//! ```
//!
//! Example output on my laptop:
//! ```rust,ignore
//! [
//!     Monitor {
//!         name: "eDP-1",
//!         is_primary: true,
//!         is_automatic: true,
//!         x: 0,
//!         y: 0,
//!         width_px: 1920,
//!         height_px: 1080,
//!         width_mm: 344,
//!         height_mm: 194,
//!         outputs: [
//!             Output {
//!                 xid: 66,
//!                 name: "eDP-1",
//!                 properties: {
//!                     "EDID": Property {
//!                         name: "EDID",
//!                         value: Edid([ 0, 255, 255, /* ... */ 80, 68, 49, 0, 62, ]),
//!                         values: None,
//!                         is_immutable: false,
//!                         is_pending: false,
//!                     },
//!                     "scaling mode": Property {
//!                         name: "scaling mode",
//!                         value: Atom("Full aspect"),
//!                         values: Some(
//!                             Supported(
//!                                 Atom(
//!                                     [
//!                                         "Full",
//!                                         "Center",
//!                                         "Full aspect",
//!                                     ],
//!                                 ),
//!                             ),
//!                         ),
//!                         is_immutable: false,
//!                         is_pending: false,
//!                     },
//!                     /* ... */
//!                     "non-desktop": Property {
//!                         name: "non-desktop",
//!                         value: Integer32([0]),
//!                         values: Some(
//!                             Range(
//!                                 Integer8(
//!                                     [
//!                                         Range {
//!                                             lower: 0,
//!                                             upper: 1,
//!                                         },
//!                                     ],
//!                                 ),
//!                             ),
//!                         ),
//!                         is_immutable: true,
//!                         is_pending: false,
//!                     },
//!                 },
//!             },
//!         ],
//!     },
//! ]
//! ```

use std::ffi::CStr;
use std::fmt::Debug;
use std::os::raw::c_ulong;
use std::{ptr, slice};

pub use indexmap;
#[cfg(feature = "serialize")]
use serde::{Deserialize, Serialize};
use thiserror::Error;
use x11::{xlib, xrandr};

pub use output::{
    property::{Property, PropertyValue, PropertyValues, Range, Ranges, Supported},
    Output,
};

mod output;

type HandleSys = ptr::NonNull<xlib::Display>;

#[derive(Debug)]
pub struct XHandle {
    sys: HandleSys,
}

impl XHandle {
    pub fn open() -> Result<Self, XrandrError> {
        let sys = ptr::NonNull::new(unsafe { xlib::XOpenDisplay(ptr::null()) })
            .ok_or(XrandrError::Open)?;

        Ok(Self { sys })
    }

    /// List every monitor
    pub fn monitors(&mut self) -> Result<Vec<Monitor>, XrandrError> {
        let mut count = 0;
        let infos =
            unsafe { xrandr::XRRGetMonitors(self.sys.as_ptr(), self.root(), 0, &mut count) };
        if count == -1 {
            return Err(XrandrError::GetMonitors);
        }
        let count = count as usize;
        let data = ptr::NonNull::new(infos).expect("Succeeded, so non-null");

        let list = unsafe { slice::from_raw_parts_mut(data.as_ptr(), count) }
            .iter()
            .map(|sys| {
                let outputs = unsafe { Output::from_list(self, sys.outputs, sys.noutput) }?;

                Ok(Monitor {
                    name: atom_name(&mut self.sys, sys.name)?,
                    is_primary: real_bool(sys.primary),
                    is_automatic: real_bool(sys.automatic),
                    x: sys.x,
                    y: sys.y,
                    width_px: sys.width,
                    height_px: sys.height,
                    width_mm: sys.mwidth,
                    height_mm: sys.mheight,
                    outputs,
                })
            })
            .collect::<Result<_, _>>()?;

        unsafe {
            xrandr::XRRFreeMonitors(data.as_ptr());
        }

        Ok(list)
    }

    /// List every monitor's outputs
    pub fn all_outputs(&mut self) -> Result<Vec<Output>, XrandrError> {
        let res = self.res()?;
        unsafe { Output::from_list(self, res.outputs, res.noutput) }
    }

    fn res<'r, 'h>(&'h mut self) -> Result<&'r mut xrandr::XRRScreenResources, XrandrError>
    where
        'r: 'h,
    {
        let res = unsafe {
            ptr::NonNull::new(xrandr::XRRGetScreenResources(
                self.sys.as_ptr(),
                self.root(),
            ))
            .ok_or(XrandrError::GetResources)?
            .as_mut()
        };
        Ok(res)
    }

    fn root(&mut self) -> c_ulong {
        unsafe { xlib::XDefaultRootWindow(self.sys.as_ptr()) }
    }
}

impl Drop for XHandle {
    fn drop(&mut self) {
        unsafe {
            xlib::XCloseDisplay(self.sys.as_ptr());
        }
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
pub struct Monitor {
    pub name: String,
    pub is_primary: bool,
    pub is_automatic: bool,
    pub x: i32,
    pub y: i32,
    pub width_px: i32,
    pub height_px: i32,
    pub width_mm: i32,
    pub height_mm: i32,
    /// An Output describes an actual physical monitor or display. A [`Monitor`]
    /// can have more than one output.
    pub outputs: Vec<Output>,
}

fn real_bool(sys: xlib::Bool) -> bool {
    assert!(sys == 0 || sys == 1);
    sys == 1
}

fn atom_name(handle: &mut HandleSys, atom: xlib::Atom) -> Result<String, XrandrError> {
    let chars = ptr::NonNull::new(unsafe { xlib::XGetAtomName(handle.as_ptr(), atom) })
        .ok_or(XrandrError::GetAtomName(atom))?;

    let name = unsafe { CStr::from_ptr(chars.as_ptr()) }
        .to_string_lossy()
        .to_string();

    unsafe {
        xlib::XFree(chars.as_ptr().cast());
    }

    Ok(name)
}

#[derive(Error, Debug)]
pub enum XrandrError {
    #[error("Failed to open connection to x11. Check out DISPLAY environment variable.")]
    Open,
    #[error("Call to XRRGetMonitors failed.")]
    GetMonitors,
    #[error("Call to XRRGetScreenResources for XRRDefaultRootWindow failed")]
    GetResources,
    #[error("Call to XRRGetOutputInfo for output with xid {0} failed")]
    GetOutputInfo(xlib::XID),
    #[error("Failed to get the properties of output with xid {0}")]
    GetOutputProp(xlib::XID),
    #[error("Failed to name of atom {0}")]
    GetAtomName(xlib::Atom),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle() -> XHandle {
        XHandle::open().unwrap()
    }

    #[test]
    fn can_open() {
        handle();
    }

    #[test]
    fn can_debug_format_monitors() {
        format!("{:#?}", handle().monitors().unwrap());
    }
}
