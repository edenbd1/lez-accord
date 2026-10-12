// antumbra_quorum (RFP-005 prototype) for LEZ v0.3.
//
// The whole program is `antumbra_quorum_core::{plan, apply}`; this binary only
// hands them to the v0.3 entry point.

fn main() {
    lee_core::program::run_program::<_, _, [u8], Vec<u8>>(antumbra_quorum_core::plan, antumbra_quorum_core::apply)
}
