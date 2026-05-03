//! Compile-pass tests for metadata behavior modules.
//!
//! These verify that valid account declarations with metadata/master_edition
//! behaviors compile successfully.

#[test]
fn compile_pass_tests() {
    let t = trybuild::TestCases::new();
    t.pass("tests/compile_pass/*.rs");
}
