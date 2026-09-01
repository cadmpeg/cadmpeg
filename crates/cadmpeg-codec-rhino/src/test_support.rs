// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic 3DM byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build archive bytes only; owner suites own the assertions.
#![allow(clippy::unwrap_used)]

mod test_archive;
pub(crate) mod test_dump;

pub(crate) use test_archive::*;

/// Plans a write at one archive version, the request the command line builds
/// for an explicit target.
///
/// The suites assert what the writer produces at a version, not how the request
/// that names it is spelled, so the spelling lives in one place. `Inherit` is a
/// different question — it resolves against the source and cannot be pinned per
/// version — and `writer/tests/targets.rs` owns it.
pub(crate) fn plan_at(
    version: crate::RhinoArchiveVersion,
    ir: &cadmpeg_ir::document::CadIr,
) -> Result<cadmpeg_ir::codec::ExportPlan, cadmpeg_core::CodecError> {
    use cadmpeg_ir::codec::{EncodeInput, Encoder, TargetRequest};

    crate::RhinoCodec.plan(
        EncodeInput::new(ir, None),
        TargetRequest::Explicit(version.descriptor().id.as_str()),
    )
}
