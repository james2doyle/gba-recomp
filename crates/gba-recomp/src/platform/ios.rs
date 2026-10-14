//! iOS adapter (SPEC-068). Not implemented yet: the target is
//! accepted by the LLVM emitter, but linking and packaging still need the
//! platform toolchain integration.

#[allow(unused_imports)]
use std::path::Path;

use super::{PackageInputs, Packaged};

pub(super) fn package(i: &PackageInputs<'_>) -> Result<Packaged, String> {
    Err(format!(
        "iOS packaging for {} is not implemented yet (SPEC-068)",
        i.spec.triple
    ))
}

pub(super) fn finalize(
    _dir: &Path,
    _rom: &[u8],
    _sha_hex: &str,
    _embed: bool,
) -> Result<(), String> {
    Ok(())
}
