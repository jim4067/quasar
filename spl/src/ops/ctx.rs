//! Context structs for capability trait methods.
//!
//! These provide the public input surface for capability traits. The derive
//! constructs them directly when emitting validation and init-contributor
//! calls.

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

/// Context for associated token address + token data validation.
pub struct AssociatedTokenCheckCtx<'a> {
    pub mint: &'a AccountView,
    pub authority: &'a AccountView,
    pub token_program: &'a AccountView,
}
