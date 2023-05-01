#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
// TODO this is done atm because xrandr seems to mix it's number types a lot
// and I cannot be bothered to do proper conversion everywhere (yet)
// Maybe I am missing something and I should handle them differently?

use std::ffi::CStr;
use std::fmt::Debug;
use std::os::raw::c_ulong;
use std::{ptr, slice};

use crtc::normalize_positions;
pub use indexmap;
pub use screen_resources::ScreenResources;
use thiserror::Error;
use x11::{xlib, xrandr};

pub use crate::crtc::Crtc;
pub use crate::crtc::{Rotation, Relation};
pub use crate::mode::Mode;
pub use crate::screensize::ScreenSize;
pub use output::{
    property::{
        Property, 
        Value, 
        Values, 
        Range, 
        Ranges, 
        Supported,
    },
    Output, 
};

mod screen_resources;
mod screensize;
mod output;
mod mode;
mod crtc;


// All retrieved information is timestamped by when that information was 
// last changed in the backend. If we alter an object (e.g. crtc, output) we 
// have to pass the timestamp we got with it. If the x backend detects that 
// changes have occured since we retrieved the information, our new change 
// will not go through.
pub type XTime = c_ulong;
// Xrandr seems to want the time `0` when calling setter functions
const CURRENT_TIME: c_ulong = 0;
// Unique identifiers for the various objects in the x backend 
// (crtcs,outputs,modes, etc.)
pub type XId = c_ulong;


// The main handle consists simply of a pointer to the display
type HandleSys = ptr::NonNull<xlib::Display>;
#[derive(Debug)]
pub struct XHandle {
    sys: HandleSys,
}

// TODO: implement this for other pointers in the lib?
// A wrapper that drops the pointer if it goes out of scope.
// Avoid having to deal with the various early returns
struct MonitorInfo {
    pub ptr: ptr::NonNull<xrandr::XRRMonitorInfo>,
    pub count: i32,
}

impl MonitorInfo {
    fn new(handle: &mut XHandle) -> Result<Self,XrandrError> {
        let mut count = 0;

        let raw_ptr = unsafe {
            xrandr::XRRGetMonitors(
                handle.sys.as_ptr(),
                handle.root(),
                0,
                &mut count,
            )
        };
        
        if count == -1 {
            return Err(XrandrError::GetMonitors);
        }
        
        let ptr = ptr::NonNull::new(raw_ptr)
            .ok_or(XrandrError::GetMonitors)?;

        Ok(Self { ptr, count })
    }

    fn as_slice(&self) -> &[xrandr::XRRMonitorInfo] {
        unsafe { 
            slice::from_raw_parts_mut(
                self.ptr.as_ptr(), 
                self.count as usize
            )
        }
    }
}

impl Drop for MonitorInfo {
    fn drop(&mut self) {
        unsafe { xrandr::XRRFreeMonitors(self.ptr.as_ptr()) };
    }
}


impl XHandle {
    // TODO: better error documentation
    /// Open a handle to the lib-xrandr backend. This will be 
    /// used for nearly all interactions with the xrandr lib
    ///
    /// # Errors
    /// * `XrandrError::Open` - Getting the handle failed.
    ///
    /// # Examples
    /// ```
    /// let xhandle = XHandle.open()?;
    /// let mon1 = xhandle.monitors()?[0];
    /// ```
    ///
    pub fn open() -> Result<Self, XrandrError> {
        // XOpenDisplay argument is screen name
        // Null pointer gets first display?
        let sys = ptr::NonNull::new(unsafe{ xlib::XOpenDisplay(ptr::null()) })
            .ok_or(XrandrError::Open)?;

        Ok(Self { sys })
    }

