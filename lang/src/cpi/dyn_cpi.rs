//! Dynamic CPI builder with runtime-tracked account and data lengths.
//!
//! `DynCpiCall` is the variable-length counterpart to [`super::CpiCall`].
//! Both accounts and data are backed by `MaybeUninit` stack arrays with
//! compile-time capacity, while the active count is tracked at runtime.
//! This replaces `BufCpiCall` which only supported variable data.

use {
    super::{
        cpi_account_from_view, get_cpi_return, invoke_raw, result_from_raw, CpiReturn,
        InstructionAccount, Seed, Signer,
    },
    crate::utils::hint::unlikely,
    core::mem::MaybeUninit,
    solana_account_view::AccountView,
    solana_address::Address,
    solana_instruction_view::cpi::CpiAccount,
    solana_program_error::{ProgramError, ProgramResult},
};

// Safety: element types stored in MaybeUninit arrays must not need dropping.
// If upstream ever adds Drop impls, these assertions catch it at compile time.
const _: () = assert!(!core::mem::needs_drop::<InstructionAccount>());
const _: () = assert!(!core::mem::needs_drop::<CpiAccount>());

/// Stack-allocated CPI builder with runtime-tracked account and data lengths.
///
/// Both the account list and instruction data use `MaybeUninit` arrays to
/// avoid zero-initialization costs. Accounts are pushed one at a time;
/// data is set all at once or written directly via `data_mut()`.
///
/// A compile-time assertion prevents monomorphizations that would overflow
/// the SVM 4 KiB stack frame.
///
/// # Type parameters
///
/// - `MAX_ACCTS`: maximum number of accounts (capacity, not initial count).
/// - `MAX_DATA`: maximum byte length of instruction data.
pub struct DynCpiCall<'a, const MAX_ACCTS: usize, const MAX_DATA: usize> {
    program_id: &'a Address,
    accounts: MaybeUninit<[InstructionAccount<'a>; MAX_ACCTS]>,
    cpi_accounts: MaybeUninit<[CpiAccount<'a>; MAX_ACCTS]>,
    acct_len: usize,
    data: MaybeUninit<[u8; MAX_DATA]>,
    data_len: usize,
}

impl<'a, const MAX_ACCTS: usize, const MAX_DATA: usize> DynCpiCall<'a, MAX_ACCTS, MAX_DATA> {
    // Compile-time stack overflow guard — fires at monomorphization time.
    // InstructionAccount is 24 bytes, CpiAccount is 56 bytes, plus data + bookkeeping.
    const _STACK_CHECK: () = assert!(
        56 * MAX_ACCTS + 24 * MAX_ACCTS + MAX_DATA + 24 <= 3072,
        "DynCpiCall exceeds safe 3 KiB stack budget for SVM 4 KiB frames"
    );

    /// Create a new builder targeting the given program.
    #[inline(always)]
    pub fn new(program_id: &'a Address) -> Self {
        // Force compile-time stack size check.
        let _ = Self::_STACK_CHECK;
        Self {
            program_id,
            // Stable MaybeUninit pattern (not nightly uninit_array).
            accounts: MaybeUninit::uninit(),
            cpi_accounts: MaybeUninit::uninit(),
            acct_len: 0,
            data: MaybeUninit::uninit(),
            data_len: 0,
        }
    }

    /// Push an account into the builder. Returns error if MAX_ACCTS exceeded.
    #[inline(always)]
    pub fn push_account(
        &mut self,
        view: &'a AccountView,
        is_signer: bool,
        is_writable: bool,
    ) -> Result<(), ProgramError> {
        if unlikely(self.acct_len >= MAX_ACCTS) {
            return Err(ProgramError::InvalidArgument);
        }
        // SAFETY: acct_len < MAX_ACCTS, so both writes are in bounds.
        // Uses the stable MaybeUninit::<[T; N]>::uninit() pattern --
        // as_mut_ptr() gives *mut [T; N], cast to *mut T for element access.
        unsafe {
            let acct_ptr = self.accounts.as_mut_ptr() as *mut InstructionAccount<'a>;
            let cpi_ptr = self.cpi_accounts.as_mut_ptr() as *mut CpiAccount<'a>;
            acct_ptr.add(self.acct_len).write(InstructionAccount {
                address: view.address(),
                is_signer,
                is_writable,
            });
            cpi_ptr
                .add(self.acct_len)
                .write(cpi_account_from_view(view));
        }
        self.acct_len += 1;
        Ok(())
    }

    /// Push an account without bounds checking.
    ///
    /// # Safety
    ///
    /// Caller must ensure `self.acct_len < MAX_ACCTS`.
    #[inline(always)]
    pub unsafe fn push_account_unchecked(
        &mut self,
        view: &'a AccountView,
        is_signer: bool,
        is_writable: bool,
    ) {
        // SAFETY: Caller guarantees acct_len < MAX_ACCTS.
        let acct_ptr = self.accounts.as_mut_ptr() as *mut InstructionAccount<'a>;
        let cpi_ptr = self.cpi_accounts.as_mut_ptr() as *mut CpiAccount<'a>;
        acct_ptr.add(self.acct_len).write(InstructionAccount {
            address: view.address(),
            is_signer,
            is_writable,
        });
        cpi_ptr
            .add(self.acct_len)
            .write(cpi_account_from_view(view));
        self.acct_len += 1;
    }

    /// Set instruction data. Overwrites any previous data.
    #[inline(always)]
    pub fn set_data(&mut self, data: &[u8]) -> Result<(), ProgramError> {
        if unlikely(data.len() > MAX_DATA) {
            return Err(ProgramError::InvalidInstructionData);
        }
        // SAFETY: data.len() <= MAX_DATA, so the copy is in bounds.
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                self.data.as_mut_ptr() as *mut u8,
                data.len(),
            );
        }
        self.data_len = data.len();
        Ok(())
    }

    /// Direct access to the data buffer for zero-copy writes.
    ///
    /// Avoids the double-copy of building data externally then calling
    /// `set_data()`. Caller must call `set_data_len()` after writing.
    #[inline(always)]
    pub fn data_mut(&mut self) -> &mut [u8; MAX_DATA] {
        // SAFETY: We expose the raw buffer for direct writes.
        // Caller must set data_len after writing via set_data_len().
        unsafe { &mut *self.data.as_mut_ptr() }
    }

    /// Set the active data length (after writing via `data_mut()`).
    #[inline(always)]
    pub fn set_data_len(&mut self, len: usize) -> Result<(), ProgramError> {
        if unlikely(len > MAX_DATA) {
            return Err(ProgramError::InvalidInstructionData);
        }
        self.data_len = len;
        Ok(())
    }

    /// Invoke the CPI without any PDA signers.
    #[inline(always)]
    pub fn invoke(&self) -> ProgramResult {
        self.invoke_inner(&[])
    }

    /// Invoke the CPI with a single PDA signer (seeds for one address).
    #[inline(always)]
    pub fn invoke_signed(&self, seeds: &[Seed]) -> ProgramResult {
        self.invoke_inner(&[Signer::from(seeds)])
    }

    /// Invoke the CPI with multiple PDA signers.
    #[inline(always)]
    pub fn invoke_with_signers(&self, signers: &[Signer]) -> ProgramResult {
        self.invoke_inner(signers)
    }

    /// Invoke the CPI and read back raw return data.
    #[inline(always)]
    pub fn invoke_with_return(&self) -> Result<CpiReturn, ProgramError> {
        self.invoke_with_return_inner(&[])
    }

    /// Invoke the CPI with one PDA signer and read back raw return data.
    #[inline(always)]
    pub fn invoke_signed_with_return(&self, seeds: &[Seed]) -> Result<CpiReturn, ProgramError> {
        self.invoke_with_return_inner(&[Signer::from(seeds)])
    }

    /// Invoke the CPI with multiple PDA signers and read back raw return data.
    #[inline(always)]
    pub fn invoke_with_signers_with_return(
        &self,
        signers: &[Signer],
    ) -> Result<CpiReturn, ProgramError> {
        self.invoke_with_return_inner(signers)
    }

    #[inline(always)]
    fn invoke_inner(&self, signers: &[Signer]) -> ProgramResult {
        // SAFETY: accounts[0..acct_len] and cpi_accounts[0..acct_len]
        // are initialized by push_account. data[0..data_len] written by
        // set_data or data_mut(). MaybeUninit<[T; N]> has same layout as [T; N].
        // We pass pointers with acct_len/data_len to invoke_raw, reading only
        // the initialized portion -- never assume_init() the whole array.
        let result = unsafe {
            invoke_raw(
                self.program_id,
                self.accounts.as_ptr() as *const InstructionAccount,
                self.acct_len,
                self.data.as_ptr() as *const u8,
                self.data_len,
                self.cpi_accounts.as_ptr() as *const CpiAccount,
                self.acct_len,
                signers,
            )
        };
        result_from_raw(result)
    }

    #[inline(always)]
    fn invoke_with_return_inner(&self, signers: &[Signer]) -> Result<CpiReturn, ProgramError> {
        crate::return_data::set_return_data(&[]);
        // SAFETY: Same as invoke_inner -- only initialized portions are read.
        let result = unsafe {
            invoke_raw(
                self.program_id,
                self.accounts.as_ptr() as *const InstructionAccount,
                self.acct_len,
                self.data.as_ptr() as *const u8,
                self.data_len,
                self.cpi_accounts.as_ptr() as *const CpiAccount,
                self.acct_len,
                signers,
            )
        };
        result_from_raw(result)?;
        let ret = get_cpi_return()?;
        if !crate::keys_eq(ret.program_id(), self.program_id) {
            return Err(crate::error::QuasarError::ReturnDataFromWrongProgram.into());
        }
        Ok(ret)
    }

    /// Debug accessor for instruction data (off-chain only).
    #[cfg(not(any(target_os = "solana", target_arch = "bpf")))]
    pub fn instruction_data(&self) -> &[u8] {
        // SAFETY: data[0..data_len] was initialized by set_data or data_mut().
        unsafe { core::slice::from_raw_parts(self.data.as_ptr() as *const u8, self.data_len) }
    }
}
