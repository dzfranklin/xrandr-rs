use std::{ptr, slice};
use itertools::EitherOrBoth as ZipEntry;
use itertools::Itertools;
use std::collections::HashMap;
use x11::xrandr;

use crate::ScreenSize;
use crate::crtc::{Crtc,normalize_positions};
use crate::output::Output;
use crate::Mode;
use crate::XHandle;
use crate::XrandrError;
use crate::CURRENT_TIME;
use crate::XId;
use crate::XTime;


impl Drop for ScreenResources {
    fn drop(&mut self) {
        unsafe { xrandr::XRRFreeScreenResources(self.ptr.as_ptr()) };
    }
}

#[derive(Debug)]
pub struct ScreenResources {
    ptr: ptr::NonNull<xrandr::XRRScreenResources>,
    pub timestamp: XTime,
    pub config_timestamp: XTime,
    pub ncrtc: i32,
    crtcs: Vec<XId>,
    pub outputs: Vec<XId>,
    pub nmode: i32,
    pub modes: Vec<Mode>,
}

impl ScreenResources {
    /// Create a handle to the `XRRScreenResources` object from libxrandr.
    /// This handle is used to query many parts of the current x11 config.
    ///
    /// # Errors
    /// * `XrandrError::GetResources` - Getting the handle failed.
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// let res = xrandr::ScreenResources::new(&mut xhandle).unwrap();
    /// ```
    ///
    pub fn new(handle: &mut XHandle) -> Result<ScreenResources, XrandrError> {
        // TODO: does this need to be freed?
        let raw_ptr = unsafe { xrandr::XRRGetScreenResources(handle.sys.as_ptr(), handle.root()) };
        let ptr = ptr::NonNull::new(raw_ptr).ok_or(XrandrError::GetResources)?;

        let xrandr::XRRScreenResources {
            modes,
            nmode,
            crtcs,
            ncrtc,
            outputs,
            noutput,
            timestamp,
            configTimestamp,
            ..
        } = unsafe { ptr.as_ref() };

        let x_modes: &[xrandr::XRRModeInfo] =
            unsafe { slice::from_raw_parts(*modes, *nmode as usize) };

        let modes: Vec<Mode> = x_modes.iter().map(Mode::from).collect();

        let x_crtcs = unsafe { slice::from_raw_parts(*crtcs, *ncrtc as usize) };

        let x_outputs = unsafe { slice::from_raw_parts(*outputs, *noutput as usize) };

        Ok(ScreenResources {
            ptr,
            timestamp: *timestamp,
            config_timestamp: *configTimestamp,
            ncrtc: *ncrtc,
            crtcs: x_crtcs.to_vec(),
            outputs: x_outputs.to_vec(),
            nmode: *nmode,
            modes,
        })
    }

    /// Gets information on all outputs
    ///
    /// # Errors
    /// * `XrandrError::GetOutputInfo(xid)`
    ///    -- Getting info failed for output xid
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// let res = xrandr::ScreenResources::new(&mut xhandle).unwrap();
    /// // All the outputs that are on this Crtc
    /// res.outputs(&mut xhandle);
    /// ```
    ///
    pub fn outputs(&self, handle: &mut XHandle) -> Result<Vec<Output>, XrandrError> {
        self.outputs
            .iter()
            .map(|xid| self.output(handle, *xid))
            .collect()
    }

    /// Gets information on output with given xid
    ///
    /// # Errors
    /// * `XrandrError::GetOutputInfo(xid)`
    ///    -- Getting info failed for output with XID `xid`
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// let res = xrandr::ScreenResources::new(&mut xhandle).unwrap();
    /// let output_id = res.outputs.get(0).unwrap();
    /// let output = res.output(&mut xhandle, *output_id).unwrap();
    /// ```
    ///
    pub fn output(&self, handle: &mut XHandle, xid: XId) -> Result<Output, XrandrError> {
        let raw_ptr =
            unsafe { xrandr::XRRGetOutputInfo(handle.sys.as_ptr(), self.ptr.as_ptr(), xid) };
        let ptr = ptr::NonNull::new(raw_ptr).ok_or(XrandrError::GetOutputInfo(xid))?;

        let output = Output::new(handle, self, unsafe { ptr.as_ref() }, xid);
        unsafe { xrandr::XRRFreeOutputInfo(ptr.as_ptr()) };

        output.map_err(|_| XrandrError::GetOutputInfo(xid))
    }

    /// Gets information on all crtcs
    ///
    /// # Errors
    /// * `XrandrError::GetCrtcInfo(xid)`
    ///    -- Getting info failed for crtc with XID `xid`
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// let res = xrandr::ScreenResources::new(&mut xhandle).unwrap();
    /// let crtcs = res.crtcs(&mut xhandle).unwrap();
    /// ```
    ///
    pub fn crtcs(&self, handle: &mut XHandle) -> Result<Vec<Crtc>, XrandrError> {
        self.crtcs
            .iter()
            .map(|xid| self.crtc(handle, *xid))
            .collect()
    }

    /// Gets information of only the enabled crtcs
    /// See also: `self.crtcs()`
    /// # Errors
    /// * `XrandrError::GetCrtcInfo(xid)`
    ///    -- Getting info failed for crtc with XID `xid`
    ///
    pub fn enabled_crtcs(&self, handle: &mut XHandle) -> Result<Vec<Crtc>, XrandrError> {
        Ok(self
            .crtcs(handle)?
            .into_iter()
            .filter(|c| c.mode != 0)
            .collect())
    }

