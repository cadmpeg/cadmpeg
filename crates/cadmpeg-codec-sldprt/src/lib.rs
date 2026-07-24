// SPDX-License-Identifier: Apache-2.0
//! Read and write `SolidWorks` `.sldprt` part documents.
//!
//! [`SldprtCodec`] decodes B-rep topology, analytic and NURBS geometry,
//! tessellation, appearances, selected document attributes, feature history,
//! and feature-input records into [`cadmpeg_ir::CadIr`]. It preserves source
//! blocks and records provenance so supported edits can retain native data.
//!
//! Support level: [L4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! on the cadmpeg support ladder.
//!
//! # Decode
//!
//! ```
//! use std::io::Cursor;
//!
//! use cadmpeg_codec_sldprt::SldprtCodec;
//! use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
//!
//! # fn decode(bytes: Vec<u8>) -> Result<(), cadmpeg_ir::codec::CodecError> {
//! let decoded = SldprtCodec.decode(
//!     &mut Cursor::new(bytes),
//!     &DecodeOptions::default(),
//! )?;
//! println!("{} faces", decoded.ir.model.faces.len());
//! for loss in &decoded.report.losses {
//!     eprintln!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Decode reports can accompany a usable model. Untyped support carriers become
//! opaque geometry linked to retained bytes, while their resolvable topology
//! remains in the IR. Failure to build a Parasolid graph yields a metadata-only
//! IR with blocking diagnostics. Set [`DecodeOptions::container_only`] to request
//! that result without attempting geometry.
//!
//! [`Codec::inspect`] inventories the outer blocks, section directory, cache
//! cells, payload families, and Parasolid schemas. It does not build model
//! geometry.
//!
//! # Format and units
//!
//! The outer container uses an 8-byte header, CRC-validated raw-DEFLATE blocks,
//! a fixed-cell section index, and a tail directory. Embedded Parasolid
//! `partition` and `deltas` streams supply the B-rep record graph. Parasolid
//! lengths are metres; decoded `CadIr` coordinates are millimetres. Directions,
//! normals, and ratios remain dimensionless.
//!
//! # Encode
//!
//! [`SldprtCodec`] implements [`Encoder`]. Unchanged decoded IR replays its
//! retained source image byte for byte. Supported geometry edits can patch the
//! native partition when the entity graph and provenance remain stable.
//! Otherwise the writer regenerates supported semantic records and returns
//! [`CodecError::NotImplemented`] for an unsupported IR shape.
//!
//! The semantic writer supports solid bodies with multiple regions and shells,
//! sheet bodies with one shell per region, analytic and non-periodic NURBS carriers, selected
//! metadata and feature records, base colors, and sequential triangle-strip
//! tessellation. It bakes right-handed rigid body transforms into geometry.
//!
//! [`Codec::inspect`]: cadmpeg_ir::codec::Codec::inspect
//! [`CodecError::NotImplemented`]: cadmpeg_ir::codec::CodecError::NotImplemented
//! [`DecodeOptions::container_only`]: cadmpeg_ir::codec::DecodeOptions::container_only

mod annotations;
mod appearance;
pub mod brep;
mod classification;
mod compound;
pub mod container;
pub mod decode;
mod feature_schema;
mod history;
pub mod loss;
mod metadata;
mod native;
pub mod parasolid;
mod pmi;
pub mod records;
mod resolved_features;
mod tessellation;
mod writer;
mod writer_patch;
mod writer_transform;

// Thin `()`-returning wrappers over the crate-private `brep` leaf scanners so
// the `cadmpeg-fuzz` harness can reach them without widening the stable API.
#[cfg(feature = "fuzzing")]
pub mod fuzzing;

use cadmpeg_ir::codec::{Codec, CodecError, Confidence, ContainerSummary, DecodeResult, Encoder};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::source_fidelity::write_plan::{plan_write, WritePlan};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Annotations, Finding, SourceFidelity};
use std::io::Write;

/// Identifier of the retained whole-file `.sldprt` source image in a
/// [`SourceFidelity`] sidecar. This is the record the write-plan gate keys on to
/// replay or patch, and the record `decode` produces for the outer container.
pub(crate) const SOURCE_IMAGE_ID: &str = "sldprt:file:source-image#0";

/// Codec for `SolidWorks` `.sldprt` part documents.
#[derive(Debug, Default, Clone, Copy)]
pub struct SldprtCodec;

/// Validate `SolidWorks` native feature-input byte references.
pub fn validate_native(ir: &CadIr) -> Vec<Finding> {
    resolved_features::validate_native(ir)
}

/// How the writer produced the output container, recorded for the export note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteOutcome {
    /// The retained source image was written back byte for byte.
    Replay,
    /// Retained records were patched into a regenerated container.
    Patch,
    /// The container was regenerated from the neutral model. `degraded` marks a
    /// regeneration that fell back from an intended replay or patch because the
    /// retained source image was unusable.
    Generate {
        /// Whether a retained source image was present but could not be used.
        degraded: bool,
    },
}

