use {super::super::resolve::FieldSemantics, quote::quote};

/// Emit generic `param::` validation — constructs a `Params::default()`,
/// fills user-specified fields, calls `AccountLoad::validate()`.
pub(super) fn emit_validate_params(sem: &FieldSemantics) -> Option<proc_macro2::TokenStream> {
    let field = &sem.core.ident;
    emit_validate_params_on(sem, quote! { #field })
}

/// Same as `emit_validate_params`, but validates a specific receiver
/// expression.
///
/// Used by `init_if_needed` existing-account validation, where the raw
/// `AccountView` has been loaded into `__existing` through `AccountLoad`.
pub(super) fn emit_validate_params_on(
    sem: &FieldSemantics,
    receiver: proc_macro2::TokenStream,
) -> Option<proc_macro2::TokenStream> {
    if sem.params.validate.is_empty() {
        return None;
    }
    let ty = &sem.core.effective_ty;
    let assigns = sem.params.validate.iter().map(|p| {
        let key = &p.key;
        let value = &p.value;
        quote! { __params.#key = #value; }
    });
    Some(quote! {
        {
            let mut __params =
                <#ty as quasar_lang::account_load::AccountLoad>::Params::default();
            #(#assigns)*
            quasar_lang::account_load::AccountLoad::validate(#receiver, &__params)?;
        }
    })
}

/// Emit built-in SPL validation params against `receiver`.
///
/// This calls `AccountLoad::validate(receiver, &__params)` with the built-in
/// token/mint constraints. ATA is handled separately because it validates the
/// associated token address derivation in addition to token account data.
pub(super) fn emit_builtin_validate_params_for(
    sem: &FieldSemantics,
    receiver: proc_macro2::TokenStream,
) -> Option<proc_macro2::TokenStream> {
    let ty = &sem.core.effective_ty;

    if let Some(tc) = &sem.token {
        let mint = &tc.mint;
        let auth = &tc.authority;
        let tp = sem
            .support
            .token_program
            .as_ref()
            .map(|tp| quote::quote! { *#tp.to_account_view().address() });
        let tp_assign = tp
            .map(|expr| quote::quote! { __params.token_program = Some(#expr); })
            .unwrap_or_default();
        return Some(quote! {
            {
                let mut __params =
                    <#ty as quasar_lang::account_load::AccountLoad>::Params::default();
                __params.mint = Some(*#mint.to_account_view().address());
                __params.authority = Some(*#auth.to_account_view().address());
                #tp_assign
                quasar_lang::account_load::AccountLoad::validate(#receiver, &__params)?;
            }
        });
    }

    if let Some(mc) = &sem.mint {
        let auth = &mc.authority;
        let decimals = &mc.decimals;
        let freeze_expr = mc.freeze_authority.as_ref().map(|fa| {
            quote::quote! { __params.freeze_authority = Some(*#fa.to_account_view().address()); }
        });
        let freeze_stmts = freeze_expr.unwrap_or_default();
        let tp = sem
            .support
            .token_program
            .as_ref()
            .map(|tp| quote::quote! { *#tp.to_account_view().address() });
        let tp_assign = tp
            .map(|expr| quote::quote! { __params.token_program = Some(#expr); })
            .unwrap_or_default();
        return Some(quote! {
            {
                let mut __params =
                    <#ty as quasar_lang::account_load::AccountLoad>::Params::default();
                __params.authority = Some(*#auth.to_account_view().address());
                __params.decimals = Some((#decimals) as u8);
                #freeze_stmts
                #tp_assign
                quasar_lang::account_load::AccountLoad::validate(#receiver, &__params)?;
            }
        });
    }

    None
}

/// Returns field assignments for `__init_params`, combining:
///   1. Built-in SPL assignments (from `sem.token` / `sem.mint` + resolved
///      support)
///   2. User `init_param::` assignments (from `sem.params.init`)
///
/// The caller is responsible for:
///   `type __Target = <#ty as AccountLoad>::BehaviorTarget;`
///   `let mut __init_params = <__Target as
/// AccountInit>::InitParams::default();`
pub(super) fn emit_init_param_assigns(
    sem: &FieldSemantics,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut stmts = Vec::new();

    // 1. Built-in SPL token init assignments.
    if sem.init.is_some() {
        if let Some(tc) = &sem.token {
            let mint = &tc.mint;
            let authority = &tc.authority;
            let tp = sem.support.token_program.as_ref().ok_or_else(|| {
                syn::Error::new(
                    sem.core.ident.span(),
                    "token init requires a token program field",
                )
            })?;
            stmts.push(quote! { __init_params.mint = Some(#mint.to_account_view()); });
            stmts.push(quote! { __init_params.authority = Some(#authority.address()); });
            stmts.push(quote! { __init_params.token_program = Some(#tp.to_account_view()); });
        }

        // 2. Built-in SPL mint init assignments.
        if let Some(mc) = &sem.mint {
            let decimals = &mc.decimals;
            let authority = &mc.authority;
            let tp = sem.support.token_program.as_ref().ok_or_else(|| {
                syn::Error::new(
                    sem.core.ident.span(),
                    "mint init requires a token program field",
                )
            })?;
            stmts.push(quote! { __init_params.decimals = Some((#decimals) as u8); });
            stmts.push(quote! { __init_params.authority = Some(#authority.address()); });
            if let Some(fa) = &mc.freeze_authority {
                stmts.push(quote! { __init_params.freeze_authority = Some(#fa.address()); });
            }
            stmts.push(quote! { __init_params.token_program = Some(#tp.to_account_view()); });
        }
    }

    // 3. User init_param:: assignments — reject overrides of built-in SPL fields.
    let builtin_keys: &[&str] = if sem.token.is_some() && sem.init.is_some() {
        &["mint", "authority", "token_program"]
    } else if sem.mint.is_some() && sem.init.is_some() {
        &["decimals", "authority", "freeze_authority", "token_program"]
    } else {
        &[]
    };
    for p in &sem.params.init {
        if builtin_keys.iter().any(|k| p.key == k) {
            return Err(syn::Error::new(
                p.key.span(),
                format!(
                    "`init_param::{}` conflicts with built-in SPL init constraint",
                    p.key,
                ),
            ));
        }
        let key = &p.key;
        let value = &p.value;
        stmts.push(quote! { __init_params.#key = #value; });
    }

    Ok(quote! { #(#stmts)* })
}
