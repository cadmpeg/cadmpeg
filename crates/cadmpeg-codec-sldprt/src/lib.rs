// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
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
//! use cadmpeg_ir::{Codec, DecodeOptions};
//!
//! # fn decode(bytes: Vec<u8>) -> Result<(), cadmpeg_core::CodecError> {
//! let decoded = SldprtCodec.decode(
//!     &mut Cursor::new(bytes),
//!     &DecodeOptions::default(),
//! )?;
//! println!("{} faces", decoded.ir().model.faces.len());
//! for loss in &decoded.report().losses {
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
//! use cadmpeg_ir::codec::TargetRequest;
//! use cadmpeg_ir::{Codec, DecodeOptions, Encoder};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.sldprt")?;
//! let decoded = SldprtCodec.decode(&mut input, &DecodeOptions::default())?;
//! let mut output = File::create("part-edited.sldprt")?;
//! SldprtCodec
//!     .plan(cadmpeg_ir::codec::EncodeInput {
//!         ir: decoded.ir(),
//!         fidelity: Some(decoded.source_fidelity()),
//!     }, TargetRequest::Inherit)?
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
//! [`CodecError::NotImplemented`]: cadmpeg_core::CodecError::NotImplemented
//! [`DecodeOptions::container_only`]: cadmpeg_ir::DecodeOptions::container_only

mod annotations;
mod appearance;
#[allow(dead_code)] // Internal parser surface is retained for fuzz and crate tests.
pub(crate) mod brep;
mod classification;
#[allow(dead_code)] // Internal parser surface is retained for fuzz and crate tests.
pub(crate) mod container;
#[allow(dead_code)] // Internal parser surface is retained for fuzz and crate tests.
pub(crate) mod decode;
mod feature_schema;
#[doc(hidden)]
pub mod fuzz;
mod history;
/// Byte-offset constants generated from `docs/layouts/sldprt.toml`.
pub(crate) mod layout;
#[allow(dead_code)] // Loss catalog is consumed by the writer and hidden facade.
pub(crate) mod loss;
mod metadata;
mod native;
#[allow(dead_code)] // Internal parser surface is retained for fuzz and crate tests.
pub(crate) mod parasolid;
mod pmi;
#[allow(dead_code)] // Internal record surface is retained for fuzz and crate tests.
pub(crate) mod records;
mod resolved_features;
mod swift;
mod tessellation;
mod writer;
mod writer_patch;
mod writer_transform;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use std::io::Write;

use cadmpeg_ir::codec::{
    CodecBackend, Confidence, DecodeResult, EncodeInput, Encoder, ExportPlan, TargetDescriptor,
    TargetRequest,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::{Annotations, FidelityResolution, Finding, SourceFidelity, WritePath};

use crate::loss::SldprtLossCode;

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
    resolved_features::validate::validate_native(ir)
}

impl SldprtCodec {
    /// Write a decoded document with its retained source-fidelity sidecar.
    pub fn write_preserved_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: &SourceFidelity,
        writer: &mut dyn Write,
    ) -> Result<WritePath, CodecError> {
        let records = source_records(ir, source_fidelity)?;
        Self::write_preserved_with_annotations(ir, &source_fidelity.annotations, &records, writer)
    }

    /// Replay the retained source image when the document is untouched since the
    /// decode that recorded its baseline, and write it semantically otherwise.
    ///
    /// The `document_local_sha256` baseline answers only "was this edited since
    /// it was decoded?", bitwise and on this machine; see
    /// [`decode::document_local_sha256`]. An absent baseline means the question
    /// cannot be answered, so the document takes the semantic write path — the
    /// conservative branch, which reproduces the document from the IR instead of
    /// replaying bytes that may no longer describe it.
    ///
    /// The returned [`WritePath`] is the branch this function took, not a
    /// judgement made about its output afterwards. The semantic writer can
    /// reproduce the input byte for byte when nothing it rewrites has moved, so
    /// the output cannot say which branch ran and only this value can.
    fn write_preserved_with_annotations(
        ir: &CadIr,
        annotations: &Annotations,
        records: &[SourceRecord<'_>],
        writer: &mut dyn Write,
    ) -> Result<WritePath, CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE));
        if expected.is_none_or(|expected| decode::document_local_sha256(ir) != *expected) {
            return Self::write_semantic(ir, annotations, records, writer);
        }
        let Some(record) = records
            .iter()
            .find(|record| record.id.0 == "sldprt:file:source-image#0")
        else {
            return Self::write_semantic(ir, annotations, records, writer);
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
        Ok(WritePath::VerbatimReplay)
    }

    /// Runs the semantic writer and names the path it stands for: `Patched` when
    /// retained source records fed the write, `Synthesized` when the document was
    /// built from the neutral IR alone.
    fn write_semantic(
        ir: &CadIr,
        annotations: &Annotations,
        records: &[SourceRecord<'_>],
        writer: &mut dyn Write,
    ) -> Result<WritePath, CodecError> {
        writer::write_semantic_with_records(ir, annotations, records, writer)?;
        Ok(if records.is_empty() {
            WritePath::Synthesized
        } else {
            WritePath::Patched
        })
    }
}

