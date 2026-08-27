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
//! Support: depth L4, breadth 1 of >=1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#fusion-360-f3d)).
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
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{
    find_target, unsupported_target, CodecBackend, Confidence, DecodeResult, EncodeInput, Encoder,
    ExportPlan, Inherited, TargetDescriptor, TargetRequest,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::{FidelityResolution, WritePath};
use std::io::Write;

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
    pub fn write_preserved_with_source_fidelity(
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
        Ok(container::summarize(&scan))
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(ctx, root)
    }
}

/// The one dialect this writer synthesizes.
///
/// `manifest::write` pins the top-level manifest version to
/// `TOP_LEVEL_MANIFEST_VERSION`, so a generated archive can be no other row.
/// The multi-document F3Z row is reachable only by replaying a retained
/// archive, which is preservation, not synthesis.
const F3D_TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
    id: "f3d:manifest-3-2-0-0",
    label: "Fusion 360 archive with top-level manifest 3-2-0-0",
    aliases: &["3-2-0-0"],
    default: true,
}];

/// The dialect `writer::generate::write_new` produces, read off the catalog so
/// the generator and the catalog cannot name different rows.
const SYNTHESIS_TARGET: &str = F3D_TARGETS[0].id;

impl Encoder for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn targets(&self) -> &'static [TargetDescriptor] {
        F3D_TARGETS
    }

    /// Resolve the request against the source, then plan the export it names
    /// (design §8.2).
    ///
    /// `Explicit(id)` refuses an id outside the synthesis catalog, and is
    /// otherwise the replay law's compare: preserving the retained archive is
    /// eligible exactly when `id` is the source's dialect.
    ///
    /// `Inherit` asks for preservation instead: a valid retained baseline
    /// replays or patches whatever dialect the source is, the F3Z
    /// multi-document row and the recovery row included, which the generator
    /// could never synthesize. Where the baseline is not usable, `Inherit`
    /// synthesizes the source's own dialect, and refuses when that dialect is
    /// not a target. There is no fall-through to the catalog default: a
    /// same-format conversion never silently changes what the file is.
    ///
    /// The catalog default supplies the target only when there is nothing to
    /// inherit: the document has no source, or a source of another format.
    fn plan<'a>(
        &self,
        input: EncodeInput<'a>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan<'a>, CodecError> {
        match request {
            TargetRequest::Explicit(id) => {
                let target = find_target(F3D_TARGETS, id).ok_or_else(|| {
                    unsupported_target(
                        dialect::FORMAT,
                        id,
                        "not a target this encoder can synthesize",
                        F3D_TARGETS,
                    )
                })?;
                plan_explicit(input, &DialectId::pinned(target.id))
            }
            TargetRequest::Inherit => plan_inherited(input),
        }
    }
}

/// Plan a write at one synthesis target: preserve the retained archive when the
/// source is already in that dialect, otherwise synthesize it.
fn plan_explicit<'a>(
    input: EncodeInput<'a>,
    target: &DialectId,
) -> Result<ExportPlan<'a>, CodecError> {
    match preserve(input, target)? {
        Preservation::Written { bytes, write_path } => {
            Ok(preserved_plan(input.ir, target.clone(), write_path, bytes))
        }
        Preservation::Declined => synthesized_plan(input, target),
    }
}

