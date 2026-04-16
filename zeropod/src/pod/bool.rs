use core::fmt;

#[repr(transparent)]
#[derive(Copy, Clone, Default)]
pub struct PodBool([u8; 1]);

impl PodBool {
    /// Returns the contained [`bool`] value. Any non-zero byte yields `true`.
    #[inline(always)]
    pub fn get(&self) -> bool {
        self.0[0] != 0
    }
}

impl From<bool> for PodBool {
    #[inline(always)]
    fn from(v: bool) -> Self {
        Self([v as u8])
    }
}

impl From<PodBool> for bool {
    #[inline(always)]
    fn from(v: PodBool) -> Self {
        v.get()
    }
}

impl PartialEq for PodBool {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.get() == other.get()
    }
}
impl Eq for PodBool {}

impl PartialEq<bool> for PodBool {
    #[inline(always)]
    fn eq(&self, other: &bool) -> bool {
        self.get() == *other
    }
}

impl core::ops::Not for PodBool {
    type Output = Self;
    #[inline(always)]
    fn not(self) -> Self {
        Self::from(!self.get())
    }
}

impl fmt::Display for PodBool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl fmt::Debug for PodBool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PodBool({})", self.get())
    }
}

const _: () = assert!(core::mem::align_of::<PodBool>() == 1);
const _: () = assert!(core::mem::size_of::<PodBool>() == 1);
