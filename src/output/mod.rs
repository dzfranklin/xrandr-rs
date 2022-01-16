pub mod property;

use crate::{XHandle, XrandrError};
use indexmap::IndexMap;
use property::{Property, PropertyValue};
#[cfg(feature = "serialize")]
use serde::{Deserialize, Serialize};
use std::os::raw::c_int;
use std::{ptr, slice};
use x11::{xlib, xrandr};

#[derive(Debug)]
#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
pub struct Output {
    pub xid: u64,
    pub name: String,
    /// Properties by name
    pub properties: IndexMap<String, Property>,
}

impl Output {
    /// Get the Output's EDID property, if it exists.
    ///
    /// EDID stands for Extended Device Identification Data. You can parse it
    /// with a crate such as [edid][edid-crate] to get information such as the
    /// device model or colorspace.
    ///
    /// [edid-crate]: https://crates.io/crates/edid
    pub fn edid(&self) -> Option<Vec<u8>> {
        self.properties.get("EDID").map(|prop| match &prop.value {
            PropertyValue::Edid(edid) => edid.clone(),
            _ => unreachable!("Property with name EDID can only be of type edid"),
        })
    }

    fn new(handle: &mut XHandle, xid: u64) -> Result<Self, XrandrError> {
        let info = unsafe {
            ptr::NonNull::new(xrandr::XRRGetOutputInfo(
                handle.sys.as_ptr(),
                handle.res()?,
                xid,
            ))
            .ok_or(XrandrError::GetOutputInfo(xid))?
            .as_ref()
        };

        let name = unsafe { slice::from_raw_parts(info.name as *const u8, info.nameLen as usize) };
        let name = String::from_utf8_lossy(name).to_string();

        let properties = Self::get_props(handle, xid)?;

        unsafe {
            xrandr::XRRFreeOutputInfo(info as *const _ as *mut _);
        }

        Ok(Self {
            xid,
            name,
            properties,
        })
    }

    fn get_props(
        handle: &mut XHandle,
        xid: xlib::XID,
    ) -> Result<IndexMap<String, Property>, XrandrError> {
        let mut props_len = 0;
        let props_data =
            unsafe { xrandr::XRRListOutputProperties(handle.sys.as_ptr(), xid, &mut props_len) };
        let props = unsafe { slice::from_raw_parts(props_data, props_len as usize) }
            .iter()
            .map(|prop_id| {
                let prop = Property::get(handle, xid, *prop_id)?;
                Ok((prop.name.clone(), prop))
            })
            .collect();

        // xrandr doesn't provide a function to free this. The other XRRFree* just call XFree,
        // so we do that ourselves
        unsafe {
            xlib::XFree(props_data as *mut _);
        }

        props
    }

    pub(crate) unsafe fn from_list(
        handle: &mut XHandle,
        data: *mut xrandr::RROutput,
        len: c_int,
    ) -> Result<Vec<Output>, XrandrError> {
        slice::from_raw_parts(data, len as usize)
            .iter()
            .map(|xid| Output::new(handle, *xid))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::XHandle;

    #[test]
    fn can_get_output_edid() {
        let outputs = XHandle::open().unwrap().all_outputs().unwrap();
        let output = outputs.first().unwrap();
        let edid = output.edid().unwrap();
        println!("{:?}", edid);
    }
}
