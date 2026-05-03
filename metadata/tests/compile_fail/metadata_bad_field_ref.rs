#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_metadata::*;
use quasar_metadata::accounts::metadata;

solana_address::declare_id!("11111111111111111111111111111112");

/// ERROR: `nonexistent_field` is not a field in the struct.
#[derive(Accounts)]
pub struct Bad {
    pub metadata_program: Program<MetadataProgram>,

    #[account(metadata(program = metadata_program, mint = nonexistent_field))]
    pub metadata: Account<MetadataAccount>,
}

fn main() {}
