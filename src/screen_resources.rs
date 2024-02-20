use std::{ptr, slice};
use x11::xrandr;

use crate::crtc::Crtc;
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
        &mut self,
        handle: &mut XHandle,
        crtc: &Crtc,
    ) -> Result<(), XrandrError> {
        let outputs = match self.outputs.len() {
            0 => std::ptr::null_mut(),
            _ => self.outputs.as_mut_ptr(),
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
                i32::try_from(self.outputs.len()).unwrap(),
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
}
