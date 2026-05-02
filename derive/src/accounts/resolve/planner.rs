//! Typed execution planner — owns inference, classification, and resolution.
//!
//! This module replaces: resolve_program_args, resolve_payer, classify_groups,
//! and validate_required_group_args. It reads raw GroupDirectives and produces
//! typed specs with correct ValueSource provenance.

use {
    super::{
        model::{FieldCore, FieldKind, FieldSemantics, GroupArg, GroupDirective, GroupKind},
        specs::*,
    },
    crate::helpers::extract_generic_inner_type,
    syn::{Expr, Ident, Type},
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Build a typed execution plan from lowered field semantics.
/// This is the single owner of inference, classification, and resolution.
pub(crate) fn build_plan(semantics: &[FieldSemantics]) -> syn::Result<AccountsPlanTyped> {
    // Step 0: Validate group args (unknown args, field references).
    let field_names: Vec<String> = semantics
        .iter()
        .map(|sem| sem.core.ident.to_string())
        .collect();
    for sem in semantics {
        validate_all_group_args(sem, &field_names)?;
    }

    // Step 1: Build instruction-wide indexes for inference.
    let program_candidates = scan_program_candidates(semantics);
    let payer_field = find_payer_field(semantics);

    // Step 2: Plan each field.
    let fields: Vec<FieldPlan> = semantics
        .iter()
        .map(|sem| plan_field(sem, &program_candidates, payer_field.as_ref()))
        .collect::<syn::Result<_>>()?;

    // Step 3: Compute instruction-wide rent plan.
    let rent = compute_rent_plan(semantics);

    Ok(AccountsPlanTyped { fields, rent })
}

// ---------------------------------------------------------------------------
// Group arg validation (items 1, 4: unknown args + field reference checks)
// ---------------------------------------------------------------------------

/// Known args per group kind.
fn known_args(kind: GroupKind) -> &'static [&'static str] {
    match kind {
        GroupKind::Token => &["mint", "authority", "token_program"],
        GroupKind::Mint => &["authority", "decimals", "freeze_authority", "token_program"],
        GroupKind::AssociatedToken => &[
            "mint",
            "authority",
            "token_program",
            "system_program",
            "ata_program",
        ],
        GroupKind::Close => &["dest", "authority", "token_program"],
        GroupKind::Sweep => &["receiver", "mint", "authority", "token_program"],
    }
}

/// Args that must be field references (not literals/paths).
fn field_ref_args(kind: GroupKind) -> &'static [&'static str] {
    match kind {
        GroupKind::Token => &["mint", "authority", "token_program"],
        GroupKind::Mint => &["authority", "freeze_authority", "token_program"],
        GroupKind::AssociatedToken => &[
            "mint",
            "authority",
            "token_program",
            "system_program",
            "ata_program",
        ],
        GroupKind::Close => &["dest", "authority", "token_program"],
        GroupKind::Sweep => &["receiver", "mint", "authority", "token_program"],
    }
}

/// Validate all group args on a field: reject unknown args, validate field
/// refs.
fn validate_all_group_args(sem: &FieldSemantics, field_names: &[String]) -> syn::Result<()> {
    for group in &sem.groups {
        let valid = known_args(group.kind);
        let must_be_field = field_ref_args(group.kind);

        for arg in &group.args {
            let key_str = arg.key.to_string();

            // Item 1: reject unknown args.
            if !valid.contains(&key_str.as_str()) {
                return Err(syn::Error::new_spanned(
                    &arg.key,
                    format!(
                        "unknown `{}(...)` arg `{key_str}`. Valid args: {}",
                        group.kind.name(),
                        valid.join(", "),
                    ),
                ));
            }

            // Item 4: validate field references against struct.
            if must_be_field.contains(&key_str.as_str()) {
                validate_field_ref_arg(&arg.value, &arg.key, &key_str, field_names)?;
            }
        }
    }
    Ok(())
}

// Note: Item 5 (validate program override types) is handled in the planner's
// resolve_* functions via require_field_ident + resolve_token_program_field
// which checks compatible_token_categories. If the user writes `token_program =
// system_program`, resolve_token_program_field will reject it because
// Program<SystemProgram> is not in the compatible set for token ops. This is
// already enforced.