/// Plan a write that preserves the source's dialect.
fn plan_inherited(input: EncodeInput<'_>) -> Result<ExportPlan<'_>, CodecError> {
    let source_dialect =
        match cadmpeg_ir::codec::resolve_inherit(input.ir, dialect::FORMAT, F3D_TARGETS)? {
            // Nothing to inherit: no source, or one of another format. The
            // catalog default stands in; no existing file's identity is at
            // stake.
            Inherited::Fallback(id) => return plan_explicit(input, &DialectId::pinned(id)),
            Inherited::Source(value) => value.clone(),
        };
    match preserve(input, &source_dialect)? {
        Preservation::Written { bytes, write_path } => {
            Ok(preserved_plan(input.ir, source_dialect, write_path, bytes))
        }
        Preservation::Declined if find_target(F3D_TARGETS, source_dialect.as_str()).is_some() => {
            synthesized_plan(input, &source_dialect)
        }
        Preservation::Declined => Err(unsupported_target(
            dialect::FORMAT,
            source_dialect.as_str(),
            "its retained source image is unavailable for preservation and the generator cannot \
             synthesize it",
            F3D_TARGETS,
        )),
    }
}

/// Outcome of the preservation decision.
enum Preservation {
    /// The retained archive, replayed verbatim or patched. Preservation
    /// reproduces the source's dialect, so the caller names that rather than a
    /// synthesis target.
    Written {
        bytes: Vec<u8>,
        write_path: WritePath,
    },
    Declined,
}

/// Decides preservation against three conditions, in order: the source is an
/// F3D document, its dialect is the one this export targets, and a retained
/// image is present.
///
/// `target` is the replay law's compare. Neither the verbatim branch nor the
/// patch branch rewrites `Manifest.dat`, so both restate the source's dialect
/// exactly; writing them under any other target would claim a dialect the bytes
/// are not. `Inherit` passes the source's own dialect, so the compare is
/// satisfied by construction and every dialect preserves, the F3Z row included.
///
/// Once all three hold this preserves or fails; it never declines to the
/// generator. A retained image with no document baseline is an unbuilt
/// capability, not a reason to regenerate: the question "was this edited since
/// it was decoded?" cannot be answered, so [`F3dCodec::write_preserved_bytes`]
/// refuses rather than guess.
fn preserve(input: EncodeInput<'_>, target: &DialectId) -> Result<Preservation, CodecError> {
    let Some(source) = input
        .ir
        .source
        .as_ref()
        .filter(|source| source.format == dialect::FORMAT)
    else {
        return Ok(Preservation::Declined);
    };
    if source.dialect.as_ref() != Some(target) {
        return Ok(Preservation::Declined);
    }
    let Some(record) = input
        .fidelity
        .and_then(|sidecar| sidecar.retained_record(ids::FILE_SOURCE_IMAGE_ID))
    else {
        return Ok(Preservation::Declined);
    };
    let Some(data) = record.data.as_deref() else {
        return Err(CodecError::Malformed(
            "retained F3D source image has no bytes".into(),
        ));
    };
    let mut bytes = Vec::new();
    let write_path = F3dCodec::write_preserved_bytes(
        input.ir,
        data,
        record.byte_len,
        &record.sha256,
        &mut bytes,
    )?;
    Ok(Preservation::Written { bytes, write_path })
}

fn preserved_plan(
    ir: &CadIr,
    target: DialectId,
    write_path: WritePath,
    bytes: Vec<u8>,
) -> ExportPlan<'_> {
    ExportPlan::buffered(
        report(
            ir,
            target,
            FidelityResolution::Replayed,
            write_path,
            Vec::new(),
        ),
        bytes,
    )
}

/// Plan a generated archive at a catalog row, charging the loss for a preserved
/// source image this export could not use.
///
/// `target` reaches here only after `find_target` admitted it, and the
/// generator pins the top-level manifest to [`SYNTHESIS_TARGET`], so a catalog
/// that grew past one row would have to teach the generator the new row before
/// this could name it.
fn synthesized_plan<'a>(
    input: EncodeInput<'a>,
    target: &DialectId,
) -> Result<ExportPlan<'a>, CodecError> {
    debug_assert_eq!(target.as_str(), SYNTHESIS_TARGET);
    let mut bytes = Vec::new();
    writer::generate::write_new(input.ir, &mut bytes)?;
    let expects_preserved_source = input
        .ir
        .source
        .as_ref()
        .is_some_and(|source| source.format == dialect::FORMAT);
    let fidelity = if input.fidelity.is_some() || expects_preserved_source {
        FidelityResolution::Degraded {
            reason: "preserved F3D source image is unavailable".into(),
        }
    } else {
        FidelityResolution::NotProvided
    };
    let losses = matches!(fidelity, FidelityResolution::Degraded { .. })
        .then(|| {
            F3dLossCode::SourcePreservedImageUnavailable
                .note("preserved F3D source image is unavailable; regenerated from IR")
        })
        .into_iter()
        .collect();
    Ok(ExportPlan::buffered(
        report(
            input.ir,
            target.clone(),
            fidelity,
            WritePath::Synthesized,
            losses,
        ),
        bytes,
    ))
}

fn report(
    ir: &CadIr,
    target: DialectId,
    fidelity: FidelityResolution,
    write_path: WritePath,
    losses: Vec<cadmpeg_ir::LossNote>,
) -> ExportReport {
    ExportReport {
        target: Some(target),
        format: dialect::FORMAT.into(),
        census: cadmpeg_ir::EntityCensus {
            basis: cadmpeg_ir::CensusBasis::IrArenas,
            counts: ir.census(),
        },
        fidelity,
        write_path,
        losses,
        notes: vec![
            match write_path {
                WritePath::VerbatimReplay => "preserved source container replayed verbatim",
                WritePath::Patched => "preserved source container replayed with semantic patches",
                WritePath::Synthesized => "source container regenerated from IR",
            }
            .into(),
            "entity counts are derived from the IR".into(),
        ],
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
