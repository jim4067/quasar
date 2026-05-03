//! Compile-pass: existing metadata account with behavior check constraints.
#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;
use quasar_metadata::{accounts::metadata, *};

solana_address::declare_id!("11111111111111111111111111111112");

#[derive(Accounts)]
pub struct ValidateMetadata {
    pub metadata_program: Program<MetadataProgram>,
    pub mint: UncheckedAccount,

    #[account(metadata(program = metadata_program, mint = mint))]
    pub metadata: Account<MetadataAccount>,
}

fn main() {}
