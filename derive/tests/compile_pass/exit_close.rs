//! Exit close via close account capability.
#![allow(unexpected_cfgs)]
extern crate alloc;

use {quasar_derive::Accounts, quasar_lang::prelude::*};

solana_address::declare_id!("11111111111111111111111111111112");

#[account(discriminator = 6)]
pub struct OldData {
    pub value: u64,
}

#[derive(Accounts)]
pub struct CloseAccounts {
    #[account(mut)]
    pub authority: Signer,

    #[account(mut,
        close(dest = authority),
    )]
    pub old_data: Account<OldData>,
}

fn main() {}
