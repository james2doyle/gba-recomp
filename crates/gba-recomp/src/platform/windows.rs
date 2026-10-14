//! Windows adapter (SPEC-068). Not implemented yet: the target is
//! accepted by the LLVM emitter, but linking and packaging still need the
//! platform toolchain integration.

#[allow(unused_imports)]
use std::path::Path;

use super::{PackageInputs, Packaged};

pub(super) fn package(i: &PackageInputs<'_>) -> Result<Packaged, String> {
    Err(format!(
        "Windows packaging for {} is not implemented yet (SPEC-068)",
        i.spec.triple
    ))
}
