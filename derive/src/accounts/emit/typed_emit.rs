//! Emit code from typed execution plan specs.
//!
//! Each function takes a completed spec and produces a TokenStream.
//! No arg lookup, no string filtering, no validation — the planner did that.

use {
    super::super::resolve::specs::*,
    quote::{format_ident, quote},
};

// ---------------------------------------------------------------------------
// Check emit
// ---------------------------------------------------------------------------

pub(crate) fn emit_token_check(
    spec: &TokenCheckSpec,
    field_ident: &syn::Ident,
    field_ty: &syn::Type,
) -> proc_macro2::TokenStream {
    let spl_crate = format_ident!("quasar_{}", "spl");
    let mint = emit_account_view_ref(&spec.mint);
    let authority = emit_account_view_ref(&spec.authority);
    let token_program = emit_token_program_check(&spec.token_program);

    quote! {
        <#field_ty as #spl_crate::ops::capabilities::TokenCheck>::check_token_view(
            #field_ident.to_account_view(),
            #spl_crate::ops::ctx::TokenCheckCtx {
                mint: #mint,
                authority: #authority,
                token_program: #token_program,
            },
        )?;
    }
}

pub(crate) fn emit_mint_check(
    spec: &MintCheckSpec,
    field_ident: &syn::Ident,
    field_ty: &syn::Type,
) -> proc_macro2::TokenStream {
    let spl_crate = format_ident!("quasar_{}", "spl");
    let authority = emit_account_view_ref(&spec.authority);
    let decimals = emit_check_mode_u8(&spec.decimals);
    let freeze_authority = emit_freeze_authority_check(&spec.freeze_authority);
    let token_program = emit_token_program_check(&spec.token_program);

    quote! {
        <#field_ty as #spl_crate::ops::capabilities::MintCheck>::check_mint_view(
            #field_ident.to_account_view(),
            #spl_crate::ops::ctx::MintCheckCtx {
                authority: #authority,
                decimals: #decimals,
                freeze_authority: #freeze_authority,
                token_program: #token_program,
            },
        )?;
    }
}

pub(crate) fn emit_associated_token_check(
    spec: &AssociatedTokenCheckSpec,
    field_ident: &syn::Ident,
    field_ty: &syn::Type,
) -> proc_macro2::TokenStream {
    let spl_crate = format_ident!("quasar_{}", "spl");
    let mint = emit_account_view_ref(&spec.mint);
    let authority = emit_account_view_ref(&spec.authority);
    let token_program = emit_token_program_check(&spec.token_program);

    quote! {
        <#field_ty as #spl_crate::ops::capabilities::AssociatedTokenCheck>::check_associated_token_view(
            #field_ident.to_account_view(),
            #spl_crate::ops::ctx::AssociatedTokenCheckCtx {
                mint: #mint,
                authority: #authority,
                token_program: #token_program,
            },
        )?;
    }
}

// ---------------------------------------------------------------------------
// Init emit
// ---------------------------------------------------------------------------

