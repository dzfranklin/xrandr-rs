use crate::ScreenResources;
use crate::ScreenSize;
use crate::XId;
use crate::XTime;
use crate::CURRENT_TIME;
use crate::XHandle;
use crate::XrandrError;
use std::ptr;
use std::slice;

use x11::xrandr;
use std::convert::TryFrom;


// A Crtc can display a mode in one of 4 rotations
#[derive(PartialEq, Eq, Copy, Debug, Clone)]
pub enum Rotation {
    Normal = 1,
    Left = 2,
    Inverted = 4,
    Right = 8,
}

impl TryFrom<u16> for Rotation {
    type Error = XrandrError;

    fn try_from(r: u16) -> Result<Self, Self::Error> {
        match r {
            1 => Ok(Rotation::Normal),
            2 => Ok(Rotation::Left),
            4 => Ok(Rotation::Inverted),
            8 => Ok(Rotation::Right),
            _ => Err(XrandrError::InvalidRotation(r)),
        }
    }
}

// A Crtc can be positioned relative to another one in one of five directions
#[derive(Copy, Debug, Clone)]
pub enum Relation {
    LeftOf,
    RightOf,
    Above,
    Below,
    SameAs,
}


// Crtcs define a region of pixels you can see. The Crtc controls the size 
// and timing of the signal. To this end, the Crtc struct in xrandr maintains
// a list of attributes that usually correspond to a physical display.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Crtc {
    pub xid: XId,
    pub timestamp: XTime,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub mode: XId,
    pub rotation: Rotation,
    pub outputs: Vec<XId>,
    pub rotations: u16,
    pub possible: Vec<XId>,
}


/// Normalizes a set of Crtcs by making sure the top left pixel of the screen
/// is at (0,0). This is needed after changing positions/rotations.
pub(crate) fn normalize_positions(mut crtcs: Vec<&mut Crtc>) {
    if crtcs.is_empty() { return };

    let left = crtcs.iter().map(|p| p.x).min().unwrap();
    let top = crtcs.iter().map(|p| p.y).min().unwrap();
    if (top,left) == (0,0) { return };
    
    for c in &mut crtcs {
        c.offset((-left, -top));
    }
}

// A wrapper that drops the pointer if it goes out of scope.
// Avoid having to deal with the various early returns
struct CrtcInfo {
    pub ptr: ptr::NonNull<xrandr::XRRCrtcInfo>
}

impl CrtcInfo {
    fn new(handle: &mut XHandle, xid: XId) -> Result<Self, XrandrError> {
        let raw_ptr = unsafe { 
            xrandr::XRRGetCrtcInfo(handle.sys.as_ptr(), handle.res()?, xid)
        };

        let ptr = ptr::NonNull::new(raw_ptr)
            .ok_or(XrandrError::GetCrtcInfo(xid))?;

        Ok(Self { ptr })
    }
}

impl Drop for CrtcInfo {
    fn drop(&mut self) {
        unsafe { xrandr::XRRFreeCrtcInfo(self.ptr.as_ptr()) };
    }
}


impl Crtc {
    // TODO: better error documentation
    /// Open a handle to the lib-xrandr backend. This will be 
    /// used for nearly all interactions with the xrandr lib
    ///
    /// # Arguments
    /// * `handle` - The xhandle to make the x calls with
    /// * `xid` - The internal XID of the requested crtc
    ///
    /// # Errors
    /// * `XrandrError::GetCrtc(xid)` - Could not find this xid.
    ///
    /// # Examples
    /// ```
    /// let xhandle = XHandle.open()?;
    /// let mon1 = xhandle.monitors()?[0];
    /// ```
    ///
    pub fn from_xid(handle: &mut XHandle, xid: XId) 
    -> Result<Self,XrandrError>
    {
        // TODO: do the same as with outputs
        let crtc_info = CrtcInfo::new(handle, xid)?;

        let xrandr::XRRCrtcInfo {
            timestamp, x, y, width, height, mode, rotation, 
            noutput, outputs, rotations, npossible, possible
        } = unsafe { crtc_info.ptr.as_ref() };

        let rotation = Rotation::try_from(*rotation)?;

        let outputs = unsafe { 
            slice::from_raw_parts(*outputs, *noutput as usize) };

        let possible = unsafe { 
            slice::from_raw_parts(*possible, *npossible as usize) };

        Ok(Self {
            xid,
            timestamp: *timestamp,
            x: *x,
            y: *y,
            width: *width,
            height: *height,
            mode: *mode,
            rotation,
            outputs: outputs.to_vec(),
            rotations: *rotations,
            possible: possible.to_vec(),
        })
    }

