// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic IGES byte-fixture builders for crate tests.
#![allow(clippy::unwrap_used)]

mod test_cards;
mod test_curves_and_surfaces;
mod test_drawing_and_trimming;
mod test_owned;
mod test_procedural_surfaces;
mod test_solids_and_structure;
mod test_tabulated_surfaces;

pub(crate) use test_cards::*;
pub(crate) use test_curves_and_surfaces::*;
pub(crate) use test_drawing_and_trimming::*;
pub(crate) use test_owned::*;
pub(crate) use test_procedural_surfaces::*;
pub(crate) use test_solids_and_structure::*;
pub(crate) use test_tabulated_surfaces::*;

/// Plans a write at one Fixed ASCII target, the request the command line
/// builds for an explicit `--iges-target`.
///
/// The tests here assert what the writer produces at a version, not how the
/// request that names it is spelled, so the spelling lives in one place.
pub(crate) fn plan_at<'a>(
    version: crate::IgesVersion,
    ir: &'a cadmpeg_ir::CadIr,
    fidelity: Option<&'a cadmpeg_ir::SourceFidelity>,
) -> Result<cadmpeg_ir::codec::ExportPlan<'a>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::codec::{EncodeInput, Encoder, TargetRequest};

    crate::IgesEncoder::new(crate::IgesWriteOptions { version }).plan(
        EncodeInput { ir, fidelity },
        TargetRequest::Explicit(version.target()),
    )
}
