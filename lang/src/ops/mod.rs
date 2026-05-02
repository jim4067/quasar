//! Op-dispatch for account lifecycle operations.
//!
//! The derive emits direct capability trait calls for validation, init
//! contribution, and exit actions. Structural ops (init, realloc, PDA
//! verification) use their own inherent methods.
//!
//! `OpCtxWithRent` carries instruction-scoped state (program_id + &Rent).
//! The derive emits this when any field uses init, realloc, or migration.

pub mod close;
pub mod init;
pub mod realloc;

/// Context with rent: program_id + pre-fetched Rent.
///
/// The derive emits this when any field uses init, realloc, or migration.
/// Rent is populated exactly once at instruction entry — either deserialized
/// from a `Sysvar<Rent>` field or fetched via `Rent::get()` syscall.
pub struct OpCtxWithRent<'a> {
    pub program_id: &'a solana_address::Address,
    pub rent: &'a crate::sysvars::rent::Rent,
}

impl<'a> OpCtxWithRent<'a> {
    #[inline(always)]
    pub fn new(
        program_id: &'a solana_address::Address,
        rent: &'a crate::sysvars::rent::Rent,
    ) -> Self {
        Self { program_id, rent }
    }
}

/// Marker trait for account types that support realloc.
///
/// The `realloc::Op` requires `F: SupportsRealloc` to ensure only
/// realloc-capable accounts are used with `realloc(...)`.
pub trait SupportsRealloc {}
