use {
    super::super::{
        resolve::{FieldSemantics, InitMode, PdaConstraint},
        syntax::SeedRenderContext,
    },
    quote::{format_ident, quote},
};

pub(super) fn require_ident(
    ident: Option<syn::Ident>,
    field: &syn::Ident,
    message: &str,
) -> syn::Result<syn::Ident> {
    ident.ok_or_else(|| syn::Error::new(field.span(), message))
}

pub(super) fn emit_init_stmts(
    semantics: &[FieldSemantics],
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let mut stmts = Vec::new();

    for sem in semantics {
        if sem.init.is_some() {
            stmts.push(emit_one_init(sem, semantics)?);
        }
    }

    Ok(stmts)
}

pub(super) fn emit_non_init_check(sem: &FieldSemantics) -> Option<proc_macro2::TokenStream> {
    let field = &sem.core.ident;

    if let Some(ac) = &sem.ata {
        let wallet = &ac.authority;
        let mint = &ac.mint;
        let token_program = token_program_expr(sem);
        return Some(quote! {
            quasar_spl::validate_ata(
                #field.to_account_view(),
                #wallet.to_account_view().address(),
                #mint.to_account_view().address(),
                #token_program,
            )?;
        });
    }

    super::params::emit_builtin_validate_params_for(sem, quote! { #field })
}

pub(super) fn token_program(sem: &FieldSemantics) -> Option<&syn::Ident> {
    sem.support.token_program.as_ref()
}

fn emit_one_init(
    sem: &FieldSemantics,
    all_semantics: &[FieldSemantics],
) -> syn::Result<proc_macro2::TokenStream> {
    let field = &sem.core.ident;
    let ty = &sem.core.effective_ty;
    let init = sem.init.as_ref().expect("checked by caller");
    let guard = matches!(init.mode, InitMode::InitIfNeeded);
    let payer = require_ident(
        sem.support.payer.clone(),
        field,
        "init requires a payer field",
    )?;

    let (signers_setup, signers_ref) = emit_signers(field, sem.pda.as_ref(), all_semantics);

    // Reject init_param:: on ATA fields — ATA init is direct codegen.
    if sem.ata.is_some() && !sem.params.init.is_empty() {
        return Err(syn::Error::new(
            sem.params.init[0].key.span(),
            "`init_param::` is not supported on associated_token fields (ATA init uses direct \
             codegen, not AccountInit)",
        ));
    }

    // ATA init stays as direct codegen — same type (Token) but different
    // CPI (Associated Token Program). Must check BEFORE the trait path.
    if let Some(ata_init) = emit_ata_init(sem, guard, &payer)? {
        return Ok(ata_init);
    }

    // --- Trait-based init path via BehaviorTarget ---
    let inner_ty = sem.core.inner_ty.as_ref().unwrap_or(&sem.core.effective_ty);
    let inner_base = crate::helpers::strip_generics(inner_ty);
    // SPL init impls (Token, Mint) handle space internally — their
    // init_account_with_rent calls use hardcoded LEN constants. Only
    // program-owned types need Space::SPACE from the macro.
    let has_spl_init = sem.token.is_some() || sem.mint.is_some();
    let space_expr = if let Some(space) = &init.space {
        quote! { (#space) as u64 }
    } else if has_spl_init {
        // SPL AccountInit::init() ignores ctx.space — pass 0.
        quote! { 0u64 }
    } else {
        quote! { <#inner_base as quasar_lang::traits::Space>::SPACE as u64 }
    };

    let init_param_assigns = super::params::emit_init_param_assigns(sem)?;

    let cpi_body = quote! {
        #signers_setup
        type __Target = <#ty as quasar_lang::account_load::AccountLoad>::BehaviorTarget;
        let mut __init_params =
            <__Target as quasar_lang::account_init::AccountInit>::InitParams::default();
        #init_param_assigns
        <__Target as quasar_lang::account_init::AccountInit>::init(
            quasar_lang::account_init::InitCtx {
                payer: #payer.to_account_view(),
                target: #field,
                program_id: __program_id,
                space: #space_expr,
                signers: #signers_ref,
                rent: &__shared_rent,
            },
            &__init_params,
        )?;
    };

    let validate = if guard {
        // init_if_needed existing-account branch: validate through the
        // wrapper's AccountLoad (handles both single-owner Account<T> and
        // multi-owner InterfaceAccount<T>), then run SPL/param validation.
        let field_name_str = field.to_string();

        // ATA is special: address-derivation validation, not account-type.
        let ata_validate = sem.ata.as_ref().map(|ac| {
            let wallet = &ac.authority;
            let mint = &ac.mint;
            let tp_expr = token_program_expr(sem);
            quote! {
                quasar_spl::validate_ata(
                    #field.to_account_view(),
                    #wallet.to_account_view().address(),
                    #mint.to_account_view().address(),
                    #tp_expr,
                )?;
            }
        });

        // Built-in SPL param validation (token::mint, mint::decimals, etc.)
        let spl_param_validate =
            super::params::emit_builtin_validate_params_for(sem, quote! { &__existing })
                .unwrap_or_default();

        // User param:: validation
        let user_param_validate =
            super::params::emit_validate_params_on(sem, quote! { &__existing });

        let ata_stmts = ata_validate.unwrap_or_default();
        let spl_stmts = spl_param_validate;
        let user_stmts = user_param_validate.unwrap_or_default();

        Some(quote! {
            // Load through wrapper's AccountLoad (correct for both
            // Account<T> and InterfaceAccount<T>).
            let __existing = <#ty as quasar_lang::account_load::AccountLoad>::load(
                #field, #field_name_str,
            )?;
            #ata_stmts
            #spl_stmts
            #user_stmts
        })
    } else {
        None
    };
    Ok(wrap_init_guard(field, guard, cpi_body, validate))
}

/// ATA init stays as direct codegen — the ATA program is a different
/// program than the account's owner, so AccountInit for Token cannot
/// distinguish token-account init from ATA init.
fn emit_ata_init(
    sem: &FieldSemantics,
    guard: bool,
    payer: &syn::Ident,
) -> syn::Result<Option<proc_macro2::TokenStream>> {
    let Some(ac) = &sem.ata else {
        return Ok(None);
    };
    let field = &sem.core.ident;
    let authority = &ac.authority;
    let mint = &ac.mint;
    let ata_program = require_ident(
        sem.support.associated_token_program.as_ref().cloned(),
        field,
        "#[account(init, associated_token::...)] requires an AssociatedTokenProgram field",
    )?;
    let token_program = require_ident(
        sem.support.token_program.as_ref().cloned(),
        field,
        "ATA init requires a token program field",
    )?;
    let system_program = require_ident(
        sem.support.system_program.as_ref().cloned(),
        field,
        "ATA init requires a System program field",
    )?;

    let cpi_body = quote! {
        quasar_spl::init_ata(
            #ata_program, #payer, #field, #authority, #mint,
            #system_program, #token_program, #guard,
        )?;
    };
    let validate = quote! {
        quasar_spl::validate_ata(
            #field.to_account_view(),
            #authority.to_account_view().address(),
            #mint.to_account_view().address(),
            #token_program.address(),
        )?;
    };
    Ok(Some(wrap_init_guard(
        field,
        guard,
        cpi_body,
        Some(validate),
    )))
}

fn emit_signers(
    field: &syn::Ident,
    pda: Option<&PdaConstraint>,
    all_semantics: &[FieldSemantics],
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let Some(pda) = pda else {
        return (quote! {}, quote! { &[] });
    };

    let bump_var = format_ident!("__bumps_{}", field);
    let bindings = super::parse::emit_seed_bindings(
        field,
        pda,
        all_semantics,
        SeedRenderContext::Init,
        "init_seed",
    );
    let seed_lets = bindings.seed_lets;
    let seed_idents = bindings.seed_idents;
    let seed_array_name = format_ident!("__init_seed_refs_{}", field);
    let explicit_bump_name = format_ident!("__init_bump_{}", field);
    let literal_seeds = super::parse::detect_literal_seeds(pda, all_semantics);
    let pda_assign = super::parse::emit_pda_bump_assignment(
        field,
        pda,
        &seed_idents,
        super::parse::PdaBumpAssignment {
            bump_var: &bump_var,
            addr_expr: &quote! { #field.address() },
            seed_array_name: &seed_array_name,
            explicit_bump_name: &explicit_bump_name,
            bare_mode: super::parse::PdaBareMode::DeriveExpected,
            log_failure: false,
            literal_seeds,
        },
    );

    (
        quote! {
            #(#seed_lets)*
            #pda_assign
            let __init_bump_ref: &[u8] = &[#bump_var];
            let __init_signer_seeds = [#(quasar_lang::cpi::Seed::from(#seed_idents),)* quasar_lang::cpi::Seed::from(__init_bump_ref)];
            let __init_signers = [quasar_lang::cpi::Signer::from(&__init_signer_seeds[..])];
        },
        quote! { &__init_signers },
    )
}

fn token_program_expr(sem: &FieldSemantics) -> syn::Expr {
    match token_program(sem) {
        Some(token_program) => syn::parse_quote!(#token_program.to_account_view().address()),
        None => syn::parse_quote!(&quasar_spl::SPL_TOKEN_ID),
    }
}

pub(super) fn wrap_init_guard(
    field: &syn::Ident,
    idempotent: bool,
    cpi_body: proc_macro2::TokenStream,
    validate_existing: Option<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    if idempotent {
        let validate = validate_existing.unwrap_or_default();
        quote! {
            {
                if quasar_lang::is_system_program(#field.owner()) {
                    #cpi_body
                } else {
                    #validate
                }
            }
        }
    } else {
        quote! {
            {
                if !quasar_lang::is_system_program(#field.owner()) {
                    return Err(ProgramError::AccountAlreadyInitialized);
                }
                #cpi_body
            }
        }
    }
}
