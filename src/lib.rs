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

pub use indexmap;
pub use screen_resources::ScreenResources;
use thiserror::Error;
use x11::{xlib, xrandr};

use crate::crtc::normalize_positions;
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


// TODO these are private in xrandr, so redfine i guess
pub type Time = c_ulong;
pub type Xid = c_ulong;
// TODO: this seems to be what xrandr does... am I missing something?
const CURRENT_TIME: c_ulong = 0;


// The main handle consists simply of a pointer to the display
type HandleSys = ptr::NonNull<xlib::Display>;
#[derive(Debug)]
pub struct XHandle {
    sys: HandleSys,
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
        let sys = ptr::NonNull::new(unsafe{ xlib::XOpenDisplay(ptr::null()) })
            .ok_or(XrandrError::Open)?;

        Ok(Self { sys })
    }

    pub(crate) fn res<'r, 'h>(
        &'h mut self,
    ) -> Result<&'r mut xrandr::XRRScreenResources, XrandrError>
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
        let mut count = 0;

        let infos = unsafe {
            xrandr::XRRGetMonitors(
                self.sys.as_ptr(),
                self.root(),
                0,
                &mut count,
            )
        };

        if count == -1 {
            return Err(XrandrError::GetMonitors);
        }

        let count = count as usize;
        let data = ptr::NonNull::new(infos).expect("Succeeded, so non-null");

        let list = unsafe { slice::from_raw_parts_mut(data.as_ptr(), count) }
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
            .collect::<Result<_, _>>()?;

        unsafe {
            xrandr::XRRFreeMonitors(data.as_ptr());
        }

        Ok(list)
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
        let res_o = ScreenResources::new(self)?;
        let crtcs = res_o.crtcs(self)?;

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
    pub fn enable(&mut self, o: &Output) -> Result<(), XrandrError> {
        if o.current_mode.is_some() { return Ok(()) }

        let target_mode = o.preferred_modes.first()
            .ok_or(XrandrError::GetOutputInfo(o.xid))?; // TODO better error?

        let mut crtc = self.find_available_crtc(o)?;
        let mode = ScreenResources::new(self)?.mode(*target_mode)?;

        crtc.mode = mode.xid;
        crtc.outputs = vec![o.xid];
        crtc.apply(self)?;

        Ok(())
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
        if output.crtc == 0 { 
            return Err(XrandrError::OutputDisabled(output.name.clone())) 
        }

        let res = ScreenResources::new(self)?;
        let mut old_crtcs: Vec<Crtc> = res.enabled_crtcs(self)?;
        let mut new_crtcs: Vec<Crtc> = old_crtcs.clone();
        
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
        rel_output: &Output,) 
        -> Result<(), XrandrError> 
    {
        let res = ScreenResources::new(self)?;
        
        let mut old_crtcs: Vec<Crtc> = res.enabled_crtcs(self)?;
        let mut new_crtcs: Vec<Crtc> = old_crtcs.clone();

        let crtc: &mut Crtc = new_crtcs.iter_mut()
            .find(|c| c.xid == output.crtc)
            .ok_or(XrandrError::GetResources)?;
        let rel_crtc = res.crtc(self, rel_output.crtc)?;

        // Calculate new (x,y) based on:
        // - own width/height
        // - relative outputs width/height/x/y
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

        let mut new_crtcs = normalize_positions(&new_crtcs);
        self.apply_new_crtcs(&mut old_crtcs, &mut new_crtcs)
    }

    // TODO: this seems to not resize the actual window, leaving black space
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
        let mut old_crtcs: Vec<Crtc> = ScreenResources::new(self)?
            .crtcs(self)?.into_iter()
            .filter(|c| c.mode != 0)
            .collect();
        let mut crtcs = old_crtcs.clone();

        let mut crtc = crtcs.iter_mut()
            .find(|c| c.xid == output.crtc)
            .ok_or(XrandrError::NoCrtcAvailable)?;

        (crtc.width, crtc.height) = crtc.rot_size(*rotation);
        crtc.rotation = *rotation;

        self.apply_new_crtcs(&mut old_crtcs, &mut crtcs)
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
    #[error("Missing before-state for changing CRTC with xid {0}")]
    GetCrtc(xlib::XID),
    #[error("Call to XRRGetOutputInfo for output with xid {0} failed")]
    GetOutputInfo(xlib::XID),
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