/// Validate that an arg value that should be a field reference actually refers
/// to a field in the struct. Accepts: bare ident (field), None, Some(field).
fn validate_field_ref_arg(
    expr: &Expr,
    _key_span: &Ident,
    key_name: &str,
    field_names: &[String],
) -> syn::Result<()> {
    match expr {
        // None — only valid for freeze_authority
        Expr::Path(ep)
            if ep.qself.is_none()
                && ep.path.segments.len() == 1
                && ep.path.segments[0].ident == "None" =>
        {
            if key_name == "freeze_authority" {
                Ok(())
            } else {
                Err(syn::Error::new_spanned(
                    expr,
                    format!("`{key_name}` does not accept `None`"),
                ))
            }
        }
        // Some(inner) — only valid for freeze_authority
        Expr::Call(call)
            if matches!(&*call.func, Expr::Path(p)
                if p.path.segments.len() == 1 && p.path.segments[0].ident == "Some")
                && call.args.len() == 1 =>
        {
            if key_name == "freeze_authority" {
                validate_field_ref_arg(&call.args[0], _key_span, key_name, field_names)
            } else {
                Err(syn::Error::new_spanned(
                    expr,
                    format!(
                        "`{key_name}` does not accept `Some(...)`; use the field name directly"
                    ),
                ))
            }
        }
        // Bare ident — must be a field
        Expr::Path(ep) if ep.qself.is_none() && ep.path.segments.len() == 1 => {
            let name = ep.path.segments[0].ident.to_string();
            if !field_names.contains(&name) {
                return Err(syn::Error::new_spanned(
                    expr,
                    format!("`{key_name} = {name}` — no field `{name}` in this accounts struct",),
                ));
            }
            Ok(())
        }
        // Multi-segment path (const) or literal — allowed for decimals, not for field refs
        Expr::Lit(_) => {
            // Literals are only valid for decimals.
            if key_name == "decimals" {
                Ok(())
            } else {
                Err(syn::Error::new_spanned(
                    expr,
                    format!("`{key_name}` must be a field reference, not a literal",),
                ))
            }
        }
        Expr::Path(_) => {
            // Multi-segment path (e.g., module::CONST) — allowed for decimals
            if key_name == "decimals" {
                Ok(())
            } else {
                Err(syn::Error::new_spanned(
                    expr,
                    format!(
                        "`{key_name}` must be a field name (bare identifier), not a path \
                         expression",
                    ),
                ))
            }
        }
        _ => Err(syn::Error::new_spanned(
            expr,
            format!("`{key_name}` must be a field reference",),
        )),
    }
}

// ---------------------------------------------------------------------------
// Per-field planning
// ---------------------------------------------------------------------------

