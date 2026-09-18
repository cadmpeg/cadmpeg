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
    Ok(plan.write_to(writer)?.write_path().clone())
}

/// Write a length-prefixed ASCII string: a little-endian `u32` byte count then
/// the bytes.
pub(crate) fn lp_ascii(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(
        &u32::try_from(value.len())
            .expect("test ASCII string length")
            .to_le_bytes(),
    );
    out.extend_from_slice(value.as_bytes());
}

/// Write a length-prefixed UTF-16 string: a little-endian `u32` code-unit count
/// then the little-endian units.
pub(crate) fn lp_utf16(out: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    out.extend_from_slice(
        &u32::try_from(units.len())
            .expect("test UTF-16 unit count")
            .to_le_bytes(),
    );
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

/// Write a same-document reference to a `u64` target: the presence byte, the
/// target, then the two zero bytes that close the reference.
pub(crate) fn push_reference_u64(out: &mut Vec<u8>, target: u64) {
    out.push(1);
    out.extend_from_slice(&target.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
}

/// A same-document reference record to `target`.
pub(crate) fn local_reference(target: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_reference_u64(&mut bytes, target);
    bytes
}

/// A cross-document reference record: the presence byte, the target, the
/// cross-document marker, the source type GUID, the link type GUID, and the
/// link name.
pub(crate) fn cross_document_reference(target: u64, link_name: &str) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(crate::bytes::lp_utf16_bytes(
        "11111111-2222-3333-4444-555555555555",
    ));
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    bytes.extend(crate::bytes::lp_utf16_bytes(link_name));
    bytes.push(0);
    bytes
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

pub(crate) mod native_test;
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