pub(crate) fn emit_init_plan(
    init: &InitPlan,
    field_ident: &syn::Ident,
    field_ty: &syn::Type,
    has_address: bool,
) -> proc_macro2::TokenStream {
    let spl_crate = format_ident!("quasar_{}", "spl");

    let (params_block, space, idempotent) = match init {
        InitPlan::Program(spec) => {
            let space = match &spec.space {
                SpaceSpec::FromType(ty) => quote! {
                    <#ty as quasar_lang::traits::Space>::SPACE as u64
                },
            };
            (quote! { let __init_params = (); }, space, spec.idempotent)
        }
        InitPlan::Token(spec) => {
            let mint = emit_account_view_ref(&spec.mint);
            let authority = emit_account_view_ref(&spec.authority);
            let token_program = emit_program_ref_view(&spec.token_program);
            let kind_ty = format_ident!("Token{}Kind", "Init");
            let params = quote! {
                let __init_params = #spl_crate::#kind_ty::Token {
                    mint: #mint,
                    authority: #authority.address(),
                    token_program: #token_program,
                };
            };
            (params, quote! { 0u64 }, spec.idempotent)
        }
        InitPlan::Mint(spec) => {
            let decimals_val = spec.decimals.value();
            let authority = emit_account_view_ref(&spec.authority);
            let token_program = emit_program_ref_view(&spec.token_program);
            let freeze_authority = emit_freeze_authority_init(&spec.freeze_authority);
            let params_ty = format_ident!("Mint{}Params", "Init");
            let params = quote! {
                let __init_params = #spl_crate::#params_ty {
                    decimals: #decimals_val,
                    authority: #authority.address(),
                    freeze_authority: #freeze_authority,
                    token_program: #token_program,
                };
            };
            (params, quote! { 0u64 }, spec.idempotent)
        }
        InitPlan::AssociatedToken(spec) => {
            let mint = emit_account_view_ref(&spec.mint);
            let authority = emit_account_view_ref(&spec.authority);
            let token_program = emit_program_ref_view(&spec.token_program);
            let system_program = emit_program_ref_view(&spec.system_program);
            let ata_program = emit_program_ref_view(&spec.ata_program);
            let idempotent = spec.idempotent;
            let kind_ty = format_ident!("Token{}Kind", "Init");
            let params = quote! {
                let __init_params = #spl_crate::#kind_ty::AssociatedToken {
                    mint: #mint,
                    authority: #authority,
                    token_program: #token_program,
                    system_program: #system_program,
                    ata_program: #ata_program,
                    idempotent: #idempotent,
                };
            };
            (params, quote! { 0u64 }, spec.idempotent)
        }
    };

    let payer_ref = match init {
        InitPlan::Program(s) => &s.payer,
        InitPlan::Token(s) => &s.payer,
        InitPlan::Mint(s) => &s.payer,
        InitPlan::AssociatedToken(s) => &s.payer,
    };
    let payer = emit_field_ref_view(payer_ref);

    let init_call = quote! {
        let __init_op = quasar_lang::ops::init::Op {
            payer: #payer,
            space: #space,
            signers: __signers,
            params: __init_params,
            idempotent: #idempotent,
        };
        __init_op.apply::<#field_ty>(#field_ident, &__rent_ctx)?;
    };

    let inner_body = quote! {
        #params_block
        #init_call
    };

    let body = if has_address {
        let bump_var = format_ident!("__bumps_{}", field_ident);
        let addr_var = format_ident!("__addr_{}", field_ident);
        quote! {
            let __bump_ref: &[u8] = &[#bump_var];
            quasar_lang::address::AddressVerify::with_signer_seeds(
                &#addr_var,
                __bump_ref,
                |__maybe_signer| -> Result<(), quasar_lang::prelude::ProgramError> {
                    let __signers = match &__maybe_signer {
                        Some(__signer) => core::slice::from_ref(__signer),
                        None => &[] as &[quasar_lang::cpi::Signer<'_, '_>],
                    };
                    #inner_body
                    Ok(())
                },
            )?;
        }
    } else {
        quote! {
            let __signers: &[quasar_lang::cpi::Signer<'_, '_>] = &[];
            #inner_body
        }
    };

    if idempotent {
        quote! {
            if quasar_lang::is_system_program(#field_ident.owner()) {
                #body
            }
        }
    } else {
        quote! { { #body } }
    }
}

// ---------------------------------------------------------------------------
// Exit emit
// ---------------------------------------------------------------------------

pub(crate) fn emit_program_close(
    spec: &ProgramCloseSpec,
    field_ident: &syn::Ident,
    field_ty: &syn::Type,
) -> proc_macro2::TokenStream {
    let dest = emit_exit_account_view_ref(&spec.destination);
    let trait_name = format_ident!("{}Close", "Account");
    quote! {
        {
            let __view = unsafe {
                <#field_ty as quasar_lang::account_load::AccountLoad>::to_account_view_mut(
                    &mut self.#field_ident
                )
            };
            <#field_ty as quasar_lang::ops::close::#trait_name>::close(
                __view,
                #dest,
            )?;
        }
    }
}

pub(crate) fn emit_token_close(
    spec: &TokenCloseSpec,
    field_ident: &syn::Ident,
    field_ty: &syn::Type,
) -> proc_macro2::TokenStream {
    let spl_crate = format_ident!("quasar_{}", "spl");
    let dest = emit_exit_account_view_ref(&spec.destination);
    let authority = emit_exit_account_view_ref(&spec.authority);
    let token_program = emit_exit_program_ref(&spec.token_program);
    let trait_name = format_ident!("Token{}", "Close");
    quote! {
        {
            let __view = unsafe {
                <#field_ty as quasar_lang::account_load::AccountLoad>::to_account_view_mut(
                    &mut self.#field_ident
                )
            };
            <#field_ty as #spl_crate::ops::close::#trait_name>::close(
                __view,
                #dest,
                #authority,
                #token_program,
            )?;
        }
    }
}

