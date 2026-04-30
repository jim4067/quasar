use {
    super::{
        super::syntax::{classify_seed, lower_bump, parse_field_attrs, AccountDirective},
        rules::validate_semantics,
        support::resolve_supports,
        FieldCore, FieldParams, FieldSemantics, FieldShape, FieldSupport, InitConstraint, InitMode,
        LifecycleConstraint, MintConstraint, ParamAssign, PdaConstraint, PdaSource,
        ReallocConstraint, SeedNode, TokenConstraint, UserCheckConstraint, UserCheckKind,
    },
    crate::helpers::{extract_generic_inner_type, is_composite_type},
    syn::{Expr, Ident, Type},
};

pub(super) fn lower_semantics(
    fields: &syn::punctuated::Punctuated<syn::Field, syn::token::Comma>,
    instruction_args: &Option<Vec<crate::accounts::InstructionArg>>,
) -> syn::Result<Vec<FieldSemantics>> {
    let parsed: Vec<(syn::Field, Vec<AccountDirective>)> = fields
        .iter()
        .map(|field| Ok((field.clone(), parse_field_attrs(field)?)))
        .collect::<syn::Result<_>>()?;

    let field_names: Vec<String> = parsed
        .iter()
        .map(|(field, _)| field.ident.as_ref().expect("named field").to_string())
        .collect();

    let cores: Vec<FieldCore> = parsed
        .iter()
        .map(|(field, directives)| lower_core(field, directives))
        .collect();

    let field_types: Vec<(Ident, Type)> = cores
        .iter()
        .map(|c| (c.ident.clone(), c.effective_ty.clone()))
        .collect();

    let mut semantics: Vec<FieldSemantics> = parsed
        .into_iter()
        .zip(cores)
        .map(|((_, directives), core)| {
            let mut sem = FieldSemantics {
                core,
                support: FieldSupport::default(),
                params: FieldParams::default(),
                init: None,
                pda: None,
                token: None,
                ata: None,
                mint: None,
                realloc: None,
                lifecycle: Vec::new(),
                user_checks: Vec::new(),
            };
            lower_constraints(
                &mut sem,
                directives,
                &field_names,
                &field_types,
                instruction_args,
            );
            Ok(sem)
        })
        .collect::<syn::Result<_>>()?;

    resolve_supports(&mut semantics)?;
    validate_semantics(&semantics)?;

    Ok(semantics)
}

fn lower_core(field: &syn::Field, directives: &[AccountDirective]) -> FieldCore {
    let ty = &field.ty;
    let optional = extract_generic_inner_type(ty, "Option").is_some();
    let after_option = extract_generic_inner_type(ty, "Option")
        .cloned()
        .unwrap_or_else(|| ty.clone());

    let effective_ty = match &after_option {
        Type::Reference(r) => (*r.elem).clone(),
        other => other.clone(),
    };

    let (shape, raw_inner_ty) = classify_shape(&effective_ty, ty);
    let inner_name = raw_inner_ty.as_ref().and_then(type_base_name).cloned();
    let is_token_account = inner_name_matches(&inner_name, &["Token", "Token2022"]);
    let is_mint = inner_name_matches(&inner_name, &["Mint", "Mint2022"]);
    let is_token_or_mint = is_token_account || is_mint;
    let supports_existing_pda_fast_path = matches!(shape, FieldShape::Account)
        || (matches!(shape, FieldShape::InterfaceAccount) && is_token_or_mint);
    let inner_ty = if matches!(
        shape,
        FieldShape::Account | FieldShape::InterfaceAccount | FieldShape::Migration
    ) {
        raw_inner_ty
    } else {
        None
    };
    let dynamic = detect_dynamic(shape, inner_ty.as_ref());

    let is_migration = matches!(shape, FieldShape::Migration);

    FieldCore {
        ident: field
            .ident
            .clone()
            .expect("account field must have an identifier"),
        field: field.clone(),
        effective_ty,
        shape,
        inner_ty,
        inner_name,
        is_token_account,
        is_mint,
        is_token_or_mint,
        supports_existing_pda_fast_path,
        optional,
        dynamic,
        // Migration fields are implicitly mutable (realloc + write target).
        is_mut: is_migration
            || directives
                .iter()
                .any(|d| matches!(d, AccountDirective::Mut)),
        dup: directives
            .iter()
            .any(|d| matches!(d, AccountDirective::Dup)),
    }
}

