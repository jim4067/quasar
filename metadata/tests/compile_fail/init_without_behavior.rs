#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_metadata::*;

solana_address::declare_id!("11111111111111111111111111111112");

#[derive(Accounts)]
pub struct BadInit {
    #[account(mut)]
    pub payer: Signer,

    pub system_program: Program<SystemProgram>,
    pub rent: Sysvar<Rent>,

    /// This should fail: init without a behavior module on a protocol account.
    #[account(init, payer = payer)]
    pub metadata: Account<MetadataAccount>,
}

fn main() {}