    pub(crate) fn res<'r, 'h>( &'h mut self,) 
    -> Result<&'r mut xrandr::XRRScreenResources, XrandrError>
    where 'r: 'h,
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


    // TODO: better error documentation
    /// List every monitor
    ///
    /// # Errors
    /// Various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let mon1 = xhandle.monitors()?[0];
    /// ```
    ///
    pub fn monitors(&mut self) -> Result<Vec<Monitor>, XrandrError> {
        let infos = MonitorInfo::new(self)?;

        infos.as_slice()
            .iter()
            .map(|sys| {
                let outputs = unsafe {
                    Output::from_list(self, sys.outputs, sys.noutput)
                }?;

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
            .collect::<Result<_, _>>()
    }


    // TODO: better error documentation
    /// List every monitor's outputs
    ///
    /// # Errors
    /// Various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let dp_1 = xhandle.all_outputs()?[0];
    /// ```
    ///
    pub fn all_outputs(&mut self) -> Result<Vec<Output>, XrandrError> {
        ScreenResources::new(self)?.outputs(self)
    }


    // START setter methods

    // TODO: this seems to be more complicated in xrandr.c
    // Finds an available Crtc for a given (disabled) output
    fn find_available_crtc(
        &mut self, o: &Output) 
        -> Result<Crtc, XrandrError> 
    {
        let res = ScreenResources::new(self)?;
        let crtcs = res.crtcs(self)?;

        for crtc in crtcs {
            if crtc.possible.contains(&o.xid) && crtc.outputs.is_empty() {
                return Ok(crtc);
            }
        }

        Err(XrandrError::NoCrtcAvailable)
    }


    // TODO: better error documentation
    /// Enable the given output by setting it to its preferred mode
    ///
    /// # Errors
    /// Various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let dp_1 = xhandle.all_outputs()?[0];
    /// xhandle.enable(dp_1)?;
    /// ```
    ///
    pub fn enable(&mut self, output: &Output) -> Result<(), XrandrError> {
        if output.current_mode.is_some() { return Ok(()) }

        let target_mode = output.preferred_modes.first()
            .ok_or(XrandrError::NoPreferredModes(output.xid))?;

        let mut crtc = self.find_available_crtc(output)?;
        let mode = ScreenResources::new(self)?.mode(*target_mode)?;

        crtc.mode = mode.xid;
        crtc.outputs = vec![output.xid];

        crtc.apply(self)
    }

    // TODO: better error documentation
    /// Disable the given output
    ///
    /// # Errors
    /// Various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let dp_1 = xhandle.all_outputs()?[0];
    /// xhandle.disable(dp_1)?;
    /// ```
    ///
    pub fn disable(&mut self, output: &Output) -> Result<(), XrandrError> {
        // TODO: this should also be an option? 0 is not enabled
        if output.crtc == 0 { 
            return Err(XrandrError::OutputDisabled(output.name.clone())) 
        }

        let res = ScreenResources::new(self)?;
        let mut old_crtcs = res.enabled_crtcs(self)?;
        let mut new_crtcs = old_crtcs.clone();
        
        let crtc = new_crtcs.iter_mut()
            .find(|c| c.xid == output.crtc)
            .ok_or(XrandrError::NoCrtcAvailable)?;

        crtc.disable(self)?;

        self.apply_new_crtcs(&mut old_crtcs, &mut new_crtcs)
    }


    // TODO: better error documentation
    /// Sets the given output as the primary output
    ///
    /// # Errors
    /// Various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let dp_1 = xhandle.all_outputs()?[0];
    /// xhandle.set_primary(dp_1)?;
    /// ```
    ///
    pub fn set_primary(&mut self, o: &Output) {
        unsafe {
            xrandr::XRRSetOutputPrimary(
                self.sys.as_ptr(), 
                self.root(), 
                o.xid);
        }
    }


    // TODO: better error documentation
    // TODO: Resize the screen after resolution change?
    // - xrandr does not seem to resize after a rotation, and this feels
    //   similar to me. I would say let the user reposition the displays
    /// Sets the mode of a given output, relative to another
    ///
    /// # Arguments
    /// * `output` - The output to change mode for
    /// * `mode` - The mode to change to
    ///
    /// # Errors
    /// Various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let dp_1 = xhandle.all_outputs()?[0];
    /// let mode = dp_1.preferred_modes[0];
    /// xhandle.set_mode(dp_1, mode)?;
    /// ```
    ///
    pub fn set_mode(
        &mut self,
        output: &Output,
        mode: &Mode) 
        -> Result<(), XrandrError> 
    {
        let mut crtc = ScreenResources::new(self)?.crtc(self, output.crtc)?;
        crtc.mode = mode.xid;
        crtc.apply(self)
    }


    /// Applies a difference in crtcs
    /// # Arguments
    /// * `old_crtcs` 
    ///     The crtcs as they were before the change. This is required,
    ///     because crtcs that do not fit the new screen size must be disabeld
    ///     before the new screen size can be set.
    /// * `new_crtcs` 
    ///     The new crtcs to apply. This must contain the same crtcs (xids) as 
    ///     `old_crtcs` and in the same order.
    // TODO: potentially a hashmap :: xid -> (old, new)
    // -- Or CrtcChanges object
    fn apply_new_crtcs(
        &mut self,
        old_crtcs: &mut [Crtc],
        new_crtcs: &mut [Crtc])
        -> Result<(), XrandrError>
    {
        assert!(new_crtcs.len() == old_crtcs.len());

        let new_size = ScreenSize::fitting_crtcs(self, new_crtcs);

        // Disable crtcs that do not fit on the new screen
        for c in old_crtcs.iter_mut() {
            if !new_size.fits_crtc(c) { 
                c.disable(self)?; 
            }
        }

        new_size.set(self);

        // Move and enable the crtcs
        for (old_c, new_c) in old_crtcs.iter().zip(new_crtcs.iter_mut()) {
            assert!(old_c.xid == new_c.xid); 
            // The below comparison checks whether a given crtc has changed
            // so we need to make sure we are actually looking at the same crtc
            if new_c != old_c { new_c.apply(self)? }
        }

        Ok(())
    }


    // TODO: better error documentation
    /// Sets the position of a given output, relative to another
    ///
    /// # Arguments
    /// * `output` - The output to reposition
    /// * `relation` - The relation `output` will have to `rel_output`
    /// * `rel_output` - The output to position relative to
    ///
    /// # Errors
    /// Various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let dp_1 = outputs[0];
    /// let hdmi_1 = outputs[3];
    /// xhandle.set_position(dp_1, Relation::LeftOf, hdmi_1)?;
    /// ```
    ///
    pub fn set_position(
        &mut self,
        output: &Output,
        relation: &Relation,
        relative_output: &Output) 
        -> Result<(), XrandrError> 
    {
        let rel_crtc = ScreenResources::new(self)?
            .crtc(self, relative_output.crtc)?;

        let mut changes = crtc::Changes::new(self)?;
        let crtc = changes.get_new(output.crtc)?;
        
        // Calculate new (x,y) based on:
        // - own width/height & relative output's width/height/x/y
        let (w, h) = (crtc.width as i32, crtc.height as i32);
        let (rel_w, rel_h) = (rel_crtc.width as i32, rel_crtc.height as i32);
        let (rel_x, rel_y) = (rel_crtc.x, rel_crtc.y);

        (crtc.x, crtc.y) = match relation {
            Relation::LeftOf  => ( rel_x - w     , rel_y         ),
            Relation::RightOf => ( rel_x + rel_w , rel_y         ),
            Relation::Above   => ( rel_x         , rel_y - h     ),
            Relation::Below   => ( rel_x         , rel_y + rel_h ),
            Relation::SameAs  => ( rel_x         , rel_y         ),
        };

        normalize_positions(changes.get_all_news());
        changes.apply(self)
    }


    // TODO: better error documentation
    /// Sets the position of a given output, relative to another
    ///
    /// # Arguments
    /// * `output` - The output to rotate
    /// * `rotation`
    ///
    /// # Errors
    /// Various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let dp_1 = outputs[0];
    /// xhandle.set_rotation(dp_1, Rotation::Inverted)?;
    /// ```
    ///
    pub fn set_rotation(
        &mut self,
        output: &Output,
        rotation: &Rotation,
    ) -> Result<(), XrandrError> {
        let mut changes = crtc::Changes::new(self)?;
        let crtc = changes.get_new(output.crtc)?;
        
        (crtc.width, crtc.height) = crtc.rotated_size(*rotation);
        crtc.rotation = *rotation;

        changes.apply(self)
    }
}


impl Drop for XHandle {
    fn drop(&mut self) {
        unsafe { xlib::XCloseDisplay(self.sys.as_ptr()) };
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
    assert!(sys == 0 || sys == 1, 
        "Integer larger than 1 does not represent a bool");
    sys == 1
}


fn atom_name(
    handle: &mut HandleSys,
    atom: xlib::Atom,
) -> Result<String, XrandrError> {
    let chars =
        ptr::NonNull::new(unsafe { xlib::XGetAtomName(handle.as_ptr(), atom) })
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
    #[error("Failed to open connection to x11.")]
    Open,

    #[error("Call to XRRGetMonitors failed.")]
    GetMonitors,

    #[error("No CRTC available to put onto new output")]
    NoCrtcAvailable,

    #[error("Call to XRRGetScreenResources for XRRDefaultRootWindow failed")]
    GetResources,

    #[error("The output '{0}' is disabled")]
    OutputDisabled(String),

    #[error("Invalid rotation: {0}")]
    InvalidRotation(u16),

    #[error("Could not get info on mode with xid {0}")]
    GetMode(xlib::XID),

    #[error("Could not get info on crtc with xid {0}")]
    NoPreviousStateCrtc(xlib::XID),

    #[error("Call to XRRGetCrtcInfo for CRTC with xid {0} failed")]
    GetCrtcInfo(xlib::XID),
    
    #[error("Failed to get Crtc: No Crtc with ID {0}")]
    GetCrtc(xlib::XID),

    #[error("Call to XRRGetOutputInfo for output with xid {0} failed")]
    GetOutputInfo(xlib::XID),
    
    #[error("No preferred modes found for output with xid {0}")]
    NoPreferredModes(xlib::XID),

    #[error("No mode found with xid {0}")]
    GetModeInfo(xlib::XID),

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
