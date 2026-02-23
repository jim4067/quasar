# Command to run profiler
CARGO_PROFILE_RELEASE_DEBUG=2 cargo build-sbf --manifest-path examples/escrow/Cargo.toml --lto && \
cargo run --release -p quasar-profile -- target/sbpf-solana-solana/release/quasar_escrow.so -o profile.svg