#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;

solana_address::declare_id!("11111111111111111111111111111112");

#[account(discriminator = 7)]
pub struct Data {
    pub value: u64,
}

#[derive(Accounts)]
pub struct BadCloseArgs {
    #[account(mut)]
    pub dest: Signer,

    #[account(mut, close(dest = dest, authority = dest))]
    pub data: Account<Data>,
}

fn main() {}
