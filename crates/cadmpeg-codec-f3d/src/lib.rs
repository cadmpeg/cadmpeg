// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write Autodesk Fusion `.f3d` archives.
//!
//! [`F3dCodec`] implements [`Codec`] and [`Encoder`]. Decoding produces a
//! [`CadIr`] document with B-rep topology, analytic and cached NURBS geometry,
//! body transforms, design and sketch records, construction history, and
//! appearances. Encoding replays an unchanged decoded archive byte for byte,
//! applies supported semantic edits to retained source data, or creates an
//! archive from the supported source-less profile.
//!
//! <!-- generated: capability -->
//! Support: L4 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#fusion-360-f3d)).
//! <!-- /generated: capability -->
//!
//! # Decode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::{CodecBackend, Codec, DecodeOptions};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! for loss in &result.report().losses {
//!     eprintln!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Codec::inspect`](cadmpeg_ir::Codec::inspect) classifies the ZIP entries and reads ASM B-rep headers
//! without building geometry. `DecodeOptions::container_only` provides the
//! corresponding metadata-only `CadIr`.
//!
//! # Encode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::codec::TargetRequest;
//! use cadmpeg_ir::{CodecBackend, Codec, DecodeOptions, Encoder};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! // Edit supported fields in result.ir().
//! let mut output = File::create("part-edited.f3d")?;
//! F3dCodec
//!     .plan(cadmpeg_ir::codec::EncodeInput {
//!         ir: result.ir(),
//!         fidelity: Some(result.source_fidelity()),
//!     }, TargetRequest::Inherit)?
//!     .write_to(&mut output)?;
//! # Ok(())
//! # }
//! ```
//!
//! Encoding with the retained `source_fidelity` sidecar replays or patches the
//! source archive. Omitting `fidelity` writes the source-less profile.
//!
//! # Data flow
//!
//! The Design body map selects every B-rep blob contributing bodies to the
//! document model; `.smb` and `.smbh` extensions do not choose either model or
//! history role. [`container`] locates history streams by the ASM header flag.
//! Without body-map bindings, a unique history-bearing stream or a single BREP
//! is the only fallback. [`cadmpeg_asm::sab`] frames each selected active
//! record slice.
//! [`brep`] builds each topology chain from bodies through vertices and points,
//! while [`cadmpeg_asm::nurbs`] decodes cached spline carriers.
//! [`design`], [`history`], and [`materials`] populate source-native records and
//! appearance bindings. [`f3z`] merges multi-document archives into one model.
//!
//! ASM model-space lengths become millimetres. Directions, ratios, angles,
//! knots, weights, and UV parameters retain their native scale.
//!
//! Inspect [`cadmpeg_ir::report::DecodeReport::losses`] before consuming a
//! decode. A stream that cannot produce geometry returns container metadata,
//! retained source data, and blocking geometry and topology losses. Referenced
//! carrier bytes needed for passthrough remain available as
//! [`cadmpeg_ir::unknown::UnknownRecord`] values.

mod act;
pub(crate) mod brep;
mod bytes;
#[allow(dead_code)] // Internal container records remain available to crate tests.
pub(crate) mod container;
pub(crate) mod decode;
pub(crate) mod design;
pub(crate) mod dialect;
mod error;
#[allow(dead_code)] // Multi-document helpers remain behind the codec facade.
pub(crate) mod f3z;
pub(crate) mod history;
mod history_records;
mod ids;
/// Byte-offset constants generated from `docs/layouts/f3d.toml`.
pub(crate) mod layout;
#[allow(dead_code)] // Loss catalog is consumed by tests and the writer.
pub(crate) mod loss;
mod manifest;
pub(crate) mod materials;
mod metastream;
mod native;
mod paramesh;
#[allow(dead_code)] // Native record surface remains behind the codec facade.
pub(crate) mod records;
mod tsm;
pub(crate) mod validate;
mod value_tree;
mod writer;
pub(crate) mod xref;
mod zip_write;

use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{
    CodecBackend, Confidence, DecodeResult, EncodeInput, Encoder, ExportPlan, TargetDescriptor,
    TargetRequest,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::WritePath;
use std::io::Write;

#[cfg(test)]
use crate::loss::F3dLossCode;

/// Validate the typed Fusion-native namespace.
pub fn validate_native(ir: &CadIr) -> Vec<cadmpeg_ir::Finding> {
    validate::validate_native(ir)
}

/// The ZIP local-file-header magic.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// The Autodesk Fusion `.f3d` container codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct F3dCodec;

impl F3dCodec {
    /// Write a decoded F3D document using its source-fidelity sidecar.
    #[cfg_attr(not(test), allow(dead_code))] // Crate-owned replay tests exercise this write door.
    #[allow(clippy::unused_self, clippy::trivially_copy_pass_by_ref)] // Preserve the tested method shape while narrowing visibility.
    pub(crate) fn write_preserved_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: &cadmpeg_ir::SourceFidelity,
        writer: &mut dyn Write,
    ) -> Result<WritePath, CodecError> {
        let record = source_fidelity
            .retained_record(ids::FILE_SOURCE_IMAGE_ID)
            .ok_or_else(|| {
                CodecError::NotImplemented("sidecar has no retained F3D source image".into())
            })?;
        let data = record.data.as_ref().ok_or_else(|| {
            CodecError::Malformed("retained F3D source image has no bytes".into())
        })?;
        Self::write_preserved_bytes(ir, data, record.byte_len, &record.sha256, writer)
    }

    /// Replay retained source bytes when the document baseline still matches;
    /// otherwise patch. Absent baseline: refuse. Returns which branch ran.
    fn write_preserved_bytes(
        ir: &CadIr,
        data: &[u8],
        byte_len: u64,
        sha256: &str,
        writer: &mut dyn Write,
    ) -> Result<WritePath, CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE))
            .ok_or_else(|| CodecError::NotImplemented("IR has no F3D document baseline".into()))?;
        let hash = sha256_hex(data);
        if data.len() as u64 != byte_len || hash != sha256 {
            return Err(CodecError::Malformed(
                "retained F3D source image failed integrity validation".into(),
            ));
        }
        if decode::document_local_sha256(ir) != *expected {
            writer::patch::write_semantic(ir, data, writer)?;
            return Ok(WritePath::Patched);
        }
        writer.write_all(data)?;
        Ok(WritePath::VerbatimReplay)
    }
}

impl CodecBackend for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if !prefix.starts_with(ZIP_MAGIC) {
            return Confidence::No;
        }
        // A ZIP alone is a weak signal (many formats are ZIPs). An f3d or f3z
        // marker string in the prefix — entry names are stored in cleartext in
        // ZIP local headers — makes it conclusive.
        if container::DETECT_MARKERS
            .iter()
            .chain(container::F3Z_DETECT_MARKERS)
            .any(|m| contains(prefix, m))
        {
            Confidence::High
        } else {
            Confidence::Low
        }
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(ctx, root)?;
        if f3z::is_f3z(&scan) {
            return f3z::inspect(ctx, &scan);
        }
        Ok(dialect::build_inspection_summary(&scan))
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(ctx, root)
    }
}

impl Encoder for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn targets(&self) -> &'static [TargetDescriptor] {
        dialect::TARGETS
    }

    fn plan<'a>(
        &self,
        input: EncodeInput<'a>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan<'a>, CodecError> {
        writer::target::plan(input, request)
    }
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
pub(crate) use cadmpeg_core::decode::InspectOptions;
#[cfg(test)]
pub(crate) use test_support::*;
#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