fn plan_field(
    sem: &FieldSemantics,
    programs: &[ProgramCandidate],
    payer_field: Option<&Ident>,
) -> syn::Result<FieldPlan> {
    let mut pre_load = Vec::new();
    let mut post_load = Vec::new();
    let mut epilogue = Vec::new();

    // Resolve payer for this field (needed by init/realloc/migration).
    let resolved_payer = resolve_field_payer(sem, payer_field);

    // Pre-load: address verification for init fields.
    if sem.has_init() {
        if let Some(addr_expr) = &sem.address {
            pre_load.push(PreLoadStep::VerifyAddress(AddressSpec {
                expr: addr_expr.clone(),
            }));
        }
    }

    // Pre-load: init plan.
    if let Some(init) = &sem.init {
        if resolved_payer.is_none() {
            return Err(syn::Error::new_spanned(
                &sem.core.field,
                "init requires `payer = ...` (or add a field named `payer`)",
            ));
        }
        let init_plan = plan_init(sem, init.idempotent, &resolved_payer, programs)?;
        pre_load.push(PreLoadStep::Init(init_plan));
    }

    // Post-load: classify groups and build check/exit specs.
    let mut check_groups: Vec<(&GroupDirective, GroupKind)> = Vec::new();
    let mut exit_groups: Vec<(&GroupDirective, GroupKind)> = Vec::new();

    for group in &sem.groups {
        match group.kind.op_kind() {
            super::model::OpKind::Check => check_groups.push((group, group.kind)),
            super::model::OpKind::Exit => exit_groups.push((group, group.kind)),
        }
    }

    // Post-load: constraint checks.
    for (group, kind) in &check_groups {
        match kind {
            GroupKind::Token => {
                post_load.push(PostLoadStep::TokenCheck(plan_token_check(
                    group, &sem.core, programs,
                )?));
            }
            GroupKind::Mint => {
                post_load.push(PostLoadStep::MintCheck(plan_mint_check(
                    group, &sem.core, programs,
                )?));
            }
            GroupKind::AssociatedToken => {
                post_load.push(PostLoadStep::AssociatedTokenCheck(
                    plan_associated_token_check(group, &sem.core, programs)?,
                ));
            }
            _ => {}
        }
    }

    // Post-load: realloc.
    if let Some(realloc_expr) = &sem.realloc {
        let payer = match resolved_payer.as_ref() {
            Some(p) => p,
            None => {
                return Err(syn::Error::new_spanned(
                    &sem.core.field,
                    "`realloc = ...` requires `payer = ...` (or add a field named `payer`)",
                ));
            }
        };
        post_load.push(PostLoadStep::Realloc(ReallocSpec {
            new_space: realloc_expr.clone(),
            payer: payer.clone(),
        }));
    }

    // Post-load: migration grow.
    if sem.is_migration && sem.core.is_mut {
        let payer = match resolved_payer.as_ref() {
            Some(p) => p,
            None => {
                return Err(syn::Error::new_spanned(
                    &sem.core.field,
                    "`Migration<From, To>` requires `payer = ...` (or add a field named `payer`)",
                ));
            }
        };
        post_load.push(PostLoadStep::MigrationGrow(MigrationSpec {
            payer: payer.clone(),
        }));
    }

    // Post-load: address verification for non-init fields.
    if !sem.has_init() {
        if let Some(addr_expr) = &sem.address {
            post_load.push(PostLoadStep::VerifyExistingAddress(AddressSpec {
                expr: addr_expr.clone(),
            }));
        }
    }

    // Epilogue: exit actions sorted (sweep before close).
    exit_groups.sort_by_key(|(_, kind)| kind.exit_order());
    for (group, kind) in &exit_groups {
        match kind {
            GroupKind::Sweep => {
                epilogue.push(EpilogueStep::TokenSweep(plan_sweep(
                    group, &sem.core, programs,
                )?));
            }
            GroupKind::Close => {
                epilogue.push(plan_close(group, &sem.core, programs)?);
            }
            _ => {}
        }
    }

    // Epilogue: migration verify + normalize.
    if sem.is_migration {
        // Payer error already raised in the migration grow step above.
        if let Some(payer) = resolved_payer.as_ref() {
            epilogue.push(EpilogueStep::MigrationVerifyAndNormalize(MigrationSpec {
                payer: payer.clone(),
            }));
        }
    }

    Ok(FieldPlan {
        pre_load,
        post_load,
        epilogue,
    })
}

// ---------------------------------------------------------------------------
// Payer resolution
// ---------------------------------------------------------------------------

/// Find the struct-wide payer field (by name convention).
fn find_payer_field(semantics: &[FieldSemantics]) -> Option<Ident> {
    semantics
        .iter()
        .find(|sem| sem.core.ident == "payer" && sem.core.kind == FieldKind::Single)
        .map(|sem| sem.core.ident.clone())
}

