//! Typed execution plan — the semantic model that replaces GroupOp bags.
//!
//! After planning, every field has a `FieldPlan` with phase-ordered steps.
//! Emit consumes these typed specs directly — no string-based arg lookup.

use syn::{Expr, Ident, Type};

// ---------------------------------------------------------------------------
// Value provenance
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

/// A reference to an account field or constant expression.
#[derive(Clone)]
pub(crate) enum ArgRef {
    /// Another field in the accounts struct.
    Field(Ident),
    /// A constant path or literal expression.
    Expr(Expr),
}

/// A resolved account reference (field or expression).
#[derive(Clone)]
pub(crate) struct AccountRef {
    pub inner: ArgRef,
}

impl AccountRef {
    pub(crate) fn field(ident: Ident) -> Self {
        Self {
            inner: ArgRef::Field(ident),
        }
    }

    pub(crate) fn expr(expr: Expr) -> Self {
        Self {
            inner: ArgRef::Expr(expr),
        }
    }

    pub(crate) fn arg_ref(&self) -> &ArgRef {
        &self.inner
    }
}

/// A reference that is guaranteed to be a field (never an expression).
/// Used for payer and program refs where the planner enforces field-only.
#[derive(Clone)]
pub(crate) struct FieldRef {
    pub ident: Ident,
}

/// How the token program is resolved for an init/exit operation (CPI needed).
pub(crate) type ProgramRef = FieldRef;

// ---------------------------------------------------------------------------
// Omission semantics
// ---------------------------------------------------------------------------

/// Validation check mode: either skip or perform the check.
#[derive(Clone)]
pub(crate) enum CheckMode<T> {
    /// Omitted by user → do not check this field.
    DoNotCheck,
    /// Present → check against this value.
    Check(T),
}

/// A value that may be explicitly provided or defaulted.
#[derive(Clone)]
pub(crate) enum MaybeDefault<T> {
    Explicit(T),
    Defaulted(T),
}

impl<T> MaybeDefault<T> {
    pub(crate) fn value(&self) -> &T {
        match self {
            Self::Explicit(v) | Self::Defaulted(v) => v,
        }
    }
}

/// Freeze authority specification.
#[derive(Clone)]
pub(crate) enum FreezeAuthoritySpec {
    /// No freeze authority.
    None,
    /// Freeze authority is this account.
    Some(AccountRef),
}

// ---------------------------------------------------------------------------
// Check specs
// ---------------------------------------------------------------------------

/// Fully resolved token account validation spec.
#[derive(Clone)]
pub(crate) struct TokenCheckSpec {
    pub mint: AccountRef,
    pub authority: AccountRef,
    pub token_program: TokenProgramCheckRef,
}

/// How the token program is provided for a check operation.
#[derive(Clone)]
pub(crate) enum TokenProgramCheckRef {
    /// Concrete account type proves the owner — no runtime field needed.
    /// Emit produces `None` for the ctx field.
    ConcreteOwner,
    /// Runtime program field provides the check value.
    RuntimeField(Ident),
}

/// Fully resolved mint account validation spec.
#[derive(Clone)]
pub(crate) struct MintCheckSpec {
    pub authority: AccountRef,
    pub decimals: CheckMode<Expr>,
    pub freeze_authority: CheckMode<FreezeAuthoritySpec>,
    pub token_program: TokenProgramCheckRef,
}

/// Fully resolved associated token validation spec.
#[derive(Clone)]
pub(crate) struct AssociatedTokenCheckSpec {
    pub mint: AccountRef,
    pub authority: AccountRef,
    pub token_program: TokenProgramCheckRef,
}

// ---------------------------------------------------------------------------
// Init specs
// ---------------------------------------------------------------------------

/// Space specification for program init.
#[derive(Clone)]
pub(crate) enum SpaceSpec {
    /// Derived from `<T as Space>::SPACE`.
    FromType(Type),
}

/// Plain program account init.
#[derive(Clone)]
pub(crate) struct ProgramInitSpec {
    pub payer: FieldRef,
    pub space: SpaceSpec,
    pub idempotent: bool,
}

/// Token account init.
#[derive(Clone)]
pub(crate) struct TokenInitSpec {
    pub payer: FieldRef,
    pub mint: AccountRef,
    pub authority: AccountRef,
    pub token_program: ProgramRef,
    pub idempotent: bool,
}