fn classify_shape(effective_ty: &Type, raw_ty: &Type) -> (FieldShape, Option<Type>) {
    if is_composite_type(raw_ty) {
        return (FieldShape::Composite, None);
    }

    // Migration<From, To> — must check before Account<T> since both are
    // generic wrappers. The `From` type is used for source-data access.
    if let Some(from_ty) = extract_migration_from_type(effective_ty) {
        return (FieldShape::Migration, Some(from_ty));
    }

    if let Some(inner) = extract_generic_inner_type(effective_ty, "Account") {
        return (FieldShape::Account, Some(inner.clone()));
    }
    if let Some(inner) = extract_generic_inner_type(effective_ty, "InterfaceAccount") {
        return (FieldShape::InterfaceAccount, Some(inner.clone()));
    }
    if let Some(inner) = extract_generic_inner_type(effective_ty, "Program") {
        return (FieldShape::Program, Some(inner.clone()));
    }
    if let Some(inner) = extract_generic_inner_type(effective_ty, "Interface") {
        return (FieldShape::Interface, Some(inner.clone()));
    }
    if let Some(inner) = extract_generic_inner_type(effective_ty, "Sysvar") {
        return (FieldShape::Sysvar, Some(inner.clone()));
    }

    let shape = match type_base_name(effective_ty) {
        Some(ident) if ident == "SystemAccount" => FieldShape::SystemAccount,
        Some(ident) if ident == "Signer" => FieldShape::Signer,
        _ => FieldShape::Other,
    };
    (shape, None)
}

/// Extract the `From` type param from `Migration<From, To>`.
fn extract_migration_from_type(ty: &Type) -> Option<Type> {
    if let Type::Path(type_path) = ty {
        if let Some(last) = type_path.path.segments.last() {
            if last.ident == "Migration" {
                if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                    let mut iter = args.args.iter();
                    if let (
                        Some(syn::GenericArgument::Type(from)),
                        Some(syn::GenericArgument::Type(_to)),
                    ) = (iter.next(), iter.next())
                    {
                        return Some(from.clone());
                    }
                }
            }
        }
    }
    None
}

fn type_base_name(ty: &Type) -> Option<&syn::Ident> {
    match ty {
        Type::Path(tp) => tp.path.segments.last().map(|s| &s.ident),
        _ => None,
    }
}

fn inner_name_matches(inner_name: &Option<Ident>, names: &[&str]) -> bool {
    inner_name
        .as_ref()
        .is_some_and(|ident| names.iter().any(|name| ident == *name))
}

fn detect_dynamic(shape: FieldShape, inner_ty: Option<&Type>) -> bool {
    if !matches!(shape, FieldShape::Account) {
        return false;
    }
    let Some(inner) = inner_ty else {
        return false;
    };
    if let Type::Path(tp) = inner {
        if let Some(last) = tp.path.segments.last() {
            if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                return args
                    .args
                    .iter()
                    .any(|arg| matches!(arg, syn::GenericArgument::Lifetime(_)));
            }
        }
    }
    false
}