/// Resolve payer for a specific field: explicit > inferred by name.
fn resolve_field_payer(sem: &FieldSemantics, payer_field: Option<&Ident>) -> Option<FieldRef> {
    if let Some(explicit_payer) = &sem.payer {
        return Some(FieldRef {
            ident: explicit_payer.clone(),
        });
    }

    let needs_payer = sem.init.is_some() || sem.is_migration || sem.realloc.is_some();
    if needs_payer {
        if let Some(payer_ident) = payer_field {
            return Some(FieldRef {
                ident: payer_ident.clone(),
            });
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Init planning
// ---------------------------------------------------------------------------

fn plan_init(
    sem: &FieldSemantics,
    idempotent: bool,
    resolved_payer: &Option<FieldRef>,
    programs: &[ProgramCandidate],
) -> syn::Result<InitPlan> {
    let payer = resolved_payer
        .as_ref()
        .ok_or_else(|| {
            syn::Error::new_spanned(
                &sem.core.field,
                "init requires `payer = ...` (or add a field named `payer`)",
            )
        })?
        .clone();

    // Find the init contributor group (token/mint/associated_token with init).
    let contributor = sem.groups.iter().find(|group| {
        matches!(
            group.kind,
            GroupKind::Token | GroupKind::Mint | GroupKind::AssociatedToken
        )
    });

    // No contributor → plain program init.
    let Some(group) = contributor else {
        return Ok(InitPlan::Program(ProgramInitSpec {
            payer,
            space: SpaceSpec::FromType(sem.core.effective_ty.clone()),
            idempotent,
        }));
    };

    match group.kind {
        GroupKind::Token => {
            let mint = require_account_ref(group, "mint", &sem.core)?;
            let authority = require_account_ref(group, "authority", &sem.core)?;
            let token_program = resolve_program_for_init(group, &sem.core, programs)?;
            Ok(InitPlan::Token(TokenInitSpec {
                payer,
                mint,
                authority,
                token_program,
                idempotent,
            }))
        }
        GroupKind::Mint => {
            let decimals = match find_arg_opt(&group.args, "decimals") {
                Some(arg) => MaybeDefault::Explicit(arg.value.clone()),
                None => MaybeDefault::Defaulted(syn::parse_quote! { 6u8 }),
            };
            let authority = require_account_ref(group, "authority", &sem.core)?;
            let freeze_authority = match find_arg_opt(&group.args, "freeze_authority") {
                Some(arg) => MaybeDefault::Explicit(parse_freeze_authority(&arg.value)),
                None => MaybeDefault::Defaulted(FreezeAuthoritySpec::None),
            };
            let token_program = resolve_program_for_init(group, &sem.core, programs)?;
            Ok(InitPlan::Mint(MintInitSpec {
                payer,
                decimals,
                authority,
                freeze_authority,
                token_program,
                idempotent,
            }))
        }
        GroupKind::AssociatedToken => {
            let mint = require_account_ref(group, "mint", &sem.core)?;
            let authority = require_account_ref(group, "authority", &sem.core)?;
            let token_program = resolve_program_for_init(group, &sem.core, programs)?;
            let system_program = resolve_simple_program_for_op(
                group,
                "system_program",
                ProgramCategory::System,
                "Program<SystemProgram>",
                &sem.core,
                programs,
            )?;
            let ata_program = resolve_simple_program_for_op(
                group,
                "ata_program",
                ProgramCategory::Ata,
                "Program<AssociatedTokenProgram>",
                &sem.core,
                programs,
            )?;
            Ok(InitPlan::AssociatedToken(AssociatedTokenInitSpec {
                payer,
                mint,
                authority,
                token_program,
                system_program,
                ata_program,
                idempotent,
            }))
        }
        _ => unreachable!("filtered to Token/Mint/AssociatedToken above"),
    }
}

// ---------------------------------------------------------------------------
// Check planning
// ---------------------------------------------------------------------------

fn plan_token_check(
    group: &GroupDirective,
    core: &FieldCore,
    programs: &[ProgramCandidate],
) -> syn::Result<TokenCheckSpec> {
    let mint = require_account_ref(group, "mint", core)?;
    let authority = require_account_ref(group, "authority", core)?;
    let token_program = resolve_token_program_for_check(group, core, programs)?;
    Ok(TokenCheckSpec {
        mint,
        authority,
        token_program,
    })
}

fn plan_mint_check(
    group: &GroupDirective,
    core: &FieldCore,
    programs: &[ProgramCandidate],
) -> syn::Result<MintCheckSpec> {
    let authority = require_account_ref(group, "authority", core)?;
    let decimals = match find_arg_opt(&group.args, "decimals") {
        Some(arg) => CheckMode::Check(arg.value.clone()),
        None => CheckMode::DoNotCheck,
    };
    let freeze_authority = match find_arg_opt(&group.args, "freeze_authority") {
        Some(arg) => CheckMode::Check(parse_freeze_authority(&arg.value)),
        None => CheckMode::DoNotCheck,
    };
    let token_program = resolve_token_program_for_check(group, core, programs)?;
    Ok(MintCheckSpec {
        authority,
        decimals,
        freeze_authority,
        token_program,
    })
}

fn plan_associated_token_check(
    group: &GroupDirective,
    core: &FieldCore,
    programs: &[ProgramCandidate],
) -> syn::Result<AssociatedTokenCheckSpec> {
    let mint = require_account_ref(group, "mint", core)?;
    let authority = require_account_ref(group, "authority", core)?;
    let token_program = resolve_token_program_for_check(group, core, programs)?;
    Ok(AssociatedTokenCheckSpec {
        mint,
        authority,
        token_program,
    })
}

// ---------------------------------------------------------------------------
// Exit planning
// ---------------------------------------------------------------------------

fn plan_close(
    group: &GroupDirective,
    core: &FieldCore,
    programs: &[ProgramCandidate],
) -> syn::Result<EpilogueStep> {
    let dest = require_account_ref(group, "dest", core)?;

    if find_arg_opt(&group.args, "authority").is_some() {
        // Token close: has authority.
        let authority = require_account_ref(group, "authority", core)?;
        let token_program = resolve_program_for_exit(group, "token_program", core, programs)?;
        Ok(EpilogueStep::TokenClose(TokenCloseSpec {
            destination: dest,
            authority,
            token_program,
        }))
    } else {
        // Program close: no authority.
        Ok(EpilogueStep::ProgramClose(ProgramCloseSpec {
            destination: dest,
        }))
    }
}

fn plan_sweep(
    group: &GroupDirective,
    core: &FieldCore,
    programs: &[ProgramCandidate],
) -> syn::Result<TokenSweepSpec> {
    let receiver = require_account_ref(group, "receiver", core)?;
    let mint = require_account_ref(group, "mint", core)?;
    let authority = require_account_ref(group, "authority", core)?;
    let token_program = resolve_program_for_exit(group, "token_program", core, programs)?;
    Ok(TokenSweepSpec {
        receiver,
        mint,
        authority,
        token_program,
    })
}

// ---------------------------------------------------------------------------
// Program inference (moved from lower.rs)
// ---------------------------------------------------------------------------

/// Program category for inference resolution.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProgramCategory {
    Token,
    Token2022,
    TokenInterface,
    System,
    Ata,
}

impl ProgramCategory {
    const fn type_name(self) -> &'static str {
        match self {
            Self::Token => "Program<TokenProgram>",
            Self::Token2022 => "Program<Token2022Program>",
            Self::TokenInterface => "Interface<TokenInterface>",
            Self::System => "Program<SystemProgram>",
            Self::Ata => "Program<AssociatedTokenProgram>",
        }
    }
}

const TOKEN_CATEGORIES: [ProgramCategory; 3] = [
    ProgramCategory::Token,
    ProgramCategory::Token2022,
    ProgramCategory::TokenInterface,
];

/// A program field candidate for inference.
struct ProgramCandidate {
    ident: Ident,
    category: ProgramCategory,
}

/// Scan all fields for program type candidates.
fn scan_program_candidates(semantics: &[FieldSemantics]) -> Vec<ProgramCandidate> {
    semantics
        .iter()
        .filter_map(|sem| {
            let cat = classify_program(
                &sem.core.effective_ty,
                sem.core.optional,
                sem.core.dup,
                sem.core.kind,
            )?;
            Some(ProgramCandidate {
                ident: sem.core.ident.clone(),
                category: cat,
            })
        })
        .collect()
}

fn classify_program(
    effective_ty: &Type,
    optional: bool,
    dup: bool,
    kind: FieldKind,
) -> Option<ProgramCategory> {
    if optional || dup || kind != FieldKind::Single {
        return None;
    }
    if let Some(inner) = extract_generic_inner_type(effective_ty, "Program") {
        return classify_program_inner(inner);
    }
    if let Some(inner) = extract_generic_inner_type(effective_ty, "Interface") {
        if let Some(name) = last_segment_name(inner) {
            if name == concat!("Token", "Interface") {
                return Some(ProgramCategory::TokenInterface);
            }
        }
    }
    None
}

fn classify_program_inner(inner: &Type) -> Option<ProgramCategory> {
    let name = last_segment_name(inner)?;
    if name == concat!("Token", "Program") {
        Some(ProgramCategory::Token)
    } else if name == concat!("Token2022", "Program") {
        Some(ProgramCategory::Token2022)
    } else if name == concat!("System", "Program") {
        Some(ProgramCategory::System)
    } else if name == concat!("Associated", "Token", "Program") {
        Some(ProgramCategory::Ata)
    } else {
        None
    }
}

/// Determine which token program categories are compatible with the field type.
fn compatible_token_categories(effective_ty: &Type) -> &'static [ProgramCategory] {
    if let Some(inner) = extract_generic_inner_type(effective_ty, "Account") {
        if let Some(name) = last_segment_name(inner) {
            if name == "Token" || name == "Mint" {
                return &[ProgramCategory::Token];
            }
            if name == "Token2022" || name == "Mint2022" {
                return &[ProgramCategory::Token2022];
            }
        }
    }
    if extract_generic_inner_type(effective_ty, "InterfaceAccount").is_some() {
        return &[
            ProgramCategory::TokenInterface,
            ProgramCategory::Token,
            ProgramCategory::Token2022,
        ];
    }
    &[
        ProgramCategory::TokenInterface,
        ProgramCategory::Token,
        ProgramCategory::Token2022,
    ]
}

