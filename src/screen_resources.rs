use std::{ptr, slice};
use x11::xrandr;

use crate::CURRENT_TIME;
use crate::XHandle;
use crate::output::Output;
use crate::Mode;
use crate::crtc::Crtc;
use crate::XrandrError;

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
    /// let xhandle = XHandle.open()?;
    /// let res = ScreenResources::new(&mut xhandle)?;
    /// let crtc_87 = res.crtc(&mut xhandle, 87);
    /// ```
    ///
    pub fn new(handle: &mut XHandle) -> Result<ScreenResources, XrandrError> {
        // TODO: does this need to be freed?
        // let res = ScreenResourcesHandle::new(handle)?;
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
    /// let res = ScreenResources::new(&mut xhandle)?;
    /// let outputs = res.outputs(&mut xhandle);
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
    /// let res = ScreenResources::new(&mut xhandle)?;
    /// let output_89 = res.output(&mut xhandle, 89);
    /// ```
    ///
    pub fn output(&self, handle: &mut XHandle, xid: XId) -> Result<Output, XrandrError> {
        let raw_ptr = unsafe { xrandr::XRRGetOutputInfo(handle.sys.as_ptr(), self.ptr.as_ptr(), xid) };
        let ptr = ptr::NonNull::new(raw_ptr).ok_or(XrandrError::GetOutputInfo(xid))?;

        let output = Output::new(handle, unsafe { ptr.as_ref() }, xid);
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
    /// let res = ScreenResources::new(&mut xhandle)?;
    /// let crtcs = res.crtcs(&mut xhandle);
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
    /// let res = ScreenResources::new(&mut xhandle)?;
    /// let current_crtc = res.crtc(&mut xhandle, output.crtc);
    /// ```
    ///
    pub fn crtc(&self, handle: &mut XHandle, xid: XId) -> Result<Crtc, XrandrError> {
        let raw_ptr = unsafe { xrandr::XRRGetCrtcInfo(handle.sys.as_ptr(), self.ptr.as_ptr(), xid) };
        let ptr = ptr::NonNull::new(raw_ptr).ok_or(XrandrError::GetCrtcInfo(xid))?;

        let crtc = Crtc::new(unsafe { ptr.as_ref() }, xid);
        unsafe { xrandr::XRRFreeCrtcInfo(ptr.as_ptr()) };

        crtc.map_err(|_| XrandrError::GetCrtcInfo(xid))
    }

    /// Apply the fields set in `crtc`.
    /// # Examples
    /// ```
    /// // Changing the mode of the Crtc of some Output:
    /// let res = ScreenResources::new(&mut xhandle)?;
    /// let mut crtc = res.crtc(&mut xhandle, output.crtc)?;
    /// crtc.mode = some_mode.xid;
    /// res.set_crtc_config(xhandle, &crtc);
    /// ```
    ///
    pub fn set_crtc_config(&mut self, handle: &mut XHandle, crtc: &Crtc) -> Result<(), XrandrError> {
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

    /// Gets information on all crtcs
    ///
    /// # Errors
    /// * `XrandrError::GetCrtcInfo(xid)`
    ///    -- Getting info failed for crtc with XID `xid`
    ///
    /// # Examples
    /// ```
    /// let res = ScreenResources::new(&mut xhandle)?;
    /// let crtcs = res.crtcs(&mut xhandle);
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
    /// let res = ScreenResources::new(&mut xhandle)?;
    /// let current_mode = res.mode(&mut xhandle, output.mode);
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
