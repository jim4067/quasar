//! SPL token operations.
//!
//! Capability traits (`capabilities`) and context structs (`ctx`) are the
//! public dispatch surface. The derive emits direct capability trait calls.

pub mod capabilities;
pub mod close;
pub mod ctx;
pub mod sweep;
