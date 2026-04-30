use syn::{Expr, Ident, Type};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldShape {
    Account,
    Migration,
    InterfaceAccount,
    Program,
    Interface,
    Sysvar,
    Signer,
    SystemAccount,
    Composite,
    Other,
}

pub(crate) struct FieldCore {
    pub ident: Ident,
    pub field: syn::Field,
    pub effective_ty: Type,
    pub shape: FieldShape,
    /// Inner/source type for account-bearing wrappers.
    ///
    /// For `Migration<From, To>`, this is `From`.
    pub inner_ty: Option<Type>,
    /// Base name of the generic inner type for any generic wrapper.
    pub inner_name: Option<Ident>,
    pub is_token_account: bool,
    pub is_mint: bool,
    pub is_token_or_mint: bool,
    /// Existing-account PDA bump fast path is valid after wrapper validation.
    pub supports_existing_pda_fast_path: bool,
    pub optional: bool,
    pub dynamic: bool,
    pub is_mut: bool,
    pub dup: bool,
}

pub(crate) struct FieldSemantics {
    pub core: FieldCore,
    pub support: FieldSupport,
    pub params: FieldParams,
    pub init: Option<InitConstraint>,
    pub pda: Option<PdaConstraint>,
    pub token: Option<TokenConstraint>,
    pub ata: Option<AtaConstraint>,
    pub mint: Option<MintConstraint>,
    pub realloc: Option<ReallocConstraint>,
    pub lifecycle: Vec<LifecycleConstraint>,
    pub user_checks: Vec<UserCheckConstraint>,
}

impl FieldSemantics {
    pub fn has_init(&self) -> bool {
        self.init.is_some()
    }

    pub fn needs_rent(&self) -> bool {
        self.init.is_some()
            || self.realloc.is_some()
            || matches!(self.core.shape, FieldShape::Migration)
    }

    pub fn has_realloc(&self) -> bool {
        self.realloc.is_some()
    }

    pub fn has_lifecycle(&self) -> bool {
        !self.lifecycle.is_empty()
    }

    pub fn is_writable(&self) -> bool {
        self.core.is_mut
            || self.has_init()
            || self.has_lifecycle()
            || self.has_realloc()
            || matches!(self.core.shape, FieldShape::Migration)
    }

    pub fn client_requires_signer(&self) -> bool {
        matches!(
            self.init.as_ref().map(|init| &init.mode),
            Some(InitMode::Init)
        ) && self.pda.is_none()
            && self.ata.is_none()
    }

    pub(crate) fn has_close(&self) -> bool {
        self.lifecycle
            .iter()
            .any(|lc| matches!(lc, LifecycleConstraint::Close { .. }))
    }

    pub(crate) fn has_sweep(&self) -> bool {
        self.lifecycle
            .iter()
            .any(|lc| matches!(lc, LifecycleConstraint::Sweep { .. }))
    }

    pub(crate) fn has_raw_pda(&self) -> bool {
        self.pda
            .as_ref()
            .is_some_and(|pda| matches!(pda.source, PdaSource::Raw { .. }))
    }
}

#[derive(Default)]
pub(crate) struct FieldSupport {
    pub payer: Option<Ident>,
    pub realloc_payer: Option<Ident>,
    pub system_program: Option<Ident>,
    pub token_program: Option<Ident>,
    pub associated_token_program: Option<Ident>,
    pub rent_sysvar: Option<Ident>,
}

pub(crate) struct InitConstraint {
    pub mode: InitMode,
    pub payer: Option<Ident>,
    pub space: Option<Expr>,
}

pub(crate) enum InitMode {
    Init,
    InitIfNeeded,
}

pub(crate) struct PdaConstraint {
    pub source: PdaSource,
    pub bump: Option<BumpSyntax>,
}

pub(crate) enum PdaSource {
    Raw {
        seeds: Vec<SeedNode>,
    },
    Typed {
        type_path: syn::Path,
        args: Vec<SeedNode>,
    },
}

pub(crate) enum SeedNode {
    Literal(Vec<u8>),
    AccountAddress {
        field: Ident,
    },
    FieldBytes {
        root: Ident,
        path: Vec<Ident>,
        root_ty: Option<Type>,
    },
    InstructionArg {
        name: Ident,
        ty: Type,
    },
    // Carries the root ident and wrapper type so init emission can rewrite the
    // raw AccountView root into the correct typed cast.
    FieldRootedExpr {
        root: Ident,
        expr: Expr,
        root_ty: Option<Type>,
    },
    OpaqueExpr(Expr),
}

pub(crate) enum BumpSyntax {
    Bare,
    Explicit(Expr),
}

pub(crate) struct TokenConstraint {
    pub mint: Ident,
    pub authority: Ident,
    pub token_program: Option<Ident>,
}

pub(crate) struct AtaConstraint {
    pub mint: Ident,
    pub authority: Ident,
    pub token_program: Option<Ident>,
}

pub(crate) struct MintConstraint {
    pub decimals: Expr,
    pub authority: Ident,
    pub freeze_authority: Option<Ident>,
    pub token_program: Option<Ident>,
}

pub(crate) struct ParamAssign {
    pub key: syn::Ident,
    pub value: syn::Expr,
}

#[derive(Default)]
pub(crate) struct FieldParams {
    pub validate: Vec<ParamAssign>,
    pub init: Vec<ParamAssign>,
}

pub(crate) struct UserCheckConstraint {
    pub kind: UserCheckKind,
    pub error: Option<Expr>,
}

pub(crate) enum UserCheckKind {
    HasOne { target: Ident },
    Constraint { expr: Expr },
    Address { expr: Expr },
}

pub(crate) enum LifecycleConstraint {
    Close { destination: Ident },
    Sweep { receiver: Ident },
}

pub(crate) struct ReallocConstraint {
    pub space_expr: Expr,
    pub payer: Option<Ident>,
}