    /// Apply the current fields of this crtc.
    /// # Examples
    /// ```
    /// // Sets new mode on the crtc of some output
    /// let mut crtc = ScreenResources::new(self)?.crtc(self, output.crtc)?;
    /// crtc.mode = mode.xid;
    /// crtc.apply(self)
    /// ```
    ///
    pub(crate) fn apply(&mut self, handle: &mut XHandle) 
    -> Result<(), XrandrError> 
    {
        let outputs = match self.outputs.len() {
            0 => std::ptr::null_mut(),
            _ => self.outputs.as_mut_ptr(),
        };

        unsafe {
            xrandr::XRRSetCrtcConfig(
                handle.sys.as_ptr(),
                handle.res()?,
                self.xid,
                CURRENT_TIME,
                self.x,
                self.y,
                self.mode,
                self.rotation as u16,
                outputs,
                self.outputs.len() as i32,
            );
        }

        Ok(())
    }

    /// Disable this crtc. Alters some of its fields.
    pub(crate) fn disable(&mut self, handle: &mut XHandle) 
    -> Result<(), XrandrError> 
    {
        self.x = 0;
        self.y = 0;
        self.mode = 0;
        self.rotation = Rotation::Normal;
        self.outputs.clear();

        self.apply(handle)
    }


    /// Width and height, accounting for a given rotation
    #[must_use] pub fn rotated_size(&self, rot: Rotation) -> (u32, u32) {
        let (w, h) = (self.width, self.height);

        let (old_w, old_h) = match self.rotation {
            Rotation::Normal | Rotation::Inverted   => (w, h),
            Rotation::Left | Rotation::Right        => (h, w),
        };

        match rot {
            Rotation::Normal | Rotation::Inverted   => (old_w, old_h),
            Rotation::Left | Rotation::Right        => (old_h, old_w),
        }
    }

    /// The most down an dright coordinates that this crtc uses
    pub(crate) fn max_coordinates(&self) -> (u32, u32) {
        assert!(self.x >= 0 && self.y >= 0,
            "max_coordinates should be called on normalized crtc");

        // let (w, h) = self.rot_size();
        // I think crtcs have this incorporated in their width/height fields
        (self.x as u32 + self.width, self.y as u32 + self.height)
    }

    /// Creates a new Crtc that is offset (.x and .y) fields, by offset param
    pub(crate) fn offset(&mut self, offset: (i32, i32)) {
        let x = self.x as i64 + offset.0 as i64;
        let y = self.y as i64 + offset.1 as i64;
        
        assert!(x < i32::MAX as i64 && y < i32::MAX as i64,
            "This offset would cause integer overflow");

        assert!(x >= 0 && y >= 0,
            "Invalid coordinates after offset");

        self.x = x as i32;
        self.y = y as i32;
    }
}

/// A Crtc change consists of a new and an old state
/// A change should be applied if these two differ
pub struct Change {
    pub old: Crtc,
    pub new: Crtc,
}

/// The changes struct keeps track of all crtcs and how they have changed
/// Its methods facilitate the changing of crtcs in the xrandr backend.
pub struct Changes {
    changes: Vec<Change>,
}

impl Changes {
    /// Generates a `changes` vector where `old` and `new` start of identical
    pub(crate) fn new(handle: &mut XHandle) -> Result<Self, XrandrError> {
        let res = ScreenResources::new(handle)?;
        let old_crtcs = res.enabled_crtcs(handle)?;

        let changes = old_crtcs.into_iter()
            .map(|c| Change { old: c.clone(), new: c })
            .collect::<Vec<Change>>();

        Ok(Self { changes })
    }

    /// Apply the differences made to the `new` crtcs
    pub(crate) fn apply(&mut self, handle: &mut XHandle) 
    -> Result<(), XrandrError> 
    {
        // Calculate a new screensize based on the new crtcs
        let new_crtcs = self.changes.iter()
            .map(|Change { old:_, new }| new)
            .collect::<Vec<&Crtc>>();
        let new_size = ScreenSize::fitting_crtcs2(handle, &new_crtcs);

        // Disable crtcs that do not fit on the new screen
        for Change { old, new:_ } in &mut self.changes {
            if !new_size.fits_crtc(old) { 
                eprintln!("Disabling:{:?}\n", old);
                old.disable(handle)?; 
            }
        }

        eprintln!("Settin screen size to: {:?}", new_size);
        new_size.set(handle); // Perform the resize xrandr call

        // Move/rotate and enable the crtcs
        for Change { old, new } in &mut self.changes {
            if new != old { 
                eprintln!("Applying:{:?}\n", new);
                new.apply(handle)?; 
            }
        }

        Ok(())
    }
    
    /// Get a reference to the new state of the crtc with id `xid`
    pub(crate) fn get_new(&mut self, xid: XId) 
    -> Result<&mut Crtc, XrandrError>
    {
        self.changes.iter_mut()
            .map(|Change { old:_, new }| new)
            .find(|crtc| crtc.xid == xid)
            .ok_or(XrandrError::GetCrtc(xid))
    }

    /// Get the refences to all the new states
    pub(crate) fn get_all_news(&mut self) -> Vec<&mut Crtc> {
        self.changes.iter_mut()
            .map(|Change { old:_, new }| new)
            .collect()
    }
}

