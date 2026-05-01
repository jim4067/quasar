//! Context structs for capability trait methods.
//!
//! These provide a stable public trait surface even if Op struct internals
//! change. Op structs are derive intermediaries; context structs are the
//! public API of capability traits.

use quasar_lang::prelude::AccountView;

/// Context for token account validation.
pub struct TokenCheckCtx<'a> {
    pub mint: &'a AccountView,
    pub authority: &'a AccountView,
    pub token_program: &'a AccountView,
}

/// Context for mint account validation.
pub struct MintCheckCtx<'a> {
    pub decimals: u8,
    pub authority: &'a AccountView,
    pub freeze_authority: Option<&'a AccountView>,
    pub token_program: &'a AccountView,
}

/// Context for ATA address + token data validation.
pub struct AtaCheckCtx<'a> {
    pub mint: &'a AccountView,
    pub authority: &'a AccountView,
    pub token_program: &'a AccountView,
}

/// Context for token account closing.
pub struct TokenCloseCtx<'a> {
    pub dest: &'a AccountView,
    pub authority: &'a AccountView,
    pub token_program: &'a AccountView,
}

/// Context for token sweep (transfer all tokens out before close).
pub struct TokenSweepCtx<'a> {
    pub receiver: &'a AccountView,
    pub mint: &'a AccountView,
    pub authority: &'a AccountView,
    pub token_program: &'a AccountView,
}

/// Context for token account init param contribution.
pub struct TokenInitCtx<'a> {
    pub mint: &'a AccountView,
    pub authority: &'a AccountView,
    pub token_program: &'a AccountView,
}

/// Context for mint account init param contribution.
pub struct MintInitCtx<'a> {
    pub decimals: u8,
    pub authority: &'a AccountView,
    pub freeze_authority: Option<&'a AccountView>,
    pub token_program: &'a AccountView,
}

/// Context for ATA init param contribution.
pub struct AtaInitCtx<'a> {
    pub authority: &'a AccountView,
    pub mint: &'a AccountView,
    pub payer: &'a AccountView,
    pub token_program: &'a AccountView,
    pub system_program: &'a AccountView,
    pub ata_program: &'a AccountView,
    pub idempotent: bool,
}