pub(crate) fn emit_token_sweep(
    spec: &TokenSweepSpec,
    field_ident: &syn::Ident,
    field_ty: &syn::Type,
) -> proc_macro2::TokenStream {
    let spl_crate = format_ident!("quasar_{}", "spl");
    let receiver = emit_exit_account_view_ref(&spec.receiver);
    let mint = emit_exit_account_view_ref(&spec.mint);
    let authority = emit_exit_account_view_ref(&spec.authority);
    let token_program = emit_exit_program_ref(&spec.token_program);
    let trait_name = format_ident!("Token{}", "Sweep");
    quote! {
        <#field_ty as #spl_crate::ops::sweep::#trait_name>::sweep(
            self.#field_ident.to_account_view(),
            #receiver,
            #mint,
            #authority,
            #token_program,
        )?;
    }
}

// ---------------------------------------------------------------------------
// Helpers: arg → TokenStream
// ---------------------------------------------------------------------------

/// Emit a FieldRef as `field.to_account_view()` (guaranteed field).
fn emit_field_ref_view(field_ref: &FieldRef) -> proc_macro2::TokenStream {
    let ident = &field_ref.ident;
    quote! { #ident.to_account_view() }
}

/// Emit an AccountRef as `field.to_account_view()` (post-load phase context).
fn emit_account_view_ref(account_ref: &AccountRef) -> proc_macro2::TokenStream {
    match account_ref.arg_ref() {
        ArgRef::Field(ident) => quote! { #ident.to_account_view() },
        ArgRef::Expr(expr) => quote! { #expr },
    }
}

/// Emit an AccountRef for exit phase: `self.field.to_account_view()`.
fn emit_exit_account_view_ref(account_ref: &AccountRef) -> proc_macro2::TokenStream {
    match account_ref.arg_ref() {
        ArgRef::Field(ident) => quote! { self.#ident.to_account_view() },
        ArgRef::Expr(expr) => quote! { #expr },
    }
}

/// Emit a ProgramRef as an account view (post-load or init context).
fn emit_program_ref_view(program_ref: &ProgramRef) -> proc_macro2::TokenStream {
    emit_field_ref_view(program_ref)
}

/// Emit a ProgramRef for exit phase.
fn emit_exit_program_ref(program_ref: &ProgramRef) -> proc_macro2::TokenStream {
    let ident = &program_ref.ident;
    quote! { self.#ident.to_account_view() }
}

/// Emit token_program for check context: Some(field.to_account_view()) or None.
fn emit_token_program_check(check_ref: &TokenProgramCheckRef) -> proc_macro2::TokenStream {
    match check_ref {
        TokenProgramCheckRef::ConcreteOwner => quote! { None },
        TokenProgramCheckRef::RuntimeField(ident) => {
            quote! { Some(#ident.to_account_view()) }
        }
    }
}

/// Emit decimals CheckMode as Option<u8>.
fn emit_check_mode_u8(mode: &CheckMode<syn::Expr>) -> proc_macro2::TokenStream {
    match mode {
        CheckMode::DoNotCheck => quote! { None },
        CheckMode::Check(expr) => quote! { Some(#expr) },
    }
}

/// Emit freeze_authority for check context: FreezeAuthorityCheck enum.
fn emit_freeze_authority_check(mode: &CheckMode<FreezeAuthoritySpec>) -> proc_macro2::TokenStream {
    let spl_crate = format_ident!("quasar_{}", "spl");
    match mode {
        CheckMode::DoNotCheck => {
            quote! { #spl_crate::ops::ctx::FreezeAuthorityCheck::Skip }
        }
        CheckMode::Check(FreezeAuthoritySpec::None) => {
            quote! { #spl_crate::ops::ctx::FreezeAuthorityCheck::AssertNone }
        }
        CheckMode::Check(FreezeAuthoritySpec::Some(account_ref)) => {
            let view = emit_account_view_ref(account_ref);
            quote! { #spl_crate::ops::ctx::FreezeAuthorityCheck::AssertEquals(#view) }
        }
    }
}

/// Emit freeze_authority for init context: Option<&Address>.
fn emit_freeze_authority_init(
    mode: &MaybeDefault<FreezeAuthoritySpec>,
) -> proc_macro2::TokenStream {
    match mode.value() {
        FreezeAuthoritySpec::None => quote! { None },
        FreezeAuthoritySpec::Some(account_ref) => {
            let view = emit_account_view_ref(account_ref);
            quote! { Some(#view.address()) }
        }
    }
}
