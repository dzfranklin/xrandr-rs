use std::slice;
use x11::xrandr;

use crate::XHandle;
use crate::output::Output;
use crate::Mode;
use crate::crtc::Crtc;
use crate::XrandrError;

use crate::XId;
use crate::XTime;


#[derive(Debug)]
// TODO: which IDs are possibly useful for the end user?
// make only those public (outside of crate)
pub struct ScreenResources {
    pub timestamp: XTime,
    pub config_timestamp: XTime,
    pub ncrtc: i32,
    crtcs: Vec<XId>,
    pub outputs: Vec<XId>,
    pub nmode: i32,
    pub modes: Vec<Mode>,
}


impl ScreenResources {
    // TODO: better error documentation
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
    pub fn new(handle: &mut XHandle) 
    -> Result<ScreenResources, XrandrError> 
    {
        // TODO: does this need to be freed?
        let res = handle.res()?;

        let x_modes: &[xrandr::XRRModeInfo] = unsafe { 
            slice::from_raw_parts(res.modes, res.nmode as usize) };

        let modes: Vec<Mode> = x_modes.iter()
            .map(Mode::from)
            .collect();

        let x_crtcs = unsafe { 
            slice::from_raw_parts(res.crtcs, res.ncrtc as usize) };

        let x_outputs = unsafe { 
            slice::from_raw_parts(res.outputs, res.noutput as usize) };

        Ok(ScreenResources { 
            timestamp: res.timestamp,
            config_timestamp: res.configTimestamp,
            ncrtc: res.ncrtc,
            crtcs: x_crtcs.to_vec(),
            outputs: x_outputs.to_vec(),
            nmode: res.nmode,
            modes
        })
    }

    // TODO: better error documentation
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
    pub fn outputs(&self, handle: &mut XHandle)
    -> Result<Vec<Output>, XrandrError> 
    {
        self.outputs.iter()
            .map(|xid| Output::from_xid(handle, *xid))
            .collect()
    }

    // TODO: better error documentation
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
    pub fn output(&self, handle: &mut XHandle, xid: XId)
    -> Result<Output, XrandrError> 
    {
        self.outputs(handle)?.into_iter()
            .find(|o| o.xid == xid)
            .ok_or(XrandrError::GetOutputInfo(xid))
    }

    // TODO: better error documentation
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
    pub fn crtcs(&self, handle: &mut XHandle) 
    -> Result<Vec<Crtc>, XrandrError> 
    {
        self.crtcs.iter()
            .map(|xid| Crtc::from_xid(handle, *xid))
            .collect()
    }

    /// Gets information of only the enabled crtcs
    /// See also: `self.crtcs()`
    /// # Errors
    /// * `XrandrError::GetCrtcInfo(xid)`
    ///    -- Getting info failed for crtc with XID `xid`
    ///
    pub fn enabled_crtcs(&self, handle: &mut XHandle) 
    -> Result<Vec<Crtc>, XrandrError> 
    {
        Ok(self.crtcs(handle)?.into_iter()
            .filter(|c| c.mode != 0)
            .collect())
    }

    // TODO: better error documentation
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
    pub fn crtc(&self, handle: &mut XHandle, xid: XId) 
    -> Result<Crtc, XrandrError> 
    {
        self.crtcs(handle)?.into_iter()
            .find(|c| c.xid == xid)
            .ok_or(XrandrError::GetCrtc(xid))
    }


    // TODO: better error documentation
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
    #[must_use] pub fn modes(&self) -> Vec<Mode> {
        self.modes.clone()
    }

    // TODO: better error documentation
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
        self.modes.iter()
            .find(|c| c.xid == xid)
            .cloned()
            .ok_or(XrandrError::GetModeInfo(xid))
    }

}

