#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;

solana_address::declare_id!("11111111111111111111111111111112");

#[account(discriminator = 1)]
pub struct ConfigV1 {
    pub value: PodU64,
}

#[account(discriminator = 2)]
pub struct ConfigV2 {
    pub value: PodU64,
    pub extra: PodU32,
}

// ERROR: Option<Migration<...>> is not supported
#[derive(Accounts)]
pub struct Bad {
    #[account(mut)]
    pub payer: Signer,
    pub system_program: Program<SystemProgram>,

    #[account(mut, payer = payer)]
    pub config: Option<Migration<ConfigV1, ConfigV2>>,
}

fn main() {}
