#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroPodError {
    BufferTooSmall,
    Overflow,
    InvalidData,
}

impl core::fmt::Display for ZeroPodError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BufferTooSmall => write!(f, "buffer too small"),
            Self::Overflow => write!(f, "field value exceeds max capacity"),
            Self::InvalidData => write!(f, "invalid data in buffer"),
        }
    }
}
