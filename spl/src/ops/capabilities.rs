//! Capability traits — the public dispatch surface for SPL token operations.
//!
//! Each trait represents a single capability (check, init contribution, close,
//! sweep). Layout-backed blanket impls wire types with the correct schema to
//! the appropriate validation logic.
//!
//! All trait method impls use `#[inline(always)]` — critical for sBPF CU parity.
//! Missing this annotation costs 100-200 CU per non-inlined call.

use {
    super::ctx::{
        AtaCheckCtx, AtaInitCtx, MintCheckCtx, MintInitCtx, TokenCheckCtx, TokenInitCtx,
    },
    quasar_lang::{account_layout::AccountLayout, prelude::*},
};

// ---------------------------------------------------------------------------
// Check capabilities
// ---------------------------------------------------------------------------

/// Capability: validate a token account (mint, authority, program).
pub trait TokenCheck: AsAccountView {
    fn check_token_view(view: &AccountView, ctx: TokenCheckCtx<'_>) -> Result<(), ProgramError>;
}

/// Capability: validate a mint account (authority, decimals, freeze authority).
pub trait MintCheck: AsAccountView {
    fn check_mint_view(view: &AccountView, ctx: MintCheckCtx<'_>) -> Result<(), ProgramError>;
}

/// Capability: validate an ATA (address derivation + token data).
pub trait AtaCheck: AsAccountView {
    fn check_ata_view(view: &AccountView, ctx: AtaCheckCtx<'_>) -> Result<(), ProgramError>;
}

// ---------------------------------------------------------------------------
// Init contributor capabilities
// ---------------------------------------------------------------------------

/// Capability: contribute token init params.
pub trait TokenInitContributor: quasar_lang::account_init::AccountInit {
    fn apply_token_init<'a>(
        params: &mut Self::InitParams<'a>,
        ctx: TokenInitCtx<'a>,
    ) -> Result<(), ProgramError>;
}

/// Capability: contribute mint init params.
pub trait MintInitContributor: quasar_lang::account_init::AccountInit {
    fn apply_mint_init<'a>(
        params: &mut Self::InitParams<'a>,
        ctx: MintInitCtx<'a>,
    ) -> Result<(), ProgramError>;
}

/// Capability: contribute ATA init params.
pub trait AtaInitContributor: quasar_lang::account_init::AccountInit {
    fn apply_ata_init<'a>(
        params: &mut Self::InitParams<'a>,
        ctx: AtaInitCtx<'a>,
    ) -> Result<(), ProgramError>;
}

// ---------------------------------------------------------------------------
// Layout-backed blanket impls — check capabilities
// ---------------------------------------------------------------------------

impl<T> TokenCheck for T
where
    T: AsAccountView + AccountLayout<Schema = crate::token::TokenData>,
{
    #[inline(always)]
    fn check_token_view(view: &AccountView, ctx: TokenCheckCtx<'_>) -> Result<(), ProgramError> {
        crate::validate::validate_token_account(
            view,
            ctx.mint.address(),
            ctx.authority.address(),
            ctx.token_program.address(),
        )
    }
}

impl<T> MintCheck for T
where
    T: AsAccountView + AccountLayout<Schema = crate::token::MintData>,
{
    #[inline(always)]
    fn check_mint_view(view: &AccountView, ctx: MintCheckCtx<'_>) -> Result<(), ProgramError> {
        crate::validate::validate_mint(
            view,
            ctx.authority.address(),
            ctx.decimals,
            ctx.freeze_authority.map(|fa| fa.address()),
            ctx.token_program.address(),
        )
    }
}

/// ATA validation: token-layout validation + ATA address derivation check.
impl<T> AtaCheck for T
where
    T: AsAccountView + AccountLayout<Schema = crate::token::TokenData>,
{
    #[inline(always)]
    fn check_ata_view(view: &AccountView, ctx: AtaCheckCtx<'_>) -> Result<(), ProgramError> {
        crate::validate::validate_ata(
            view,
            ctx.authority.address(),
            ctx.mint.address(),
            ctx.token_program.address(),
        )
    }
}