/// Mint account init.
#[derive(Clone)]
pub(crate) struct MintInitSpec {
    pub payer: FieldRef,
    pub decimals: MaybeDefault<Expr>,
    pub authority: AccountRef,
    pub freeze_authority: MaybeDefault<FreezeAuthoritySpec>,
    pub token_program: ProgramRef,
    pub idempotent: bool,
}

/// Associated token account init.
#[derive(Clone)]
pub(crate) struct AssociatedTokenInitSpec {
    pub payer: FieldRef,
    pub mint: AccountRef,
    pub authority: AccountRef,
    pub token_program: ProgramRef,
    pub system_program: ProgramRef,
    pub ata_program: ProgramRef,
    pub idempotent: bool,
}

/// Discriminated init plan.
#[derive(Clone)]
pub(crate) enum InitPlan {
    Program(ProgramInitSpec),
    Token(TokenInitSpec),
    Mint(MintInitSpec),
    AssociatedToken(AssociatedTokenInitSpec),
}

// ---------------------------------------------------------------------------
// Exit specs
// ---------------------------------------------------------------------------

/// Program-level close (drain lamports).
#[derive(Clone)]
pub(crate) struct ProgramCloseSpec {
    pub destination: AccountRef,
}

/// Token close via CPI.
#[derive(Clone)]
pub(crate) struct TokenCloseSpec {
    pub destination: AccountRef,
    pub authority: AccountRef,
    pub token_program: ProgramRef,
}

/// Token sweep (transfer all tokens before close).
#[derive(Clone)]
pub(crate) struct TokenSweepSpec {
    pub receiver: AccountRef,
    pub mint: AccountRef,
    pub authority: AccountRef,
    pub token_program: ProgramRef,
}

// ---------------------------------------------------------------------------
// Realloc and migration specs
// ---------------------------------------------------------------------------

/// Realloc spec.
#[derive(Clone)]
pub(crate) struct ReallocSpec {
    pub new_space: Expr,
    pub payer: FieldRef,
}

/// Migration spec.
#[derive(Clone)]
pub(crate) struct MigrationSpec {
    pub payer: FieldRef,
}

// ---------------------------------------------------------------------------
// Address spec
// ---------------------------------------------------------------------------

/// Address verification plan for a field.
#[derive(Clone)]
pub(crate) struct AddressSpec {
    pub expr: Expr,
}

// ---------------------------------------------------------------------------
// Rent plan (instruction-wide)
// ---------------------------------------------------------------------------

/// Instruction-wide rent resolution.
#[derive(Clone)]
pub(crate) enum RentPlan {
    /// No step needs rent.
    NotNeeded,
    /// A Sysvar<Rent> field exists — read from it.
    FromSysvarField { field: Ident },
    /// No sysvar field — syscall once.
    FetchOnce,
}

// ---------------------------------------------------------------------------
// Field plan — per-field phase vectors only
// ---------------------------------------------------------------------------

/// A step that runs before account load (address verify + init CPI).
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum PreLoadStep {
    VerifyAddress(AddressSpec),
    Init(InitPlan),
}

/// A step that runs after account load (checks, realloc, migration grow).
#[derive(Clone)]
pub(crate) enum PostLoadStep {
    TokenCheck(TokenCheckSpec),
    MintCheck(MintCheckSpec),
    AssociatedTokenCheck(AssociatedTokenCheckSpec),
    Realloc(ReallocSpec),
    MigrationGrow(MigrationSpec),
    VerifyExistingAddress(AddressSpec),
}

/// A step that runs in the epilogue (close, sweep, migration normalize).
#[derive(Clone)]
pub(crate) enum EpilogueStep {
    TokenSweep(TokenSweepSpec),
    TokenClose(TokenCloseSpec),
    ProgramClose(ProgramCloseSpec),
    MigrationVerifyAndNormalize(MigrationSpec),
}

/// Per-field execution plan. Only phase vectors — structural info lives in
/// FieldSemantics.
#[derive(Clone)]
pub(crate) struct FieldPlan {
    /// Steps before load (init fields only).
    pub pre_load: Vec<PreLoadStep>,
    /// Steps after load (checks, realloc, migration grow, address verify).
    pub post_load: Vec<PostLoadStep>,
    /// Steps in epilogue (sweep, close, migration normalize).
    pub epilogue: Vec<EpilogueStep>,
}

/// Instruction-wide execution plan.
pub(crate) struct AccountsPlanTyped {
    pub fields: Vec<FieldPlan>,
    pub rent: RentPlan,
}