/// Returns true if the account type is a concrete token/mint type whose owner
/// is already validated by AccountLoad — no runtime program field needed for
/// checks.
fn is_concrete_token_type(effective_ty: &Type) -> bool {
    if let Some(inner) = extract_generic_inner_type(effective_ty, "Account") {
        if let Some(name) = last_segment_name(inner) {
            return matches!(name.as_str(), "Token" | "Mint" | "Token2022" | "Mint2022");
        }
    }
    false
}

/// Resolve token program for a CHECK operation.
/// Concrete accounts → ConcreteOwner (no runtime field needed).
/// Interface accounts → resolve from candidates.
/// Explicit override always wins.
fn resolve_token_program_for_check(
    group: &GroupDirective,
    core: &FieldCore,
    candidates: &[ProgramCandidate],
) -> syn::Result<TokenProgramCheckRef> {
    // Explicit override wins — but must be a token-compatible program.
    if let Some(arg) = find_arg_opt(&group.args, "token_program") {
        let ident = require_field_ident(&arg.value, "token_program", core)?;
        validate_program_override_category(&ident, candidates, core, &TOKEN_CATEGORIES)?;
        return Ok(TokenProgramCheckRef::RuntimeField(ident));
    }

    // Concrete account → owner proof, no runtime field needed.
    if is_concrete_token_type(&core.effective_ty) {
        return Ok(TokenProgramCheckRef::ConcreteOwner);
    }

    // Interface account → must resolve from candidates.
    let ident = resolve_token_program_field(candidates, &core.effective_ty, &core.field)?;
    Ok(TokenProgramCheckRef::RuntimeField(ident))
}

