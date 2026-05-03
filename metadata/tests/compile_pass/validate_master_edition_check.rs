//! Compile-pass: existing master edition account with behavior check constraints.
#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_metadata::{accounts::master_edition, *};

solana_address::declare_id!("11111111111111111111111111111112");

#[derive(Accounts)]
pub struct ValidateMasterEdition {
    pub metadata_program: Program<MetadataProgram>,
    pub mint: UncheckedAccount,

    #[account(master_edition(program = metadata_program, mint = mint))]
    pub master_edition: Account<MasterEditionAccount>,
}

fn main() {}
