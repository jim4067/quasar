#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_metadata::*;

solana_address::declare_id!("11111111111111111111111111111112");

/// ERROR: init on MasterEditionAccount without a behavior module.
#[derive(Accounts)]
pub struct BadInit {
    #[account(mut)]
    pub payer: Signer,

    pub system_program: Program<SystemProgram>,
    pub rent: Sysvar<Rent>,

    #[account(init, payer = payer)]
    pub master_edition: Account<MasterEditionAccount>,
}

fn main() {}
