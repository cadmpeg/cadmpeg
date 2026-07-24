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
//! use cadmpeg_ir::{CodecEntry, DecodeOptions};
//!
//! # fn decode(bytes: Vec<u8>) -> Result<(), cadmpeg_ir::CodecError> {
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
//! [`Codec::inspect`]: cadmpeg_ir::Codec::inspect
//! [`CodecError::NotImplemented`]: cadmpeg_ir::CodecError::NotImplemented
//! [`DecodeOptions::container_only`]: cadmpeg_ir::DecodeOptions::container_only

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

use cadmpeg_ir::codec::{Codec, CodecError, Confidence, ContainerSummary, DecodeResult, Encoder};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Annotations, Finding, SourceFidelity};
use std::io::Write;

/// Codec for `SolidWorks` `.sldprt` part documents.
#[derive(Debug, Default, Clone, Copy)]
pub struct SldprtCodec;

/// Validate `SolidWorks` native feature-input byte references.
pub fn validate_native(ir: &CadIr) -> Vec<Finding> {
    resolved_features::validate_native(ir)
}

impl SldprtCodec {
    /// Write a decoded document with its retained source-fidelity sidecar.
    pub fn write_preserved_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: &SourceFidelity,
        writer: &mut dyn Write,
    ) -> Result<(), CodecError> {
        let records = source_records(ir, source_fidelity)?;
        Self::write_preserved_with_annotations(ir, &source_fidelity.annotations, &records, writer)
    }

    fn write_preserved_with_annotations(
        ir: &CadIr,
        annotations: &Annotations,
        records: &[UnknownRecord],
        writer: &mut dyn Write,
    ) -> Result<(), CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("semantic_sha256"));
        if expected.is_none_or(|expected| decode::semantic_hash(ir) != *expected) {
            return writer::write_semantic_with_records(ir, annotations, records, writer);
        }
        let record = records
            .iter()
            .find(|record| record.id.0 == "sldprt:file:source-image#0")
            .ok_or_else(|| {
                CodecError::NotImplemented("IR has no retained SLDPRT source image".into())
            })?;
        let data = record.data.as_ref().ok_or_else(|| {
            CodecError::Malformed("retained SLDPRT source image has no bytes".into())
        })?;
        let hash = sha256_hex(data);
        if data.len() as u64 != record.byte_len || hash != record.sha256 {
            return Err(CodecError::Malformed(
                "retained SLDPRT source image failed integrity validation".into(),
            ));
        }
        writer.write_all(data)?;
        Ok(())
    }
}

impl Codec for SldprtCodec {
    fn id(&self) -> &'static str {
        "sldprt"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if container::looks_like_sldprt(prefix) {
            Confidence::High
        } else if container::looks_like_compound_file(prefix) {
            Confidence::Low
        } else {
            Confidence::No
        }
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan_bytes(root.window());
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

impl Encoder for SldprtCodec {
    fn id(&self) -> &'static str {
        "sldprt"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        Self::encode_with_annotations(ir, &Annotations::default(), &[], writer)
    }

    fn encode_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: Option<&SourceFidelity>,
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        match source_fidelity {
            Some(value) => Self::encode_with_fidelity(ir, value, writer),
            None => Self::encode_with_annotations(ir, &Annotations::default(), &[], writer),
        }
    }
}

impl SldprtCodec {
    fn encode_with_fidelity(
        ir: &CadIr,
        source_fidelity: &SourceFidelity,
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        let records = source_records(ir, source_fidelity)?;
        Self::encode_with_annotations(ir, &source_fidelity.annotations, &records, writer)
    }

    fn encode_with_annotations(
        ir: &CadIr,
        annotations: &Annotations,
        records: &[UnknownRecord],
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        let replay = records
            .iter()
            .any(|record| record.id.0 == "sldprt:file:source-image#0");
        Self::write_preserved_with_annotations(ir, annotations, records, writer)?;
        let validation = cadmpeg_ir::validate(ir, Vec::new());
        let total_entities = validation.entity_counts.values().sum();
        Ok(ExportReport {
            format: "sldprt".into(),
            entity_counts: validation.entity_counts,
            total_entities,
            losses: Vec::new(),
            notes: vec![
                if replay {
                    "preserved source container replayed verbatim"
                } else {
                    "source container regenerated from IR"
                }
                .into(),
                "entity counts are derived from the IR".into(),
            ],
        })
    }
}

fn source_records(
    ir: &CadIr,
    source_fidelity: &SourceFidelity,
) -> Result<Vec<UnknownRecord>, CodecError> {
    let mut records = source_fidelity.native_unknown_records(ir, "sldprt")?;
    if let Some(source) = source_fidelity.retained_record("sldprt:file:source-image#0") {
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
