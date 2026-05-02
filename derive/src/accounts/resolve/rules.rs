//! Structural validation rules — no domain knowledge.
//! Arg presence/absence validation is now done by the planner during
//! resolution.

use super::{FieldSemantics, GroupKind};

pub(super) fn validate_semantics(semantics: &[FieldSemantics]) -> syn::Result<()> {
    for sem in semantics {
        validate_field(sem)?;
    }
    Ok(())
}

fn validate_field(sem: &FieldSemantics) -> syn::Result<()> {
    let span = &sem.core.field;

    // --- Migration exclusivity rules ---
    if sem.is_migration {
        if !sem.core.is_mut {
            return Err(syn::Error::new_spanned(
                span,
                "`Migration<From, To>` requires `mut`",
            ));
        }
        // Payer presence is validated by the planner (cross-field resolution).
        if sem.core.optional {
            return Err(syn::Error::new_spanned(
                span,
                "`Option<Migration<...>>` is not supported — migration fields cannot be optional",
            ));
        }
        if sem.has_init() {
            return Err(syn::Error::new_spanned(
                span,
                "`init` cannot be used with `Migration<From, To>`",
            ));
        }
        if sem.realloc.is_some() {
            return Err(syn::Error::new_spanned(
                span,
                "`realloc` cannot be used with `Migration<From, To>`",
            ));
        }
        for group in &sem.groups {
            if matches!(group.kind, GroupKind::Close | GroupKind::Sweep) {
                return Err(syn::Error::new_spanned(
                    span,
                    format!(
                        "`{}` cannot be used with `Migration<From, To>` — migrating and closing \
                         the same account is ordering-sensitive",
                        group.kind.name()
                    ),
                ));
            }
        }
    }

    // init requires mut
    if sem.has_init() && !sem.core.is_mut {
        return Err(syn::Error::new_spanned(span, "`init(...)` requires `mut`"));
    }

    // init + realloc mutual exclusion
    if sem.has_init() && sem.realloc.is_some() {
        return Err(syn::Error::new_spanned(
            span,
            "`realloc = ...` cannot be used with `init`",
        ));
    }

    // dup + mutation ops blocked (init, realloc, exit ops)
    if sem.core.dup {
        if sem.has_init() {
            return Err(syn::Error::new_spanned(
                span,
                "`dup` cannot be used with `init` — mutation on aliased accounts is unsound",
            ));
        }
        if sem.realloc.is_some() {
            return Err(syn::Error::new_spanned(
                span,
                "`dup` cannot be used with `realloc` — mutation on aliased accounts is unsound",
            ));
        }
        for group in &sem.groups {
            if matches!(group.kind, GroupKind::Close | GroupKind::Sweep) {
                return Err(syn::Error::new_spanned(
                    span,
                    format!(
                        "`dup` cannot be used with `{}` — mutation on aliased accounts is unsound",
                        group.kind.name()
                    ),
                ));
            }
        }
    }

    // Exit ops require mut
    if !sem.core.is_mut {
        for group in &sem.groups {
            if matches!(group.kind, GroupKind::Close | GroupKind::Sweep) {
                return Err(syn::Error::new_spanned(
                    span,
                    format!("`{}(...)` requires `mut`", group.kind.name()),
                ));
            }
        }
    }

    // sweep-before-close hard error: close must come AFTER sweep.
    validate_exit_ordering(sem)?;
    validate_close_groups(sem)?;

    // dup requires /// CHECK: doc comment
    if sem.core.dup {
        let has_doc = sem
            .core
            .field
            .attrs
            .iter()
            .any(|a| a.path().is_ident("doc"));
        if !has_doc {
            return Err(syn::Error::new_spanned(
                span,
                "#[account(dup)] requires a /// CHECK: <reason> doc comment",
            ));
        }
    }

    // Optional accounts cannot have init or op groups
    if sem.core.optional {
        if sem.has_init() {
            return Err(syn::Error::new_spanned(
                span,
                "init(...) cannot be used on Option<T> fields",
            ));
        }
        if sem.realloc.is_some() {
            return Err(syn::Error::new_spanned(
                span,
                "`realloc = ...` cannot be used on Option<T> fields",
            ));
        }
        if !sem.groups.is_empty() {
            return Err(syn::Error::new_spanned(
                span,
                "op groups cannot be used on Option<T> fields (only has_one/address/constraints)",
            ));
        }
    }

    // Payer presence is validated by the planner (cross-field resolution).
    if sem.realloc.is_some() && !sem.core.is_mut {
        return Err(syn::Error::new_spanned(
            span,
            "`realloc = ...` requires `mut`",
        ));
    }

    // Zero or one init contributor per field.
    if sem.has_init() {
        let init_contributor_count = sem
            .groups
            .iter()
            .filter(|group| {
                matches!(
                    group.kind,
                    GroupKind::Token | GroupKind::Mint | GroupKind::AssociatedToken
                )
            })
            .count();
        if init_contributor_count > 1 {
            return Err(syn::Error::new_spanned(
                span,
                "only one init contributor group is allowed per field (e.g., `token(...)` or \
                 `associated_token(...)`, not both)",
            ));
        }
    }

    // init(idempotent) requires a validation group or address constraint
    if let Some(init) = &sem.init {
        if init.idempotent {
            let has_validation = !sem.groups.is_empty();
            let has_address = sem.address.is_some();
            if !has_validation && !has_address {
                return Err(syn::Error::new_spanned(
                    span,
                    "`init(idempotent)` requires a validation group (e.g., token(...)) or address \
                     constraint",
                ));
            }
        }
    }

    Ok(())
}

/// Validate close args and reject ambiguous close forms.
fn validate_close_groups(sem: &FieldSemantics) -> syn::Result<()> {
    let has_sweep = sem
        .groups
        .iter()
        .any(|group| group.kind == GroupKind::Sweep);

    for group in &sem.groups {
        if group.kind != GroupKind::Close {
            continue;
        }

        let has_authority = group.args.iter().any(|a| a.key == "authority");
        let has_token_program = group.args.iter().any(|a| a.key == "token_program");
        if has_token_program && !has_authority {
            return Err(syn::Error::new_spanned(
                &group.path,
                "`close(...)` with `token_program = ...` also requires `authority = ...`",
            ));
        }

        if has_sweep && !has_authority {
            return Err(syn::Error::new_spanned(
                &group.path,
                "`sweep(...)` can only be paired with token close. Use `close(dest = ..., \
                 authority = ...)`",
            ));
        }
    }

    Ok(())
}

/// Validate exit action ordering: sweep must come before close.
fn validate_exit_ordering(sem: &FieldSemantics) -> syn::Result<()> {
    let span = &sem.core.field;
    let mut seen_close = false;

    for group in &sem.groups {
        match group.kind {
            GroupKind::Close => {
                seen_close = true;
            }
            GroupKind::Sweep if seen_close => {
                return Err(syn::Error::new_spanned(
                    span,
                    "`sweep(...)` must appear before `close(...)` — wrong ordering is always a bug",
                ));
            }
            _ => {}
        }
    }

    Ok(())
}
