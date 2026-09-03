// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.f3d` ZIP archives and ASM stream payloads.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::io::Write;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::{CadIr, SourceFidelity, WritePath};

use crate::F3dCodec;

/// Plans an inherited write through the sealed encoder and writes its bytes.
pub(crate) fn plan_inherited_write(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, CodecError> {
    let plan = F3dCodec.plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)?;
    Ok(plan.write_to(writer)?.write_path)
}

mod tokens_test;
pub(crate) use tokens_test::*;

mod smbh_header_test;
pub(crate) use smbh_header_test::*;

mod smbh_blocks_test;
pub(crate) use smbh_blocks_test::*;

mod smbh_geometry_test;
pub(crate) use smbh_geometry_test::*;

mod smbh_pcurves_test;
pub(crate) use smbh_pcurves_test::*;

mod smbh_curves_test;
pub(crate) use smbh_curves_test::*;

mod smbh_surfaces_test;
pub(crate) use smbh_surfaces_test::*;

mod smbh_revision_test;
pub(crate) use smbh_revision_test::*;

mod smbh_blends_test;
pub(crate) use smbh_blends_test::*;

mod smbh_bf4_test;
pub(crate) use smbh_bf4_test::*;

mod native_test;
pub(crate) use native_test::*;

mod manifest_test;
pub(crate) use manifest_test::*;

mod protein_test;
pub(crate) use protein_test::*;

mod streams_test;
pub(crate) use streams_test::*;

mod zip_test;
pub(crate) use zip_test::*;

mod assembly_test;
pub(crate) use assembly_test::*;

mod procedural_test;
pub(crate) use procedural_test::*;
