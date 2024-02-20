use itertools::EitherOrBoth as ZipEntry;
use itertools::Itertools;
use std::collections::HashMap;
use std::ffi::CStr;
use std::fmt::Debug;
use std::os::raw::c_ulong;
use std::ptr;

use crtc::normalize_positions;
pub use indexmap;
pub use screen_resources::ScreenResources;
use thiserror::Error;
use x11::{xlib, xrandr};

pub use crate::crtc::Crtc;
pub use crate::crtc::{Relation, Rotation};
pub use crate::mode::Mode;
pub use crate::monitor::Monitor;
use crate::monitor::MonitorHandle;
pub use crate::screensize::ScreenSize;
pub use output::{
    property::{Property, Range, Ranges, Supported, Value, Values},
    Output,
};

mod crtc;
mod mode;
mod monitor;
mod output;
mod screen_resources;
mod screensize;

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

impl XHandle {
    /// Open a handle to the lib-xrandr backend. This will be
    /// used for nearly all interactions with the xrandr lib
    ///
    /// # Errors
    /// * `XrandrError::Open` - Getting the handle failed.
    ///
    pub fn open() -> Result<Self, XrandrError> {
        // XOpenDisplay argument is screen name
        // Null pointer gets first display?
        let sys = ptr::NonNull::new(unsafe { xlib::XOpenDisplay(ptr::null()) })
            .ok_or(XrandrError::Open)?;

        Ok(Self { sys })
    }

