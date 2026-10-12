// accord_multisig (RFP-005 prototype) for LEZ v0.3.
//
// The whole program is `accord_core::{plan, apply}`; this binary only
// hands them to the v0.3 entry point.

fn main() {
    lee_core::program::run_program::<_, _, [u8], Vec<u8>>(accord_core::plan, accord_core::apply)
}
