#![allow(unexpected_cfgs)]
use quasar_lang::prelude::*;

solana_address::declare_id!("11111111111111111111111111111112");

#[derive(Copy, Clone, QuasarSerialize)]
pub struct Metadata {
    pub label: PodString<16>,
    pub values: PodVec<u8, 4>,
    pub version: u32,
}

#[account(discriminator = 1, set_inner)]
pub struct Registry {
    pub meta: Metadata,
    pub bump: u8,
}

fn main() {}
