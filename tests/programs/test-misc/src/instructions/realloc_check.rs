use {crate::state::SimpleAccount, quasar_derive::Accounts, quasar_lang::prelude::*};

#[derive(Accounts)]
pub struct ReallocCheck {
    #[account(mut)]
    pub account: Account<SimpleAccount>,
    #[account(mut)]
    pub payer: Signer,
    pub system_program: Program<SystemProgram>,
}

impl ReallocCheck {
    #[inline(always)]
    pub fn handler(&mut self, new_space: u64) -> Result<(), ProgramError> {
        let new_space = new_space as usize;
        let min_space = <crate::state::SimpleAccount as quasar_lang::traits::Space>::SPACE;
        if new_space < min_space {
            return Err(ProgramError::AccountDataTooSmall);
        }
        self.account
            .realloc(new_space, self.payer.to_account_view(), None)
    }
}
