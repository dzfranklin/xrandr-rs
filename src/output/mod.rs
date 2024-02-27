pub mod property;

use crate::{ScreenResources, XHandle, XrandrError};
use indexmap::IndexMap;
use property::{Property, Value};
#[cfg(feature = "serialize")]
use serde::{Deserialize, Serialize};
use std::os::raw::c_int;
use std::slice;
use x11::xrandr::XRROutputInfo;
use x11::{xlib, xrandr};

use crate::XId;
use crate::XTime;
use crate::CURRENT_TIME;

#[derive(Debug)]
#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
pub struct Output {
    pub xid: XId,
    pub properties: IndexMap<String, Property>,
    pub timestamp: XTime,
    pub is_primary: bool,
    pub crtc: Option<XId>,
    pub name: String,
    pub mm_width: u64,
    pub mm_height: u64,
    pub connected: bool,
    pub subpixel_order: u16,
    pub crtcs: Vec<XId>,
    pub clones: Vec<XId>,
    pub modes: Vec<XId>,
    pub preferred_modes: Vec<XId>,
    pub current_mode: Option<XId>,
}

// TODO: replace this
// impl Drop for OutputHandle {
//     fn drop(&mut self) {
//         unsafe { xrandr::XRRFreeOutputInfo(self.ptr.as_ptr()) };
//     }
// }

impl Output {
    /// Get the Output's EDID property, if it exists.
    ///
    /// EDID stands for Extended Device Identification Data. You can parse it
    /// with a crate such as [edid][edid-crate] to get information such as the
    /// device model or colorspace.
    ///
    /// [edid-crate]: https://crates.io/crates/edid
    #[must_use]
    pub fn edid(&self) -> Option<Vec<u8>> {
        self.properties.get("EDID").map(|prop| match &prop.value {
            Value::Edid(edid) => edid.clone(),
            _ => unreachable!("Property with name EDID should have type edid"),
        })
    }

    // Requires resources because this currently resolves the current_mode
    // field to a fully owned object. Perhaps this should be done more lazily?
    pub(crate) fn new(
        handle: &mut XHandle,
        resources: &ScreenResources,
        output_info: &XRROutputInfo,
        xid: u64,
    ) -> Result<Self, XrandrError> {
        let xrandr::XRROutputInfo {
            crtc,
            ncrtc,
            crtcs,
            nclone,
            clones,
            nmode,
            npreferred,
            modes,
            name,
            nameLen,
            connection,
            mm_width,
            mm_height,
            subpixel_order,
            ..
        } = &output_info;

        let is_primary =
            xid == unsafe { xrandr::XRRGetOutputPrimary(handle.sys.as_ptr(), handle.root()) };

        let clones = unsafe { slice::from_raw_parts(*clones, *nclone as usize) };

        let modes = unsafe { slice::from_raw_parts(*modes, *nmode as usize) };
        let preferred_modes = modes[0..*npreferred as usize].to_vec();

        let crtcs = unsafe { slice::from_raw_parts(*crtcs, *ncrtc as usize) };
        let crtc_id = if *crtc == 0 { None } else { Some(*crtc) };
        let curr_crtc = crtc_id.and_then(|crtc_id| resources.crtc(handle, crtc_id).ok());
        let current_mode =
            curr_crtc.and_then(|crtc_info| modes.iter().copied().find(|&m| m == crtc_info.mode));

        let name_b = unsafe { slice::from_raw_parts(*name as *const u8, *nameLen as usize) };
        let name = String::from_utf8_lossy(name_b).to_string();
        let properties = Self::get_props(handle, xid)?;
        let connected = c_int::from(*connection) == xrandr::RR_Connected;

        let result = Self {
            xid,
            properties,
            timestamp: CURRENT_TIME,
            is_primary,
            crtc: crtc_id,
            name,
            mm_width: *mm_width,
            mm_height: *mm_height,
            connected,
            subpixel_order: *subpixel_order,
            crtcs: crtcs.to_vec(),
            clones: clones.to_vec(),
            modes: modes.to_vec(),
            preferred_modes,
            current_mode,
        };

        Ok(result)
    }

    fn get_props(
        handle: &mut XHandle,
        xid: xlib::XID,
    ) -> Result<IndexMap<String, Property>, XrandrError> {
        let mut props_len = 0;
        let props_data =
            unsafe { xrandr::XRRListOutputProperties(handle.sys.as_ptr(), xid, &mut props_len) };

        let props_slice = unsafe { slice::from_raw_parts(props_data, props_len as usize) };

        let props = props_slice
            .iter()
            .map(|prop_id| {
                let prop = Property::get(handle, xid, *prop_id)?;
                Ok((prop.name.clone(), prop))
            })
            .collect();

        unsafe { xlib::XFree(props_data.cast()) };

        props
    }

    pub(crate) unsafe fn from_list(
        handle: &mut XHandle,
        resources: &ScreenResources,
        data: *mut xrandr::RROutput,
        len: c_int,
    ) -> Result<Vec<Output>, XrandrError> {
        slice::from_raw_parts(data, len as usize)
            .iter()
            .map(|xid| resources.output(handle, *xid))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::XHandle;

    #[test]
    fn can_get_output_edid() {
        let mut handle = XHandle::open().unwrap();
        let outputs = handle.all_outputs().unwrap();
        let output = outputs.iter().find(|o| o.connected).unwrap();

        let edid = output.edid().unwrap();
        println!("{:?}", edid);
    }
}
