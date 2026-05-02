use syn::{Expr, Ident, Type};

/// 2 variants. No domain knowledge.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldKind {
    Single,
    Composite,
}

/// Op classification for direct capability dispatch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OpKind {
    /// Validation after typed load; also contributes init params on init
    /// fields.
    Check,
    /// Epilogue action (exit phase).
    Exit,
}

/// Known op group names after directive classification.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum GroupKind {
    Token,
    Mint,
    AssociatedToken,
    Close,
    Sweep,
}

impl GroupKind {
    pub(crate) fn from_path(path: &syn::Path) -> syn::Result<Self> {
        if let Some(segment) = path.segments.last() {
            let ident = &segment.ident;
            if ident == "token" {
                return Ok(Self::Token);
            }
            if ident == "mint" {
                return Ok(Self::Mint);
            }
            if ident == "associated_token" {
                return Ok(Self::AssociatedToken);
            }
            if ident == "close" {
                return Ok(Self::Close);
            }
            if ident == "sweep" {
                return Ok(Self::Sweep);
            }
        }

        let name = path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        Err(syn::Error::new_spanned(
            path,
            format!(
                "unknown op group `{name}`. Valid: token, mint, associated_token, close, sweep"
            ),
        ))
    }

    pub(crate) const fn op_kind(self) -> OpKind {
        match self {
            Self::Token | Self::Mint | Self::AssociatedToken => OpKind::Check,
            Self::Close | Self::Sweep => OpKind::Exit,
        }
    }

    pub(crate) const fn exit_order(self) -> u8 {
        match self {
            Self::Sweep => 0,
            Self::Close => 1,
            _ => 2,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Mint => "mint",
            Self::AssociatedToken => "associated_token",
            Self::Close => "close",
            Self::Sweep => "sweep",
        }
    }
}

pub(crate) struct FieldCore {
    pub ident: Ident,
    pub field: syn::Field,
    pub effective_ty: Type,
    pub kind: FieldKind,
    /// Inner/source type for generic wrappers.
    pub inner_ty: Option<Type>,
    pub optional: bool,
    pub dynamic: bool,
    pub is_mut: bool,
    pub dup: bool,
}

/// A group directive: `path(key = value, ...)`.
#[derive(Clone)]
pub(crate) struct GroupDirective {
    pub path: syn::Path,
    pub kind: GroupKind,
    pub args: Vec<GroupArg>,
}

/// A single `key = value` arg in a group directive.
#[derive(Clone)]
pub(crate) struct GroupArg {
    pub key: Ident,
    pub value: Expr,
}

/// User-specified structural assertion.
pub(crate) enum UserCheck {
    HasOne {
        targets: Vec<Ident>,
        error: Option<Expr>,
    },
    Address {
        expr: Expr,
        error: Option<Expr>,
    },
    Constraints {
        exprs: Vec<Expr>,
        error: Option<Expr>,
    },
}

pub(crate) struct FieldSemantics {
    pub core: FieldCore,
    /// `init` / `init(idempotent)` — structural, Phase 1.
    pub init: Option<InitDirective>,
    /// Top-level `payer = field`.
    pub payer: Option<Ident>,
    /// `address = expr` — opaque address constraint.
    pub address: Option<Expr>,
    /// `realloc = expr` — realloc size expression.
    pub realloc: Option<Expr>,
    /// All op groups (raw directives — classification happens in the planner).
    pub groups: Vec<GroupDirective>,
    /// Structural assertions: has_one, address, constraints.
    pub user_checks: Vec<UserCheck>,
    /// True when the field type is `Migration<From, To>` (syntactic detection
    /// on the last path segment). Proc macros cannot resolve type aliases —
    /// only direct `Migration<From, To>` paths are supported.
    pub is_migration: bool,
}

impl FieldSemantics {
    pub fn has_init(&self) -> bool {
        self.init.is_some()
    }

    pub fn is_writable(&self) -> bool {
        self.core.is_mut || self.has_init()
    }
}

/// Parsed `init` / `init(idempotent)` directive.
pub(crate) struct InitDirective {
    pub idempotent: bool,
}