fn lower_constraints(
    sem: &mut FieldSemantics,
    directives: Vec<AccountDirective>,
    field_names: &[String],
    field_types: &[(Ident, Type)],
    instruction_args: &Option<Vec<crate::accounts::InstructionArg>>,
) {
    let mut init_mode = None;
    let mut payer = None;
    let mut space = None;
    let mut bump = None;
    let mut token_mint = None;
    let mut token_authority = None;
    let mut token_token_program = None;
    let mut ata_mint = None;
    let mut ata_authority = None;
    let mut ata_token_program = None;
    let mut mint_decimals = None;
    let mut mint_authority = None;
    let mut mint_freeze_authority = None;
    let mut mint_token_program = None;
    let mut realloc = None;
    let mut realloc_payer = None;

    for directive in directives {
        match directive {
            AccountDirective::Mut | AccountDirective::Dup => {}
            AccountDirective::Init => init_mode = Some(InitMode::Init),
            AccountDirective::InitIfNeeded => init_mode = Some(InitMode::InitIfNeeded),
            AccountDirective::Close(destination) => {
                sem.lifecycle
                    .push(LifecycleConstraint::Close { destination });
            }
            AccountDirective::Sweep(receiver) => {
                sem.lifecycle.push(LifecycleConstraint::Sweep { receiver });
            }
            AccountDirective::Payer(v) => payer = Some(v),
            AccountDirective::Space(v) => space = Some(v),
            AccountDirective::HasOne(target, error) => sem.user_checks.push(UserCheckConstraint {
                kind: UserCheckKind::HasOne { target },
                error,
            }),
            AccountDirective::Constraint(expr, error) => {
                sem.user_checks.push(UserCheckConstraint {
                    kind: UserCheckKind::Constraint { expr },
                    error,
                })
            }
            AccountDirective::Address(expr, error) => sem.user_checks.push(UserCheckConstraint {
                kind: UserCheckKind::Address { expr },
                error,
            }),
            AccountDirective::Seeds(seed_exprs) => {
                let seeds =
                    lower_seed_nodes(seed_exprs, field_names, field_types, instruction_args);
                sem.pda = Some(PdaConstraint {
                    source: PdaSource::Raw { seeds },
                    bump: lower_bump(&bump),
                });
            }
            AccountDirective::TypedSeeds(ts) => {
                let args = lower_seed_nodes(ts.args, field_names, field_types, instruction_args);
                sem.pda = Some(PdaConstraint {
                    source: PdaSource::Typed {
                        type_path: ts.type_path,
                        args,
                    },
                    bump: lower_bump(&bump),
                });
            }
            AccountDirective::Bump(v) => {
                bump = Some(v);
                if let Some(pda) = &mut sem.pda {
                    pda.bump = lower_bump(&bump);
                }
            }
            AccountDirective::TokenMint(v) => token_mint = Some(v),
            AccountDirective::TokenAuthority(v) => token_authority = Some(v),
            AccountDirective::TokenTokenProgram(v) => token_token_program = Some(v),
            AccountDirective::AssociatedTokenMint(v) => ata_mint = Some(v),
            AccountDirective::AssociatedTokenAuthority(v) => ata_authority = Some(v),
            AccountDirective::AssociatedTokenTokenProgram(v) => ata_token_program = Some(v),
            AccountDirective::Realloc(v) => realloc = Some(v),
            AccountDirective::ReallocPayer(v) => realloc_payer = Some(v),
            AccountDirective::MintDecimals(v) => mint_decimals = Some(v),
            AccountDirective::MintInitAuthority(v) => mint_authority = Some(v),
            AccountDirective::MintFreezeAuthority(v) => mint_freeze_authority = Some(v),
            AccountDirective::MintTokenProgram(v) => mint_token_program = Some(v),
            AccountDirective::Param { key, value } => {
                sem.params.validate.push(ParamAssign { key, value });
            }
            AccountDirective::InitParam { key, value } => {
                sem.params.init.push(ParamAssign { key, value });
            }
        }
    }

    if let Some(mode) = init_mode {
        sem.init = Some(InitConstraint {
            mode,
            payer: payer.clone(),
            space,
        });
    }

    if let (Some(mint), Some(authority)) = (token_mint, token_authority) {
        sem.token = Some(TokenConstraint {
            mint,
            authority,
            token_program: token_token_program,
        });
    }

    if let (Some(mint), Some(authority)) = (ata_mint, ata_authority) {
        sem.ata = Some(super::AtaConstraint {
            mint,
            authority,
            token_program: ata_token_program,
        });
    }

    if let (Some(decimals), Some(authority)) = (mint_decimals, mint_authority) {
        sem.mint = Some(MintConstraint {
            decimals,
            authority,
            freeze_authority: mint_freeze_authority,
            token_program: mint_token_program,
        });
    }

    if let Some(space_expr) = realloc {
        sem.realloc = Some(ReallocConstraint {
            space_expr,
            payer: realloc_payer,
        });
    }

    // For Migration<From, To> fields, store explicit payer directly on support
    // so resolve_supports can find it. Without init, payer has nowhere else to go.
    if matches!(sem.core.shape, FieldShape::Migration) {
        if let Some(p) = payer {
            sem.support.payer = Some(p);
        }
    }
}

fn lower_seed_nodes(
    exprs: Vec<Expr>,
    field_names: &[String],
    field_types: &[(Ident, Type)],
    instruction_args: &Option<Vec<crate::accounts::InstructionArg>>,
) -> Vec<SeedNode> {
    exprs
        .into_iter()
        .map(|expr| classify_seed(expr, field_names, field_types, instruction_args))
        .collect()
}