    /// List every monitor
    ///
    /// # Errors
    /// * `XrandrError::_` - various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// let mon1 = xhandle.monitors().unwrap().first().unwrap();
    /// ```
    ///
    pub fn monitors(&mut self) -> Result<Vec<Monitor>, XrandrError> {
        let infos = MonitorHandle::new(self)?;
        let res = ScreenResources::new(self)?;

        infos
            .as_slice()
            .iter()
            .map(|sys| {
                let outputs = unsafe { Output::from_list(self, &res, sys.outputs, sys.noutput) }?;

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

    /// List every monitor's outputs
    ///
    /// # Errors
    /// * `XrandrError::_` - various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// let output = xhandle.all_outputs().unwrap().first().unwrap();
    /// ```
    ///
    pub fn all_outputs(&mut self) -> Result<Vec<Output>, XrandrError> {
        ScreenResources::new(self)?.outputs(self)
    }

    // TODO: this seems to be more complicated in xrandr.c
    // Finds an available Crtc for a given (disabled) output
    fn find_available_crtc(&mut self, o: &Output) -> Result<Crtc, XrandrError> {
        let res = ScreenResources::new(self)?;
        let crtcs = res.crtcs(self)?;

        for crtc in crtcs {
            if crtc.possible.contains(&o.xid) && crtc.outputs.is_empty() {
                return Ok(crtc);
            }
        }

        Err(XrandrError::NoCrtcAvailable)
    }

    /// Enable the given output by setting it to its preferred mode
    ///
    /// # Errors
    /// * `XrandrError::_` - various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    ///
    /// // Find output
    /// let outputs = xhandle.all_outputs().unwrap();
    /// let output = outputs.iter().find(|o| o.current_mode.is_some()).unwrap();
    /// ```
    /// ```rust,ignore
    /// xhandle.enable(output).unwrap();
    /// ```
    ///
    pub fn enable(&mut self, output: &Output) -> Result<(), XrandrError> {
        if output.current_mode.is_some() {
            return Ok(());
        }

        let target_mode = output
            .preferred_modes
            .first()
            .ok_or(XrandrError::NoPreferredModes(output.xid))?;

        let mut crtc = self.find_available_crtc(output)?;
        let mode = ScreenResources::new(self)?.mode(*target_mode)?;

        crtc.mode = mode.xid;
        crtc.width = mode.width;
        crtc.height = mode.height;
        crtc.outputs = vec![output.xid];

        self.apply_new_crtcs(&mut [crtc])
    }

    /// Disable the given output
    ///
    /// # Errors
    /// * `XrandrError::_` - various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    ///
    /// // Find output
    /// let outputs = xhandle.all_outputs().unwrap();
    /// let output = outputs.iter().find(|o| o.current_mode.is_some()).unwrap();
    /// ```
    /// ```rust,ignore
    /// xhandle.disable(output).unwrap();
    /// ```
    ///
    pub fn disable(&mut self, output: &Output) -> Result<(), XrandrError> {
        let crtc_id = match output.crtc {
            None => return Ok(()),
            Some(xid) => xid,
        };

        let res = ScreenResources::new(self)?;
        let mut crtc = res.crtc(self, crtc_id)?;
        crtc.set_disable();

        self.apply_new_crtcs(&mut [crtc])
    }

    /// Sets the given output as the primary output
    ///
    /// # Errors
    /// * `XrandrError::_` - various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    ///
    /// // Find output
    /// let outputs = xhandle.all_outputs().unwrap();
    /// let output = outputs.iter().find(|o| o.current_mode.is_some()).unwrap();
    /// ```
    /// ```rust,ignore
    /// xhandle.set_primary(output);
    /// ```
    ///
    pub fn set_primary(&mut self, o: &Output) {
        unsafe {
            xrandr::XRRSetOutputPrimary(self.sys.as_ptr(), self.root(), o.xid);
        }
    }

    // - xrandr does not seem to resize after a rotation, and so I will not do so either
    // TODO: this should probably just take an xid for the mode?
    /// Sets the mode of a given output, relative to another
    ///
    /// # Arguments
    /// * `output` - The output to change mode for
    /// * `mode` - The mode to change to
    ///
    /// # Errors
    /// * `XrandrError::_` - various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    ///
    /// // Find output
    /// let outputs = xhandle.all_outputs().unwrap();
    /// let output = outputs.iter().find(|o| o.current_mode.is_some()).unwrap();
    ///
    /// // Get the preferred mode
    /// let preferred_mode_id = output.preferred_modes.first().unwrap();
    /// let res = xrandr::ScreenResources::new(&mut xhandle).unwrap();
    /// let preferred_mode = res.mode(*preferred_mode_id).unwrap();
    /// ```
    /// ```rust,ignore
    /// xhandle.set_mode(output, &preferred_mode).unwrap();
    /// ```
    ///
    pub fn set_mode(&mut self, output: &Output, mode: &Mode) -> Result<(), XrandrError> {
        let crtc_id = output
            .crtc
            .ok_or(XrandrError::OutputDisabled(output.name.clone()))?;
        let mut crtc = ScreenResources::new(self)?.crtc(self, crtc_id)?;

        crtc.mode = mode.xid;
        self.apply_new_crtcs(&mut [crtc])
    }

    /// Sets the position of a given output, relative to another
    ///
    /// # Arguments
    /// * `output` - The output to reposition
    /// * `relation` - The relation `output` will have to `rel_output`
    /// * `rel_output` - The output to position relative to
    ///
    /// # Errors
    /// * `XrandrError::_` - various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    ///
    /// // find primary output
    /// let outputs = xhandle.all_outputs().unwrap();
    /// let primary_output = outputs.iter().find(|o| o.is_primary)
    ///     .expect("Could not determine primary display");
    /// ```
    /// ```rust,ignore
    /// // Find a second output and position relative to it
    /// let other_output = outputs.iter().find(|o| o.current_mode.is_some() && o.xid != primary_output.xid)
    ///     .expect("Cannot test `set_position` with fewer than 2 enabled displays");
    /// xhandle.set_position(primary_output, xrandr::Relation::LeftOf, other_output).unwrap();
    /// ```
    ///
    pub fn set_position(
        &mut self,
        output: &Output,
        relation: Relation,
        relative_output: &Output,
    ) -> Result<(), XrandrError> {
        let crtc_id = output
            .crtc
            .ok_or(XrandrError::OutputDisabled(output.name.clone()))?;
        let rel_crtc_id = relative_output
            .crtc
            .ok_or(XrandrError::OutputDisabled(relative_output.name.clone()))?;

        let res = ScreenResources::new(self)?;
        let mut crtc = res.crtc(self, crtc_id)?;
        let rel_crtc = res.crtc(self, rel_crtc_id)?;

        // Calculate new (x,y) based on:
        // - own width/height & relative output's width/height/x/y
        let (w, h) = (crtc.width as i32, crtc.height as i32);
        let (rel_w, rel_h) = (rel_crtc.width as i32, rel_crtc.height as i32);
        let (rel_x, rel_y) = (rel_crtc.x, rel_crtc.y);

        (crtc.x, crtc.y) = match relation {
            Relation::LeftOf => (rel_x - w, rel_y),
            Relation::RightOf => (rel_x + rel_w, rel_y),
            Relation::Above => (rel_x, rel_y - h),
            Relation::Below => (rel_x, rel_y + rel_h),
            Relation::SameAs => (rel_x, rel_y),
        };

        self.apply_new_crtcs(&mut [crtc])
    }

    /// Sets the position of a given output, relative to another
    ///
    /// # Arguments
    /// * `output` - The output to rotate
    /// * `rotation`
    ///
    /// # Errors
    /// * `XrandrError::_` - various calls to the xrandr backend may fail
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// 
    /// // Find enabled output
    /// let outputs = xhandle.all_outputs().unwrap();
    /// let output = outputs.iter().find(|o| o.current_mode.is_some()).unwrap();
    /// ```
    /// ```rust,ignore
    /// // Turn upside-down
    /// xhandle.set_rotation(output, xrandr::Rotation::Inverted).unwrap();
    /// ```
    ///
    pub fn set_rotation(
        &mut self,
        output: &Output,
        rotation: Rotation,
    ) -> Result<(), XrandrError> {
        let crtc_id = output
            .crtc
            .ok_or(XrandrError::OutputDisabled(output.name.clone()))?;

        let res = ScreenResources::new(self)?;
        let mut crtc = res.crtc(self, crtc_id)?;

        (crtc.width, crtc.height) = crtc.rotated_size(rotation);
        crtc.rotation = rotation;

        self.apply_new_crtcs(&mut [crtc])
    }

    /// Applies some set of altered crtcs
    /// Due to xrandr's structure, changing one or more crtcs properly can be
    /// quite complicated. One should therefore call this function on any crtcs
    /// that you want to change.
    /// # Arguments
    /// * `changes`
    ///     Altered crtcs. Must be mutable because of crct.apply() calls.
    ///
    fn apply_new_crtcs(&mut self, changed: &mut [Crtc]) -> Result<(), XrandrError> {
        let mut res = ScreenResources::new(self)?;
        let old_crtcs = res.enabled_crtcs(self)?;

        // Construct new crtcs out of the old ones and the new where provided
        let mut changed_map: HashMap<XId, Crtc> = HashMap::new();
        changed.iter().cloned().for_each(|c| {
            changed_map.insert(c.xid, c);
        });

        let mut new_crtcs: Vec<Crtc> = Vec::new();
        for crtc in &old_crtcs {
            match changed_map.remove(&crtc.xid) {
                None => new_crtcs.push(crtc.clone()),
                Some(c) => new_crtcs.push(c.clone()),
            }
        }
        new_crtcs.extend(changed_map.drain().map(|(_, v)| v));

        // In case the top-left corner is no longer at (0,0), renormalize
        normalize_positions(&mut new_crtcs);
        let new_size = ScreenSize::fitting_crtcs(self, &new_crtcs);

        // Disable crtcs that do not fit before setting the new size
        // Note that this should only be crtcs that were changed, but `changed`
        // contains the already altered crtc, so we have to use `old_crtcs`
        let mut old_crtcs = old_crtcs;
        for crtc in &mut old_crtcs {
            if !new_size.fits_crtc(crtc) {
                crtc.set_disable();
                res.set_crtc_config(self, crtc)?;
            }
        }
        self.set_screensize(&new_size);

        // Find the crtcs that were changed. Done at this point to also account
        // for crtcs that were altered by normalize_positions()
        let mut to_apply: Vec<&mut Crtc> = Vec::new();
        for pair in old_crtcs.iter().zip_longest(new_crtcs.iter_mut()) {
            match pair {
                ZipEntry::Both(old, new) => {
                    assert!(old.xid == new.xid, "invalid new_crtcs");
                    if new.timestamp < old.timestamp {
                        return Err(XrandrError::CrtcChanged(new.xid));
                    }
                    if new != old {
                        to_apply.push(new);
                    }
                }
                ZipEntry::Right(new) => to_apply.push(new),
                ZipEntry::Left(_) => unreachable!("invalid new_crtcs"),
            }
        }

        // Move and re-enable the crtcs
        to_apply
            .iter_mut()
            .try_for_each(|c| res.set_crtc_config(self, c))
    }

    /// Sets the screen size in the x backend
    fn set_screensize(&mut self, size: &ScreenSize) {
        unsafe {
            xrandr::XRRSetScreenSize(
                self.sys.as_ptr(),
                self.root(),
                size.width,
                size.height,
                size.width_mm,
                size.height_mm,
            );
        }
    }

    fn root(&mut self) -> c_ulong {
        unsafe { xlib::XDefaultRootWindow(self.sys.as_ptr()) }
    }
}

impl Drop for XHandle {
    fn drop(&mut self) {
        unsafe { xlib::XCloseDisplay(self.sys.as_ptr()) };
    }
}

fn real_bool(sys: xlib::Bool) -> bool {
    assert!(
        sys == 0 || sys == 1,
        "Integer larger than 1 does not represent a bool"
    );
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

    #[error("Crtc changed since last requesting its state")]
    CrtcChanged(xlib::XID),

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
    use std::thread::sleep;

    use super::*;

    #[test]
    fn can_open() {
        XHandle::open().unwrap();
    }

    #[test]
    fn can_get_monitor() {
        let mut handle = XHandle::open().unwrap();
        let monitors = handle.monitors().unwrap();
        let monitor = monitors.first().unwrap();
        eprintln!("Successfully found monitor:\n{:?}", monitor);
    }
    
    #[test]
    fn can_get_output() {
        let mut handle = XHandle::open().unwrap();
        let outputs = handle.all_outputs().unwrap();
        let output = outputs.first().unwrap();
        eprintln!("Successfully found output:\n{:?}", output);
    }
    
    #[test]
    #[ignore] // ignore setter methods by default
    fn can_disable_enable_output() {
        if std::env::var("XRANDR_TEST_NO_SET_METHODS").is_ok() { return }

        let mut handle = XHandle::open().unwrap();
        let outputs = handle.all_outputs().unwrap();
        let output = outputs.iter().find(|o| o.current_mode.is_some()).unwrap();

        handle.disable(output).unwrap();
        sleep(core::time::Duration::from_secs(2));
        handle.enable(output).unwrap();
    }

    #[test]
    #[ignore] // ignore setter methods by default
    fn can_rotate() {
        if std::env::var("XRANDR_TEST_NO_SET_METHODS").is_ok() { return }

        let mut handle = XHandle::open().unwrap();
        let outputs = handle.all_outputs().unwrap();
        let output = outputs.first().unwrap();

        handle.set_rotation(output, Rotation::Left).unwrap();
        sleep(core::time::Duration::from_secs(1));
        handle.set_rotation(output, Rotation::Inverted).unwrap();
        sleep(core::time::Duration::from_secs(1));
        handle.set_rotation(output, Rotation::Right).unwrap();
        sleep(core::time::Duration::from_secs(1));
        handle.set_rotation(output, Rotation::Normal).unwrap();
    }

    #[test]
    #[ignore] // ignore setter methods by default
    fn can_set_primary() {
        if std::env::var("XRANDR_TEST_NO_SET_METHODS").is_ok() { return }

        let mut handle = XHandle::open().unwrap();
        let outputs = handle.all_outputs().unwrap();
        assert!(outputs.len() > 1, "Cannot test `set_primary` with fewer than 2 displays");

        let primary_output = outputs.iter().find(|o| o.is_primary)
            .expect("Could not determine primary display");
        let other_output = outputs.iter().find(|o| o.xid != primary_output.xid)
            .expect("Cannot test `set_primary` with fewer than 2 displays");

        handle.set_primary(other_output);
        sleep(core::time::Duration::from_secs(2));
        handle.set_primary(primary_output);
    }
    
    #[test]
    #[ignore] // ignore setter methods by default
    fn can_set_mode() {
        if std::env::var("XRANDR_TEST_NO_SET_METHODS").is_ok() { return }

        let mut handle = XHandle::open().unwrap();
        let outputs = handle.all_outputs().unwrap();
        let output = outputs.first().unwrap();

        let res = ScreenResources::new(&mut handle).unwrap();
        let current_mode_id = output.current_mode
            .expect("Could not determine current mode");
        let other_mode_id = *output.modes.iter().find(|m| **m != current_mode_id)
            .expect("Cannot test `set_mode` with fewer than 2 modes");

        let current_mode = res.mode(current_mode_id).unwrap();
        let other_mode = res.mode(other_mode_id).unwrap();

        handle.set_mode(output, &other_mode).unwrap();
        sleep(core::time::Duration::from_secs(2));
        handle.set_mode(output, &current_mode).unwrap();
    }

    #[test]
    #[ignore] // ignore setter methods by default
    fn can_set_position() {
        if std::env::var("XRANDR_TEST_NO_SET_METHODS").is_ok() { return }

        let mut handle = XHandle::open().unwrap();
        let outputs = handle.all_outputs().unwrap();
        assert!(outputs.len() > 1, "Cannot test `set_position` with fewer than 2 displays");

        let primary_output = outputs.iter().find(|o| o.is_primary)
            .expect("Could not determine primary display");
        let other_output = outputs.iter().find(|o| o.current_mode.is_some() && o.xid != primary_output.xid)
            .expect("Cannot test `set_position` with fewer than 2 enabled displays");

        handle.set_position(primary_output, Relation::LeftOf, other_output).unwrap();
    }
}
