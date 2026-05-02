//! Phased codegen — emitting from typed execution plan.
//!
//! Generated parse body shape:
//!
//! ```text
//! // Rent (only when init/realloc/migration needs it)
//! let __rent: Rent = Sysvar::get()?;  // or deserialized from Sysvar<Rent> field
//! let __rent_ctx = OpCtxWithRent::new(&program_id, &__rent);
//!
//! // Phase 1: load non-init fields
//! let field_a = <Ty>::load(field_a, "field_a")?;
//! let mut field_b = <Ty>::load_mut(field_b, "field_b")?;
//!
//! // Phase 2: address verify + init CPI for init fields
//! init_op.apply::<Ty>(slot, &__rent_ctx)?;
//!
//! // Phase 3: load init fields
//! let field_c = <Ty>::load(field_c, "field_c")?;
//!
//! // Phase 4: checks, realloc, migration grow, user checks
//! <Ty as TokenCheck>::check_token_view(view, ctx)?;
//!
//! Ok((Self { field_a, field_b, field_c }, bumps))
//! ```

use {
    super::{
        super::resolve::{
            specs::{AccountsPlanTyped, EpilogueStep, PostLoadStep, PreLoadStep, RentPlan},
            FieldKind, FieldSemantics, UserCheck,
        },
        typed_emit,
    },
    crate::helpers::strip_generics,
    quote::{format_ident, quote},
};