/// Resolve token program for an INIT operation (needs runtime field for CPI).
fn resolve_program_for_init(
    group: &GroupDirective,
    core: &FieldCore,
    candidates: &[ProgramCandidate],
) -> syn::Result<ProgramRef> {
    // Explicit override — must be token-compatible.
    if let Some(arg) = find_arg_opt(&group.args, "token_program") {
        let ident = require_field_ident(&arg.value, "token_program", core)?;
        validate_program_override_category(&ident, candidates, core, &TOKEN_CATEGORIES)?;
        return Ok(ProgramRef { ident });
    }

    // Infer.
    let ident = resolve_token_program_field(candidates, &core.effective_ty, &core.field)?;
    Ok(ProgramRef { ident })
}

/// Resolve token program for an EXIT operation (needs runtime field for CPI).
fn resolve_program_for_exit(
    group: &GroupDirective,
    key: &str,
    core: &FieldCore,
    candidates: &[ProgramCandidate],
) -> syn::Result<ProgramRef> {
    // Explicit override — must be token-compatible.
    if let Some(arg) = find_arg_opt(&group.args, key) {
        let ident = require_field_ident(&arg.value, key, core)?;
        validate_program_override_category(&ident, candidates, core, &TOKEN_CATEGORIES)?;
        return Ok(ProgramRef { ident });
    }

    // Infer.
    let ident = resolve_token_program_field(candidates, &core.effective_ty, &core.field)?;
    Ok(ProgramRef { ident })
}

