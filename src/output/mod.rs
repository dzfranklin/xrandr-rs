pub mod property;

use crate::{XHandle, XrandrError};
use indexmap::IndexMap;
use property::Property;
use std::os::raw::c_int;
use std::{ptr, slice};
use x11::{xlib, xrandr};

#[derive(Debug)]
pub struct Output {
    pub xid: u64,
    pub name: String,
    /// Properties by name
    pub properties: IndexMap<String, Property>,
}

impl Output {
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
        unsafe { slice::from_raw_parts(props_data, props_len as usize) }
            .iter()
            .map(|prop_id| {
                let prop = Property::get(handle, xid, *prop_id)?;
                Ok((prop.name.clone(), prop))
            })
            .collect()
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