impl CodecBackend for SldprtCodec {
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
/// `writer::generated_solidworks_xml` emits a `swSolidWorks` block with no
/// `swVersion` attribute, so a synthesized part carries no version declaration
/// and classifies into the registry's totality row. Naming it here is the
/// honest catalog: re-decoding this writer's output lands on exactly this id.
/// Every versioned row is reachable by replaying a retained part, which is
/// preservation, not synthesis.
const SLDPRT_TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
    id: "sldprt:unknown",
    label: "SolidWorks part with no swVersion declaration",
    aliases: &[],
    default: true,
}];

impl Encoder for SldprtCodec {
    fn id(&self) -> &'static str {
        "sldprt"
    }

    fn targets(&self) -> &'static [TargetDescriptor] {
        SLDPRT_TARGETS
    }

    fn plan<'a>(
        &self,
        input: EncodeInput<'a>,
        request: TargetRequest<'_>,
    ) -> Result<ExportPlan<'a>, CodecError> {
        request.check_explicit(Encoder::id(self), self.targets())?;
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
        report.fidelity = match (input.fidelity.is_some() || expects_preserved_source, replay) {
            (_, true) => FidelityResolution::Replayed,
            (true, false) => FidelityResolution::Degraded {
                reason: "preserved SLDPRT source image is unavailable".into(),
            },
            (false, false) => FidelityResolution::NotProvided,
        };
        if matches!(report.fidelity, FidelityResolution::Degraded { .. }) {
            report.losses.push(
                SldprtLossCode::SourcePreservedImageUnavailable
                    .note("preserved SLDPRT source image is unavailable; regenerated from IR"),
            );
        }
        Ok(ExportPlan::buffered(report, bytes))
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
        let write_path = Self::write_preserved_with_annotations(ir, annotations, records, writer)?;
        Ok(ExportReport {
            target: None,
            format: "sldprt".into(),
            census: cadmpeg_ir::EntityCensus {
                basis: cadmpeg_ir::CensusBasis::IrArenas,
                counts: ir.census(),
            },
            fidelity: FidelityResolution::NotProvided,
            write_path,
            losses: Vec::new(),
            notes: vec![
                match write_path {
                    WritePath::VerbatimReplay => "preserved source container replayed verbatim",
                    WritePath::Patched => {
                        "preserved source container replayed with semantic patches"
                    }
                    WritePath::Synthesized => "source container regenerated from IR",
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
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests {
    #[test]
    fn source_record_join_borrows_the_retained_source_image() {
        let payload = vec![0x5a; 4096];
        let payload_ptr = payload.as_ptr();
        let mut fidelity = cadmpeg_ir::SourceFidelity::default();
        fidelity.retained_records = vec![cadmpeg_ir::source_fidelity::RetainedSourceRecord {
            id: "sldprt:file:source-image#0".into(),
            stream: "source".into(),
            offset: 0,
            byte_len: payload.len() as u64,
            sha256: cadmpeg_ir::hash::sha256_hex(&payload),
            data: Some(payload),
        }];

        let records = crate::source_records(&cadmpeg_ir::examples::unit_cube(), &fidelity).unwrap();
        let retained = records[0].data.expect("retained source bytes");
        assert_eq!(retained.as_ptr(), payload_ptr);
    }
}
