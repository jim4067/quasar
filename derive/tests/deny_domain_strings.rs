//! Deny test: derive macro boundary enforcement.
//!
//! The derive owns SPL lowering in explicit SPL modules (specs, planner,
//! typed_emit). Generic modules (syntax, field classification, output) must
//! not contain literal SPL domain strings.
//!
//! SPL-aware modules are excluded from this check because they legitimately
//! reference SPL type/trait names for greppability.

/// Domain strings that must never appear in derive/src/accounts/ source.
///
/// This list is the single source of truth.
const BANNED: &[&str] = &[
    // ATA domain knowledge (Phase 3 deleted)
    "AssociatedTokenProgram",
    "find_field_by_inner_name",
    // Expression-shape dispatch (Phase 1 deleted)
    "is_raw_slot_safe",
    // Migration type detection (Phase 4 deleted)
    "is_migration_type",
    // SPL param types
    "TokenParams",
    "TokenInitKind",
    "MintParams",
    "MintInitParams",
    "SPL_TOKEN",
    "quasar_spl",
    // SPL trait names
    "TokenClose",
    "TokenSweep",
    "CloseCtx",
    "SweepCtx",
    // SPL function names
    "realloc_account",
    "init_token",
    "validate_token",
    "close_token",
    "sweep_token",
    "init_ata",
    "validate_ata",
    // SPL type names
    "AccountClose",
    "SupportsRealloc",
    // Domain names in program type detection
    "SystemProgram",
    // Old PDA machinery (deleted in Task 8)
    "SeedNode",
    "classify_seed",
];

fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
}

#[test]
fn deny_domain_strings_in_derive() {
    let derive_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/accounts");

    let mut files = Vec::new();
    collect_rs_files(&derive_src, &mut files);
    assert!(!files.is_empty(), "no .rs files found in {:?}", derive_src);

    // SPL-aware modules are allowed to use domain strings directly.
    // Generic modules must remain domain-free.
    let spl_modules: &[&str] = &[
        "resolve/specs.rs",
        "resolve/planner.rs",
        "emit/typed_emit.rs",
        "emit/parse.rs",
    ];

    let mut violations = Vec::new();

    for file in &files {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(file);
        let rel_str = rel.to_string_lossy();

        // Skip SPL-aware modules.
        if spl_modules.iter().any(|m| rel_str.contains(m)) {
            continue;
        }

        let content = std::fs::read_to_string(file).unwrap();
        for (line_num, line) in content.lines().enumerate() {
            // Skip comments
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            for term in BANNED {
                if line.contains(term) {
                    violations.push(format!("  {}:{}: `{}`", rel.display(), line_num + 1, term,));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Domain strings found in derive/src/accounts/:\n{}",
        violations.join("\n"),
    );
}