    /// Gets information on crtc with given xid
    ///
    /// # Errors
    /// * `XrandrError::GetCrtcInfo(xid)`
    ///    -- Getting info failed for crtc with XID `xid`
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// // Get an enabled output
    /// let outputs = xhandle.all_outputs().unwrap();
    /// let output = outputs.iter().find(|o| o.current_mode.is_some()).unwrap();
    /// // Find information about its Crtc
    /// let output_crtc_id = output.crtc.unwrap();
    /// let res = xrandr::ScreenResources::new(&mut xhandle).unwrap();
    /// let crtc = res.crtc(&mut xhandle, output.crtc.unwrap());
    /// ```
    ///
    pub fn crtc(&self, handle: &mut XHandle, xid: XId) -> Result<Crtc, XrandrError> {
        let raw_ptr =
            unsafe { xrandr::XRRGetCrtcInfo(handle.sys.as_ptr(), self.ptr.as_ptr(), xid) };
        let ptr = ptr::NonNull::new(raw_ptr).ok_or(XrandrError::GetCrtcInfo(xid))?;

        let crtc = Crtc::new(unsafe { ptr.as_ref() }, xid);
        unsafe { xrandr::XRRFreeCrtcInfo(ptr.as_ptr()) };

        crtc.map_err(|_| XrandrError::GetCrtcInfo(xid))
    }

    /// Apply the fields set in `crtc`.
    /// # Examples
    /// ```
    /// // Changing the rotation of the Crtc of some Output:
    /// // This is an example and should really be done using set_rotation()
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// // Find enabled output
    /// let outputs = xhandle.all_outputs().unwrap();
    /// let output = outputs.iter().find(|o| o.current_mode.is_some()).unwrap();
    /// // Get its current Crtc information
    /// let mut res = xrandr::ScreenResources::new(&mut xhandle).unwrap();
    /// let crtc_id = output.crtc.unwrap();
    /// let mut crtc = res.crtc(&mut xhandle, crtc_id).unwrap();
    /// ```
    /// ```rust,ignore
    /// // Alter the mode field and apply
    /// crtc.mode = 0;
    /// res.set_crtc_config(&mut xhandle, &crtc);
    /// ```
    ///
    pub fn set_crtc_config(
        &self,
        handle: &mut XHandle,
        crtc: &mut Crtc,
    ) -> Result<(), XrandrError> {
        let outputs = match crtc.outputs.len() {
            0 => std::ptr::null_mut(),
            _ => crtc.outputs.as_mut_ptr(),
        };
        
        unsafe {
            xrandr::XRRSetCrtcConfig(
                handle.sys.as_ptr(),
                self.ptr.as_ptr(),
                crtc.xid,
                CURRENT_TIME,
                crtc.x,
                crtc.y,
                crtc.mode,
                crtc.rotation as u16,
                outputs,
                i32::try_from(crtc.outputs.len()).unwrap(),
            );
        }

        Ok(())
    }

    /// Gets information on all modes
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// let res = xrandr::ScreenResources::new(&mut xhandle).unwrap();
    /// let crtcs = res.modes();
    /// ```
    ///
    #[must_use]
    pub fn modes(&self) -> Vec<Mode> {
        self.modes.clone()
    }

    /// Gets information on mode with given xid
    ///
    /// # Errors
    /// * `XrandrError::GetModeInfo(xid)`
    ///    -- Getting info failed for mode with XID `xid`
    ///
    /// # Examples
    /// ```
    /// let mut xhandle = xrandr::XHandle::open().unwrap();
    /// // Get an enabled output
    /// let outputs = xhandle.all_outputs().unwrap();
    /// let output = outputs.iter().find(|o| o.current_mode.is_some()).unwrap();
    /// // Find its current mode
    /// let current_mode_id = output.current_mode.unwrap();
    /// let res = xrandr::ScreenResources::new(&mut xhandle).unwrap();
    /// let current_mode = res.mode(current_mode_id);
    /// ```
    ///
    pub fn mode(&self, xid: XId) -> Result<Mode, XrandrError> {
        self.modes
            .iter()
            .find(|c| c.xid == xid)
            .cloned()
            .ok_or(XrandrError::GetModeInfo(xid))
    }

    /// Applies some set of altered crtcs
    /// Due to xrandr's structure, changing one or more crtcs properly can be
    /// quite complicated. One should therefore call this function on any crtcs
    /// that you want to change.
    /// # Arguments
    /// * `changes`
    ///     Altered crtcs. Must be mutable because of crct.apply() calls.
    ///
    pub(crate) fn apply_new_crtcs(&self, handle: &mut XHandle, changed: &mut [Crtc]) -> Result<(), XrandrError> {
        let res = ScreenResources::new(handle)?;
        let old_crtcs = res.enabled_crtcs(handle)?;

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

        // To calculate the right screensize, we should make sure the 
        // mode-related fields are updated if the mode_id has changed
        for crtc in &mut new_crtcs {
            let mode = self.mode(crtc.mode)?;
            crtc.width = mode.width;
            crtc.height = mode.height;
        }

        // In case the top-left corner is no longer at (0,0), renormalize
        normalize_positions(&mut new_crtcs);
        let new_size = ScreenSize::fitting_crtcs(handle, &new_crtcs);

        // Disable crtcs that do not fit before setting the new size
        // Note that this should only be crtcs that were changed, but `changed`
        // contains the already altered crtc, so we have to use `old_crtcs`
        let mut old_crtcs = old_crtcs;
        for crtc in &mut old_crtcs {
            if !new_size.fits_crtc(crtc) {
                crtc.set_disable();
                res.set_crtc_config(handle, crtc)?;
            }
        }
        handle.set_screensize(&new_size);

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
            .try_for_each(|c| self.set_crtc_config(handle, c))
    }
}
