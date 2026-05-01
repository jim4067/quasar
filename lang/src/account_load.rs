use {
    crate::traits::AsAccountView, solana_account_view::AccountView,
    solana_program_error::ProgramError,
};

/// Unified validation, construction, and header flag consts for account wrapper
/// types.
///
/// All implementors must be `#[repr(transparent)]` over `AccountView`.
pub trait AccountLoad: AsAccountView + Sized {
    const IS_SIGNER: bool = false;
    const IS_EXECUTABLE: bool = false;

    // --- Migration hooks (override only in Migration<From, To>) ---
    const HAS_BEFORE_INIT: bool = false;
    const HAS_EXIT_VALIDATION: bool = false;

    /// Validate this account after header flag checks pass.
    ///
    /// Header flags (signer, writable, executable) are already validated by
    /// `parse_accounts` before this is called.
    fn check(view: &AccountView, field_name: &str) -> Result<(), ProgramError>;

    /// Pre-handler hook for migration types. Called after load, before handler.
    /// Default no-op for all non-migration types.
    #[inline(always)]
    fn before_init(
        &mut self,
        _payer: Option<&AccountView>,
        _ctx: &crate::ops::OpCtx<'_>,
    ) -> Result<(), ProgramError> {
        Ok(())
    }

    /// Post-handler validation hook for migration types. Called in epilogue.
    /// Default no-op for all non-migration types.
    #[inline(always)]
    fn exit_validation(
        &mut self,
        _payer: Option<&AccountView>,
        _ctx: &crate::ops::OpCtx<'_>,
    ) -> Result<(), ProgramError> {
        Ok(())
    }

    /// # Safety
    /// Caller must ensure the `AccountView` is valid for `#[repr(transparent)]`
    /// cast.
    #[inline(always)]
    unsafe fn from_view_unchecked(view: &AccountView) -> &Self {
        &*(view as *const AccountView as *const Self)
    }

    /// # Safety
    /// Same as `from_view_unchecked`, plus the account must be writable.
    #[inline(always)]
    unsafe fn from_view_unchecked_mut(view: &mut AccountView) -> &mut Self {
        &mut *(view as *mut AccountView as *mut Self)
    }

    #[inline(always)]
    fn load(view: &AccountView, field_name: &str) -> Result<Self, ProgramError> {
        Self::check(view, field_name)?;
        Ok(unsafe { core::ptr::read(Self::from_view_unchecked(view) as *const Self) })
    }

    #[inline(always)]
    fn load_mut(view: &mut AccountView, field_name: &str) -> Result<Self, ProgramError> {
        Self::check(view, field_name)?;
        Ok(unsafe { core::ptr::read(Self::from_view_unchecked_mut(view) as *const Self) })
    }

    /// Get a mutable view for lifecycle operations (close, realloc).
    ///
    /// # Safety
    ///
    /// Caller must ensure the account is writable and that no other
    /// references to the underlying `AccountView` are live. Only called
    /// by generated epilogue code after writable/lifecycle checks pass.
    #[doc(hidden)]
    #[inline(always)]
    unsafe fn to_account_view_mut(&mut self) -> &mut AccountView {
        &mut *(self as *mut Self as *mut AccountView)
    }
}