pub(crate) fn emit_parse_body(
    semantics: &[FieldSemantics],
    plan: &AccountsPlanTyped,
    cx: &super::EmitCx,
) -> syn::Result<proc_macro2::TokenStream> {
    // Rent context from typed plan.
    let ctx_init = emit_rent_context(&plan.rent, semantics);
    let bump_vars = emit_bump_vars(semantics);

    // Phase 1: load non-init fields.
    let load_non_init = emit_load_filtered(semantics, false);
    // Phase 2: address verify + init CPI (from typed plan).
    let init_phase = emit_init_phase_typed(&plan.fields, semantics)?;
    // Phase 3: load init fields.
    let load_init = emit_load_filtered(semantics, true);
    // Phase 4: post-load steps (checks, realloc, migration grow, address verify,
    // user checks).
    let phase4 = emit_post_load_typed(&plan.fields, semantics);
    let bump_init = emit_bump_init(semantics, &cx.bumps_name);

    let construct_fields: Vec<proc_macro2::TokenStream> = semantics
        .iter()
        .map(|sem| {
            let ident = &sem.core.ident;
            quote! { #ident }
        })
        .collect();

    Ok(quote! {
        #bump_vars
        #ctx_init
        #(#load_non_init)*
        #(#init_phase)*
        #(#load_init)*
        #(#phase4)*
        Ok((Self { #(#construct_fields,)* }, #bump_init))
    })
}

// ==== Rent context ====

fn emit_rent_context(
    rent_plan: &RentPlan,
    _semantics: &[FieldSemantics],
) -> proc_macro2::TokenStream {
    match rent_plan {
        RentPlan::NotNeeded => quote! {},
        RentPlan::FromSysvarField { field } => {
            quote! {
                let __rent: quasar_lang::sysvars::rent::Rent = unsafe {
                    core::clone::Clone::clone(
                        <quasar_lang::sysvars::rent::Rent as quasar_lang::sysvars::Sysvar>::from_bytes_unchecked(
                            #field.borrow_unchecked()
                        )
                    )
                };
                let __rent_ctx = quasar_lang::ops::OpCtxWithRent::new(
                    unsafe { &*(__program_id as *const quasar_lang::prelude::Address) },
                    &__rent,
                );
            }
        }
        RentPlan::FetchOnce => {
            quote! {
                let __rent: quasar_lang::sysvars::rent::Rent =
                    <quasar_lang::sysvars::rent::Rent as quasar_lang::sysvars::Sysvar>::get()?;
                let __rent_ctx = quasar_lang::ops::OpCtxWithRent::new(
                    unsafe { &*(__program_id as *const quasar_lang::prelude::Address) },
                    &__rent,
                );
            }
        }
    }
}

// ==== Init phase (from typed plan) ====

fn emit_init_phase_typed(
    field_plans: &[super::super::resolve::specs::FieldPlan],
    semantics: &[FieldSemantics],
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let mut stmts = Vec::new();

    for (fp, sem) in field_plans.iter().zip(semantics.iter()) {
        let ident = &sem.core.ident;
        let ty = &sem.core.effective_ty;

        for step in &fp.pre_load {
            match step {
                PreLoadStep::VerifyAddress(addr_spec) => {
                    let bump_var = format_ident!("__bumps_{}", ident);
                    let addr_var = format_ident!("__addr_{}", ident);
                    let addr_expr = &addr_spec.expr;
                    stmts.push(quote! {
                        let #addr_var = #addr_expr;
                        #bump_var = quasar_lang::address::AddressVerify::verify(
                            &#addr_var, #ident.address(), __program_id,
                        )?;
                    });
                }
                PreLoadStep::Init(init_plan) => {
                    let has_address = sem.address.is_some();
                    let ts = typed_emit::emit_init_plan(init_plan, ident, ty, has_address);
                    stmts.push(ts);
                }
            }
        }
    }

    Ok(stmts)
}

// ==== Post-load phase (from typed plan) ====

fn emit_post_load_typed(
    field_plans: &[super::super::resolve::specs::FieldPlan],
    semantics: &[FieldSemantics],
) -> Vec<proc_macro2::TokenStream> {
    let mut stmts = Vec::new();

    for (fp, sem) in field_plans.iter().zip(semantics.iter()) {
        let ident = &sem.core.ident;
        let ty = &sem.core.effective_ty;
        let is_optional = sem.core.optional;

        for step in &fp.post_load {
            let call = match step {
                PostLoadStep::TokenCheck(spec) => typed_emit::emit_token_check(spec, ident, ty),
                PostLoadStep::MintCheck(spec) => typed_emit::emit_mint_check(spec, ident, ty),
                PostLoadStep::AssociatedTokenCheck(spec) => {
                    typed_emit::emit_associated_token_check(spec, ident, ty)
                }
                PostLoadStep::Realloc(spec) => {
                    let payer_ident = &spec.payer.ident;
                    let realloc_expr = &spec.new_space;
                    quote! {
                        {
                            let __realloc_op = quasar_lang::ops::realloc::Op {
                                space: (#realloc_expr) as usize,
                                payer: #payer_ident.to_account_view(),
                            };
                            __realloc_op.apply::<#ty>(&mut #ident, &__rent_ctx)?;
                        }
                    }
                }
                PostLoadStep::MigrationGrow(spec) => {
                    let payer_ident = &spec.payer.ident;
                    quote! {
                        quasar_lang::accounts::migration::Migration::grow_to_target(
                            &mut #ident,
                            #payer_ident.to_account_view(),
                            &__rent_ctx,
                        )?;
                    }
                }
                PostLoadStep::VerifyExistingAddress(addr_spec) => {
                    let bump_var = format_ident!("__bumps_{}", ident);
                    let addr_expr = &addr_spec.expr;
                    let use_fast_path = is_validated_account_type(ty);
                    let verify_method = if use_fast_path {
                        quote! { verify_existing }
                    } else {
                        quote! { verify }
                    };
                    quote! {
                        {
                            let __addr = #addr_expr;
                            #bump_var = quasar_lang::address::AddressVerify::#verify_method(
                                &__addr, #ident.to_account_view().address(), __program_id,
                            )?;
                        }
                    }
                }
            };

            let needs_mut = matches!(
                step,
                PostLoadStep::Realloc(_) | PostLoadStep::MigrationGrow(_)
            );
            stmts.push(wrap_optional(is_optional, ident, &call, needs_mut));
        }

        // User checks (still sourced from semantics — they're structural, not op-group
        // based).
        for check in &sem.user_checks {
            let check_stmts = emit_user_check(sem, check);
            let combined = quote! { #(#check_stmts)* };
            stmts.push(wrap_optional(is_optional, ident, &combined, false));
        }
    }

    stmts
}

// ==== Epilogue (from typed plan) ====

pub(crate) fn emit_epilogue(
    semantics: &[FieldSemantics],
    plan: &AccountsPlanTyped,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut exit_stmts = Vec::new();
    let needs_lifecycle_rent = plan.fields.iter().any(|fp| {
        fp.epilogue
            .iter()
            .any(|step| matches!(step, EpilogueStep::MigrationVerifyAndNormalize(_)))
    });

    for (fp, sem) in plan.fields.iter().zip(semantics.iter()) {
        let ident = &sem.core.ident;
        let ty = &sem.core.effective_ty;

        for step in &fp.epilogue {
            let stmt = match step {
                EpilogueStep::TokenSweep(spec) => typed_emit::emit_token_sweep(spec, ident, ty),
                EpilogueStep::TokenClose(spec) => typed_emit::emit_token_close(spec, ident, ty),
                EpilogueStep::ProgramClose(spec) => typed_emit::emit_program_close(spec, ident, ty),
                EpilogueStep::MigrationVerifyAndNormalize(spec) => {
                    let payer_ident = &spec.payer.ident;
                    quote! {
                        if !quasar_lang::accounts::migration::Migration::is_migrated(&self.#ident) {
                            return Err(ProgramError::Custom(
                                quasar_lang::error::QuasarError::AccountNotMigrated as u32,
                            ));
                        }
                        quasar_lang::accounts::migration::Migration::normalize_to_target(
                            &mut self.#ident,
                            self.#payer_ident.to_account_view(),
                            &__rent_ctx,
                        )?;
                    }
                }
            };
            exit_stmts.push(stmt);
        }
    }

    if exit_stmts.is_empty() {
        return Ok(quote! {});
    }

    let ctx_init = if needs_lifecycle_rent {
        let rent_field = match &plan.rent {
            RentPlan::FromSysvarField { field } => Some(field.clone()),
            _ => None,
        };
        let rent_fetch = if let Some(rent_ident) = rent_field {
            quote! {
                let __rent: quasar_lang::sysvars::rent::Rent =
                    core::clone::Clone::clone(self.#rent_ident.get());
            }
        } else {
            quote! {
                let __rent: quasar_lang::sysvars::rent::Rent =
                    <quasar_lang::sysvars::rent::Rent as quasar_lang::sysvars::Sysvar>::get()?;
            }
        };
        quote! {
            #rent_fetch
            let __rent_ctx = quasar_lang::ops::OpCtxWithRent::new(&crate::ID, &__rent);
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        #[inline(always)]
        fn epilogue(&mut self) -> Result<(), ProgramError> {
            #ctx_init
            #(#exit_stmts)*
            Ok(())
        }
    })
}

pub(crate) fn emit_has_epilogue_typed(plan: &AccountsPlanTyped) -> proc_macro2::TokenStream {
    let has_epilogue = plan.fields.iter().any(|fp| !fp.epilogue.is_empty());
    if has_epilogue {
        quote! { true }
    } else {
        quote! { false }
    }
}

// ==== Load phase ====

fn emit_load_filtered(
    semantics: &[FieldSemantics],
    init_only: bool,
) -> Vec<proc_macro2::TokenStream> {
    semantics
        .iter()
        .filter(|sem| sem.core.kind == FieldKind::Single)
        .filter(|sem| sem.has_init() == init_only)
        .map(emit_one_load)
        .collect()
}

fn emit_one_load(sem: &FieldSemantics) -> proc_macro2::TokenStream {
    let ident = &sem.core.ident;
    let ty = &sem.core.effective_ty;
    let field_name_str = ident.to_string();

    if sem.core.dynamic {
        let inner_ty = sem.core.inner_ty.as_ref().unwrap_or(ty);
        let base = strip_generics(inner_ty);
        return quote! { let #ident = #base::from_account_view(#ident)?; };
    }

    if sem.core.optional {
        return if sem.core.is_mut {
            quote! {
                let mut #ident = if quasar_lang::keys_eq(#ident.address(), __program_id) {
                    None
                } else {
                    Some(<#ty as quasar_lang::account_load::AccountLoad>::load_mut(#ident, #field_name_str)?)
                };
            }
        } else {
            quote! {
                let #ident = if quasar_lang::keys_eq(#ident.address(), __program_id) {
                    None
                } else {
                    Some(<#ty as quasar_lang::account_load::AccountLoad>::load(#ident, #field_name_str)?)
                };
            }
        };
    }

    if sem.core.is_mut {
        quote! {
            let mut #ident = <#ty as quasar_lang::account_load::AccountLoad>::load_mut(#ident, #field_name_str)?;
        }
    } else {
        quote! {
            let #ident = <#ty as quasar_lang::account_load::AccountLoad>::load(#ident, #field_name_str)?;
        }
    }
}

// ==== User checks (structural — not op-group based) ====

fn emit_user_check(sem: &FieldSemantics, check: &UserCheck) -> Vec<proc_macro2::TokenStream> {
    let field_ident = &sem.core.ident;
    let mut stmts = Vec::new();

    match check {
        UserCheck::HasOne { targets, error } => {
            let err = match error {
                Some(e) => quote! { #e.into() },
                None => quote! { QuasarError::HasOneMismatch.into() },
            };
            for target in targets {
                stmts.push(quote! {
                    quasar_lang::validation::check_address_match(
                        &#field_ident.#target,
                        #target.to_account_view().address(),
                        #err,
                    )?;
                });
            }
        }
        UserCheck::Address { expr, error } => {
            let err = match error {
                Some(e) => quote! { #e.into() },
                None => quote! { QuasarError::AddressMismatch.into() },
            };
            stmts.push(quote! {
                quasar_lang::validation::check_address_match(
                    #field_ident.to_account_view().address(),
                    &#expr,
                    #err,
                )?;
            });
        }
        UserCheck::Constraints { exprs, error } => {
            let err = match error {
                Some(e) => quote! { #e.into() },
                None => quote! { QuasarError::ConstraintViolation.into() },
            };
            for expr in exprs {
                stmts.push(quote! {
                    quasar_lang::validation::check_constraint(#expr, #err)?;
                });
            }
        }
    }

    stmts
}

// ==== Helpers ====

/// Wrap a code block in `if let Some(ref field) = field { ... }` for optional
/// accounts.
fn wrap_optional(
    is_optional: bool,
    ident: &syn::Ident,
    body: &proc_macro2::TokenStream,
    needs_mut: bool,
) -> proc_macro2::TokenStream {
    if !is_optional {
        return body.clone();
    }
    if needs_mut {
        quote! {
            if let Some(ref mut #ident) = #ident {
                #body
            }
        }
    } else {
        quote! {
            if let Some(ref #ident) = #ident {
                #body
            }
        }
    }
}

fn emit_bump_vars(semantics: &[FieldSemantics]) -> proc_macro2::TokenStream {
    let vars: Vec<proc_macro2::TokenStream> = semantics
        .iter()
        .filter(|sem| sem.address.is_some())
        .map(|sem| {
            let var = format_ident!("__bumps_{}", sem.core.ident);
            quote! { let mut #var: u8 = 0; }
        })
        .collect();

    quote! { #(#vars)* }
}

fn emit_bump_init(
    semantics: &[FieldSemantics],
    bumps_name: &syn::Ident,
) -> proc_macro2::TokenStream {
    let inits: Vec<proc_macro2::TokenStream> = semantics
        .iter()
        .filter(|sem| sem.address.is_some())
        .flat_map(|sem| {
            let name = &sem.core.ident;
            let var = format_ident!("__bumps_{}", name);
            let arr_name = format_ident!("__{}_bump", name);
            vec![quote! { #name: #var }, quote! { #arr_name: [#var] }]
        })
        .collect();

    if inits.is_empty() {
        quote! { #bumps_name }
    } else {
        quote! { #bumps_name { #(#inits,)* } }
    }
}

pub(crate) fn emit_bump_struct_def(
    semantics: &[FieldSemantics],
    cx: &super::EmitCx,
) -> proc_macro2::TokenStream {
    let bumps_name = &cx.bumps_name;
    let fields: Vec<proc_macro2::TokenStream> = semantics
        .iter()
        .filter(|sem| sem.address.is_some())
        .flat_map(|sem| {
            let name = &sem.core.ident;
            let arr_name = format_ident!("__{}_bump", name);
            vec![quote! { pub #name: u8 }, quote! { pub #arr_name: [u8; 1] }]
        })
        .collect();

    if fields.is_empty() {
        quote! { #[derive(Copy, Clone)] pub struct #bumps_name; }
    } else {
        quote! { #[derive(Copy, Clone)] pub struct #bumps_name { #(#fields,)* } }
    }
}

/// Returns true for account types with owner + discriminator validation.
fn is_validated_account_type(ty: &syn::Type) -> bool {
    use crate::helpers::extract_generic_inner_type;
    extract_generic_inner_type(ty, "Account").is_some()
        || extract_generic_inner_type(ty, "InterfaceAccount").is_some()
        || extract_generic_inner_type(ty, "Migration").is_some()
}
