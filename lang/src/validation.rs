//! Runtime validation helpers for account constraint checks.
//!
//! Each function is `#[inline(always)]` and 5–15 lines — independently
//! auditable, independently testable. The derive macro generates calls
//! to these functions instead of inline `quote!` blocks, so an auditor
//! reads this file once and then verifies the macro just wires them.
//!
//! Debug logging: every check accepts a `_field: &str` parameter carrying
//! the field name from the accounts struct. In release builds the
//! `#[cfg(feature = "debug")]` blocks are stripped and LLVM eliminates
//! the parameter entirely — zero CU cost.

use {crate::utils::hint::unlikely, solana_address::Address, solana_program_error::ProgramError};

// ---------------------------------------------------------------------------
// Constraint checks (has_one, address, user constraint)
// ---------------------------------------------------------------------------

/// Validate that two addresses match (used for `has_one` and `address`
/// constraints — the check is identical).
#[inline(always)]
pub fn check_address_match(
    actual: &Address,
    expected: &Address,
    error: ProgramError,
) -> Result<(), ProgramError> {
    if unlikely(!crate::keys_eq(actual, expected)) {
        return Err(error);
    }
    Ok(())
}

/// Validate a user-defined boolean constraint.
#[inline(always)]
pub fn check_constraint(condition: bool, error: ProgramError) -> Result<(), ProgramError> {
    if unlikely(!condition) {
        return Err(error);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Kani model-checking proof harnesses
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// Prove `check_address_match` returns `Ok(())` when addresses are equal.
    #[kani::proof]
    fn check_address_match_equal_returns_ok() {
        let bytes: [u8; 32] = kani::any();
        let a = Address::new_from_array(bytes);
        let b = Address::new_from_array(bytes);
        assert!(check_address_match(&a, &b, ProgramError::InvalidArgument) == Ok(()));
    }

    /// Prove `check_address_match` returns the caller's exact error when
    /// addresses differ.
    #[kani::proof]
    fn check_address_match_unequal_returns_exact_error() {
        let a_bytes: [u8; 32] = kani::any();
        let b_bytes: [u8; 32] = kani::any();
        kani::assume(a_bytes != b_bytes);
        let a = Address::new_from_array(a_bytes);
        let b = Address::new_from_array(b_bytes);
        let code: u32 = kani::any();
        let error = ProgramError::Custom(code);
        assert!(check_address_match(&a, &b, error) == Err(ProgramError::Custom(code)));
    }

    /// Prove `check_constraint` returns `Ok(())` when condition is true.
    #[kani::proof]
    fn check_constraint_true_returns_ok() {
        let code: u32 = kani::any();
        let error = ProgramError::Custom(code);
        assert!(check_constraint(true, error) == Ok(()));
    }

    /// Prove `check_constraint` returns the caller's exact error when condition
    /// is false.
    #[kani::proof]
    fn check_constraint_false_returns_exact_error() {
        let code: u32 = kani::any();
        let error = ProgramError::Custom(code);
        assert!(check_constraint(false, error) == Err(ProgramError::Custom(code)));
    }
}