// ---------------------------------------------------------------------------
// Init contributor impls — Token types
// ---------------------------------------------------------------------------

impl TokenInitContributor for crate::token::Token {
    #[inline(always)]
    fn apply_token_init<'a>(
        params: &mut Self::InitParams<'a>,
        ctx: TokenInitCtx<'a>,
    ) -> Result<(), ProgramError> {
        if params.kind.is_some() {
            return Err(ProgramError::InvalidArgument);
        }
        params.kind = Some(crate::token::TokenInitKind::Token {
            mint: ctx.mint,
            authority: ctx.authority.address(),
            token_program: ctx.token_program,
        });
        Ok(())
    }
}

impl TokenInitContributor for crate::token_2022::Token2022 {
    #[inline(always)]
    fn apply_token_init<'a>(
        params: &mut Self::InitParams<'a>,
        ctx: TokenInitCtx<'a>,
    ) -> Result<(), ProgramError> {
        if params.kind.is_some() {
            return Err(ProgramError::InvalidArgument);
        }
        params.kind = Some(crate::token::TokenInitKind::Token {
            mint: ctx.mint,
            authority: ctx.authority.address(),
            token_program: ctx.token_program,
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Init contributor impls — Mint types
// ---------------------------------------------------------------------------

impl MintInitContributor for crate::token::Mint {
    #[inline(always)]
    fn apply_mint_init<'a>(
        params: &mut Self::InitParams<'a>,
        ctx: MintInitCtx<'a>,
    ) -> Result<(), ProgramError> {
        params.decimals = Some(ctx.decimals);
        params.authority = Some(ctx.authority.address());
        params.freeze_authority = ctx.freeze_authority.map(|fa| fa.address());
        params.token_program = Some(ctx.token_program);
        Ok(())
    }
}

impl MintInitContributor for crate::token_2022::Mint2022 {
    #[inline(always)]
    fn apply_mint_init<'a>(
        params: &mut Self::InitParams<'a>,
        ctx: MintInitCtx<'a>,
    ) -> Result<(), ProgramError> {
        params.decimals = Some(ctx.decimals);
        params.authority = Some(ctx.authority.address());
        params.freeze_authority = ctx.freeze_authority.map(|fa| fa.address());
        params.token_program = Some(ctx.token_program);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Init contributor impls — ATA types (uses TokenInitParams with ATA kind)
// ---------------------------------------------------------------------------

impl AtaInitContributor for crate::token::Token {
    #[inline(always)]
    fn apply_ata_init<'a>(
        params: &mut Self::InitParams<'a>,
        ctx: AtaInitCtx<'a>,
    ) -> Result<(), ProgramError> {
        if params.kind.is_some() {
            return Err(ProgramError::InvalidArgument);
        }
        params.kind = Some(crate::token::TokenInitKind::AssociatedToken {
            mint: ctx.mint,
            authority: ctx.authority,
            token_program: ctx.token_program,
            system_program: ctx.system_program,
            ata_program: ctx.ata_program,
            idempotent: ctx.idempotent,
        });
        Ok(())
    }
}

impl AtaInitContributor for crate::token_2022::Token2022 {
    #[inline(always)]
    fn apply_ata_init<'a>(
        params: &mut Self::InitParams<'a>,
        ctx: AtaInitCtx<'a>,
    ) -> Result<(), ProgramError> {
        if params.kind.is_some() {
            return Err(ProgramError::InvalidArgument);
        }
        params.kind = Some(crate::token::TokenInitKind::AssociatedToken {
            mint: ctx.mint,
            authority: ctx.authority,
            token_program: ctx.token_program,
            system_program: ctx.system_program,
            ata_program: ctx.ata_program,
            idempotent: ctx.idempotent,
        });
        Ok(())
    }
}