/// Resolve a simple (non-token) program for an op. Used for system_program,
/// ata_program.
fn resolve_simple_program_for_op(
    group: &GroupDirective,
    key: &str,
    category: ProgramCategory,
    type_name: &str,
    core: &FieldCore,
    candidates: &[ProgramCandidate],
) -> syn::Result<ProgramRef> {
    // Explicit override — must match expected category.
    if let Some(arg) = find_arg_opt(&group.args, key) {
        let ident = require_field_ident(&arg.value, key, core)?;
        validate_program_override_category(&ident, candidates, core, &[category])?;
        return Ok(ProgramRef { ident });
    }

    // Infer.
    let filtered: Vec<&ProgramCandidate> = candidates
        .iter()
        .filter(|c| c.category == category)
        .collect();
    match filtered.len() {
        0 => Err(syn::Error::new_spanned(
            &core.field,
            format!(
                "no `{type_name}` field found. Add one to the accounts struct, or specify `{key} \
                 = ...` explicitly. Program fields inside composite accounts are not considered",
            ),
        )),
        1 => Ok(ProgramRef {
            ident: filtered[0].ident.clone(),
        }),
        _ => Err(syn::Error::new_spanned(
            &core.field,
            format!("multiple `{type_name}` fields found — specify `{key} = ...` explicitly",),
        )),
    }
}

