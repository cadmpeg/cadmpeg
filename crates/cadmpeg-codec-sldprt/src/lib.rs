// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(
    test,
    allow(
        clippy::redundant_field_names,
        clippy::unreadable_literal,
        clippy::unwrap_used
    )
)]
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
//! # fn decode(bytes: Vec<u8>) -> Result<(), cadmpeg_codec_core::CodecError> {
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
//! `partition` and `deltas` streams supply the B-rep record graph. The decoder
//! groups related body streams by site, selects the richest resulting B-rep,
//! and merges alternate sites as configuration-specific bodies. Parasolid
//! lengths are metres; decoded `CadIr` coordinates are millimetres. Directions,
//! normals, and ratios remain dimensionless.
//!
//! # Encode
//!
//! ```no_run
//! use std::fs::File;
//!
//! use cadmpeg_codec_sldprt::SldprtCodec;
//! use cadmpeg_ir::{CodecEntry, DecodeOptions, Encoder};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.sldprt")?;
//! let decoded = SldprtCodec.decode(&mut input, &DecodeOptions::default())?;
//! let mut output = File::create("part-edited.sldprt")?;
//! SldprtCodec
//!     .plan(cadmpeg_ir::codec::EncodeInput {
//!         ir: &decoded.ir,
//!         fidelity: Some(&decoded.source_fidelity),
//!     })?
//!     .write_to(&mut output)?;
//! # Ok(())
//! # }
//! ```
//!
//! [`SldprtCodec`] implements [`Encoder`] through `plan` → `write_to`. Encoding
//! with the retained `source_fidelity` sidecar replays or patches the source
//! image. Omitting `fidelity` regenerates the supported source-less profile.
//! Supported geometry edits can patch the native partition when the entity graph
//! and provenance remain stable. Retained writing can synchronize supported
//! feature, sketch, parameter, configuration, and PMI edits and returns
//! [`CodecError::NotImplemented`] for an unsupported IR shape.
//!
//! The semantic writer supports solid bodies with at most five regions and at
//! most six shells per solid region, sheet bodies with one shell per region,
//! analytic and non-periodic NURBS carriers, selected metadata and feature
//! records, base colors, and sequential triangle-strip tessellation. It bakes
//! right-handed rigid body transforms into geometry.
//!
//! [`Codec::inspect`]: cadmpeg_ir::Codec::inspect
//! [`CodecError::NotImplemented`]: cadmpeg_codec_core::CodecError::NotImplemented
//! [`DecodeOptions::container_only`]: cadmpeg_ir::DecodeOptions::container_only

mod annotations;
mod appearance;
pub mod brep;
mod classification;
mod compound;
pub mod container;
pub mod decode;
mod feature_schema;
#[cfg(feature = "fuzzing")]
pub mod fuzzing;
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

use cadmpeg_codec_core::decode::{DecodeContext, View};
use cadmpeg_codec_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeResult, EncodeInput, Encoder, ExportPlan};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::{Annotations, FidelityResolution, Finding, LossNote, Severity, SourceFidelity};
use std::io::Write;

/// Codec for `SolidWorks` `.sldprt` part documents.
#[derive(Debug, Default, Clone, Copy)]
pub struct SldprtCodec;

/// A joined native-record reference whose retained payload stays owned by the
/// source-fidelity sidecar throughout export.
struct SourceRecord<'a> {
    id: UnknownId,
    byte_len: u64,
    sha256: &'a str,
    data: Option<&'a [u8]>,
}

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
        records: &[SourceRecord<'_>],
        writer: &mut dyn Write,
    ) -> Result<(), CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("semantic_sha256"));
        if expected.is_none_or(|expected| decode::semantic_hash(ir) != *expected) {
            return writer::write_semantic_with_records(ir, annotations, records, writer);
        }
        let Some(record) = records
            .iter()
            .find(|record| record.id.0 == "sldprt:file:source-image#0")
        else {
            return writer::write_semantic_with_records(ir, annotations, records, writer);
        };
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

    fn plan<'a>(&self, input: EncodeInput<'a>) -> Result<ExportPlan<'a>, CodecError> {
        let mut bytes = Vec::new();
        let mut report = match input.fidelity {
            Some(value) => Self::encode_with_fidelity(input.ir, value, &mut bytes)?,
            None => {
                Self::encode_with_annotations(input.ir, &Annotations::default(), &[], &mut bytes)?
            }
        };
        let replay = input
            .fidelity
            .and_then(|value| value.retained_record("sldprt:file:source-image#0"))
            .is_some();
        let expects_preserved_source = input
            .ir
            .source
            .as_ref()
            .is_some_and(|source| source.format == "sldprt");
        let fidelity = match (input.fidelity.is_some() || expects_preserved_source, replay) {
            (_, true) => FidelityResolution::Replayed,
            (true, false) => FidelityResolution::Degraded {
                reason: "preserved SLDPRT source image is unavailable".into(),
            },
            (false, false) => FidelityResolution::NotProvided,
        };
        if replay {
            report.notes[0] = input
                .fidelity
                .and_then(|value| value.retained_record("sldprt:file:source-image#0"))
                .filter(|source| source.data.as_deref() == Some(bytes.as_slice()))
                .map_or(
                    "preserved source container replayed with semantic patches",
                    |_| "preserved source container replayed verbatim",
                )
                .into();
        }
        if matches!(fidelity, FidelityResolution::Degraded { .. }) {
            report.losses.push(LossNote {
                code: cadmpeg_ir::LossKind::PreservedSourceUnavailable,
                severity: Severity::Blocking,
                message: "preserved SLDPRT source image is unavailable; regenerated from IR".into(),
                provenance: None,
            });
        }
        Ok(ExportPlan::buffered(report, fidelity, bytes))
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
        records: &[SourceRecord<'_>],
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        let replay = records
            .iter()
            .any(|record| record.id.0 == "sldprt:file:source-image#0");
        Self::write_preserved_with_annotations(ir, annotations, records, writer)?;
        let validation = cadmpeg_ir::validate(ir, Vec::new());
        Ok(ExportReport {
            format: "sldprt".into(),
            census: cadmpeg_ir::EntityCensus {
                basis: cadmpeg_ir::CensusBasis::IrArenas,
                counts: validation.entity_counts,
            },
            fidelity: FidelityResolution::NotProvided,
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

fn source_records<'a>(
    ir: &CadIr,
    source_fidelity: &'a SourceFidelity,
) -> Result<Vec<SourceRecord<'a>>, CodecError> {
    let retained_by_id = source_fidelity
        .retained_records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<std::collections::HashMap<_, _>>();
    let mut records = ir
        .native_unknowns_iter("sldprt")
        .map(|reference| {
            let reference = reference?;
            let retained = retained_by_id.get(reference.id.0.as_str()).ok_or_else(|| {
                cadmpeg_ir::native::NativeConvertError::MissingRetainedSourceRecord(
                    reference.id.0.clone(),
                )
            })?;
            Ok(SourceRecord {
                id: reference.id,
                byte_len: retained.byte_len,
                sha256: &retained.sha256,
                data: retained.data.as_deref(),
            })
        })
        .collect::<Result<Vec<_>, cadmpeg_ir::native::NativeConvertError>>()?;
    if let Some(source) = source_fidelity.retained_record("sldprt:file:source-image#0") {
        records.push(SourceRecord {
            id: source.id.clone().into(),
            byte_len: source.byte_len,
            sha256: &source.sha256,
            data: source.data.as_deref(),
        });
    }
    Ok(records)
}

#[cfg(test)]
mod tests;
