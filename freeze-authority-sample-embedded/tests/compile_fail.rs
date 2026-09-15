//! Negative tests: programs that MUST fail to compile, with the exact
//! errors the framework promises. If a fixture ever compiles, the
//! guarantee it documents has regressed.
#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