impl SldprtCodec {
    /// Write a decoded document with its retained source-fidelity sidecar.
    pub fn write_preserved_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: &SourceFidelity,
        writer: &mut dyn Write,
    ) -> Result<(), CodecError> {
        write_container(ir, Some(source_fidelity), writer)?;
        Ok(())
    }
}

/// Choose replay, patch, or generate for `ir` through the shared write-plan
/// gate, then emit the corresponding container.
///
/// The gate is [`cadmpeg_ir::source_fidelity::write_plan::plan_write`]:
/// [`WritePlan::Replay`] writes the verified source image verbatim,
/// [`WritePlan::Patch`] feeds the retained records to the semantic writer, and
/// [`WritePlan::Generate`] regenerates the container from the model alone. A
/// missing sidecar, missing or corrupt source-image record, or absent baseline
/// all resolve to [`WritePlan::Generate`]; when a source-image record was
/// nonetheless present the outcome is marked degraded for the export note.
fn write_container(
    ir: &CadIr,
    sidecar: Option<&SourceFidelity>,
    writer: &mut dyn Write,
) -> Result<WriteOutcome, CodecError> {
    let default_annotations = Annotations::default();
    let annotations = sidecar.map_or(&default_annotations, |fidelity| &fidelity.annotations);
    // Built eagerly for the patch branch; the build also preserves the
    // pre-write error surface of `native_unknown_records` on every branch.
    let records = match sidecar {
        Some(fidelity) => source_records(ir, fidelity)?,
        None => Vec::new(),
    };
    let baseline = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("semantic_sha256"))
        .map(String::as_str);
    let current = decode::semantic_hash(ir);
    match plan_write(sidecar, SOURCE_IMAGE_ID, baseline, &current) {
        WritePlan::Replay(image) => {
            writer.write_all(image)?;
            Ok(WriteOutcome::Replay)
        }
        WritePlan::Patch(_) => {
            writer::write_semantic_with_records(ir, annotations, &records, writer)?;
            Ok(WriteOutcome::Patch)
        }
        WritePlan::Generate => {
            let degraded =
                sidecar.is_some_and(|fidelity| fidelity.retained_record(SOURCE_IMAGE_ID).is_some());
            writer::write_semantic_with_records(ir, annotations, &[], writer)?;
            Ok(WriteOutcome::Generate { degraded })
        }
    }
}

/// Emit the container and build the export report describing the branch taken.
fn encode_container(
    ir: &CadIr,
    sidecar: Option<&SourceFidelity>,
    writer: &mut dyn Write,
) -> Result<ExportReport, CodecError> {
    let outcome = write_container(ir, sidecar, writer)?;
    let validation = cadmpeg_ir::validate(ir, Vec::new());
    let total_entities = validation.entity_counts.values().sum();
    let mut notes = vec![
        match outcome {
            WriteOutcome::Replay => "preserved source container replayed verbatim",
            WriteOutcome::Patch => "retained source records patched into the container",
            WriteOutcome::Generate { .. } => "source container regenerated from IR",
        }
        .into(),
        "entity counts are derived from the IR".into(),
    ];
    if matches!(outcome, WriteOutcome::Generate { degraded: true }) {
        notes.push(
            "retained source image was unusable for replay or patch; regenerated from IR".into(),
        );
    }
    Ok(ExportReport {
        format: "sldprt".into(),
        entity_counts: validation.entity_counts,
        total_entities,
        losses: Vec::new(),
        notes,
    })
}

impl Codec for SldprtCodec {
    fn id(&self) -> &'static str {
        "sldprt"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if container::looks_like_sldprt(prefix) {
            Confidence::High
        } else {
            Confidence::No
        }
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(ctx, root);
        Ok(container::summarize(&scan))
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(ctx, root)
    }

    fn validate_native(&self, ir: &CadIr) -> Vec<Finding> {
        validate_native(ir)
    }
}

impl Encoder for SldprtCodec {
    fn id(&self) -> &'static str {
        "sldprt"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        encode_container(ir, None, writer)
    }

    fn encode_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: Option<&SourceFidelity>,
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        encode_container(ir, source_fidelity, writer)
    }
}

fn source_records(
    ir: &CadIr,
    source_fidelity: &SourceFidelity,
) -> Result<Vec<UnknownRecord>, CodecError> {
    let mut records = source_fidelity.native_unknown_records(ir, "sldprt")?;
    if let Some(source) = source_fidelity.retained_record(SOURCE_IMAGE_ID) {
        records.push(UnknownRecord {
            id: source.id.clone().into(),
            offset: source.offset,
            byte_len: source.byte_len,
            sha256: source.sha256.clone(),
            data: source.data.clone(),
            links: Vec::new(),
        });
    }
    Ok(records)
}

#[cfg(test)]
mod tests;