/// Core token program resolution: filter by compatibility, apply interface
/// priority.
fn resolve_token_program_field(
    candidates: &[ProgramCandidate],
    effective_ty: &Type,
    span: &syn::Field,
) -> syn::Result<Ident> {
    let compatible = compatible_token_categories(effective_ty);
    let filtered: Vec<&ProgramCandidate> = candidates
        .iter()
        .filter(|c| compatible.contains(&c.category))
        .collect();

    // Interface priority for InterfaceAccount fields.
    let is_interface_account =
        extract_generic_inner_type(effective_ty, "InterfaceAccount").is_some();
    if is_interface_account {
        let interfaces: Vec<&ProgramCandidate> = filtered
            .iter()
            .filter(|c| c.category == ProgramCategory::TokenInterface)
            .copied()
            .collect();
        if interfaces.len() == 1 {
            return Ok(interfaces[0].ident.clone());
        }
        if interfaces.len() > 1 {
            return Err(syn::Error::new_spanned(
                span,
                "multiple `Interface<TokenInterface>` fields found — specify `token_program = \
                 ...` explicitly",
            ));
        }
        // No interface → fall through to concrete programs.
    }

    match filtered.len() {
        0 => Err(syn::Error::new_spanned(
            span,
            "no compatible token program field found. Add a `Program<TokenProgram>`, \
             `Program<Token2022Program>`, or `Interface<TokenInterface>` field, or specify \
             `token_program = ...` explicitly. Program fields inside composite accounts are not \
             considered",
        )),
        1 => Ok(filtered[0].ident.clone()),
        _ => {
            let names: Vec<String> = filtered.iter().map(|c| c.ident.to_string()).collect();
            Err(syn::Error::new_spanned(
                span,
                format!(
                    "ambiguous token program — found {}. Specify `token_program = ...` explicitly",
                    names.join(", "),
                ),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Rent planning
// ---------------------------------------------------------------------------

fn compute_rent_plan(semantics: &[FieldSemantics]) -> RentPlan {
    let needs_rent = semantics
        .iter()
        .any(|sem| sem.init.is_some() || sem.realloc.is_some() || sem.is_migration);

    if !needs_rent {
        return RentPlan::NotNeeded;
    }

    for sem in semantics {
        if sem.core.optional {
            continue;
        }
        if let Type::Path(tp) = &sem.core.effective_ty {
            if let Some(last) = tp.path.segments.last() {
                if last.ident == "Sysvar" {
                    if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                        for arg in &args.args {
                            if let syn::GenericArgument::Type(Type::Path(inner)) = arg {
                                if inner
                                    .path
                                    .segments
                                    .last()
                                    .is_some_and(|s| s.ident == "Rent")
                                {
                                    return RentPlan::FromSysvarField {
                                        field: sem.core.ident.clone(),
                                    };
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    RentPlan::FetchOnce
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_arg_opt<'a>(args: &'a [GroupArg], key: &str) -> Option<&'a GroupArg> {
    args.iter().find(|a| a.key == key)
}

/// Extract a required AccountRef from a group arg, or produce a compile error.
fn require_account_ref(
    group: &GroupDirective,
    key: &str,
    core: &FieldCore,
) -> syn::Result<AccountRef> {
    let arg = find_arg_opt(&group.args, key).ok_or_else(|| {
        let kind_name = group
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        syn::Error::new_spanned(
            &core.field,
            format!("`{kind_name}(...)` requires `{key} = ...`"),
        )
    })?;

    if let Some(ident) = expr_as_ident(&arg.value) {
        Ok(AccountRef::field(ident))
    } else {
        Ok(AccountRef::expr(arg.value.clone()))
    }
}

/// Parse a freeze_authority value into FreezeAuthoritySpec.
fn parse_freeze_authority(expr: &Expr) -> FreezeAuthoritySpec {
    match expr {
        Expr::Path(ep)
            if ep.qself.is_none()
                && ep.path.segments.len() == 1
                && ep.path.segments[0].ident == "None" =>
        {
            FreezeAuthoritySpec::None
        }
        Expr::Call(call)
            if matches!(&*call.func, Expr::Path(p)
                if p.path.segments.len() == 1 && p.path.segments[0].ident == "Some")
                && call.args.len() == 1 =>
        {
            let inner = &call.args[0];
            if let Some(ident) = expr_as_ident(inner) {
                FreezeAuthoritySpec::Some(AccountRef::field(ident))
            } else {
                FreezeAuthoritySpec::Some(AccountRef::expr(inner.clone()))
            }
        }
        _ => {
            if let Some(ident) = expr_as_ident(expr) {
                FreezeAuthoritySpec::Some(AccountRef::field(ident))
            } else {
                FreezeAuthoritySpec::Some(AccountRef::expr(expr.clone()))
            }
        }
    }
}

/// Validate that an explicit program override field matches one of the expected
/// program categories.
fn validate_program_override_category(
    ident: &Ident,
    candidates: &[ProgramCandidate],
    core: &FieldCore,
    expected: &[ProgramCategory],
) -> syn::Result<()> {
    let candidate = candidates.iter().find(|c| c.ident == *ident);
    match candidate {
        Some(c) => {
            if !expected.contains(&c.category) {
                let names: Vec<_> = expected.iter().map(|e| e.type_name()).collect();
                return Err(syn::Error::new_spanned(
                    &core.field,
                    format!(
                        "`... = {ident}` — field `{ident}` is `{}`, expected one of: {}",
                        c.category.type_name(),
                        names.join(", "),
                    ),
                ));
            }
            Ok(())
        }
        None => Err(syn::Error::new_spanned(
            &core.field,
            format!(
                "field `{ident}` is not a recognized program type. Expected a `Program<...>` or \
                 `Interface<...>` field",
            ),
        )),
    }
}

/// Require that a program arg is a bare field identifier. Errors if it's a
/// multi-segment path or complex expression — explicit overrides must be
/// unambiguous field references.
fn require_field_ident(expr: &Expr, arg_name: &str, core: &FieldCore) -> syn::Result<Ident> {
    expr_as_ident(expr).ok_or_else(|| {
        syn::Error::new_spanned(
            &core.field,
            format!(
                "`{arg_name} = ...` must be a field name (bare identifier), not an expression",
            ),
        )
    })
}

/// Try to extract a single identifier from an expression.
fn expr_as_ident(expr: &Expr) -> Option<Ident> {
    if let Expr::Path(ep) = expr {
        if ep.qself.is_none() && ep.path.segments.len() == 1 {
            return Some(ep.path.segments[0].ident.clone());
        }
    }
    None
}

fn last_segment_name(ty: &Type) -> Option<String> {
    if let Type::Path(tp) = ty {
        tp.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}
