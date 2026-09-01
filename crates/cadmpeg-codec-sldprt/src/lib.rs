// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write `SolidWorks` `.sldprt` part documents.
//!
//! [`SldprtCodec`] decodes B-rep topology, analytic and NURBS geometry,
//! tessellation, appearances, selected document attributes, feature history,
//! and feature-input records into [`cadmpeg_ir::CadIr`]. It preserves source
//! blocks and records provenance so supported edits can retain native data.
//!
//! <!-- generated: capability sldprt -->
//! Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidworks-sldprt)).
//! <!-- /generated: capability sldprt -->
//!
//! # Decode
//!
//! ```
//! use std::io::Cursor;
//!
//! use cadmpeg_codec_sldprt::SldprtCodec;
//! use cadmpeg_ir::{Codec, DecodeOptions};
//!
//! # fn decode(bytes: Vec<u8>) -> Result<(), cadmpeg_ir::DecodeFailure> {
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
//! Supported geometry edits can patch the native partition when the entity
//! graph and provenance remain stable. Retained writing synchronizes supported
//! feature, sketch, parameter, configuration, and PMI edits. Semantic writing
//! returns [`CodecError::NotImplemented`] when the requested IR shape cannot be
//! represented.
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
mod dialect;
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
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ContainerSummary;
use std::io::Write;

use cadmpeg_ir::codec::{
    CodecBackend, Confidence, DecodeResult, EncodeInput, EncoderBackend, EncoderTargetDomain,
    ExportPlan, ResolvedEncoderTarget,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::{Annotations, FidelityResolution, Finding, SourceFidelity, WritePath};

/// Retained-record id of the whole source part, the byte-replay baseline.
const SOURCE_IMAGE_ID: &str = "sldprt:file:source-image#0";

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
    #[cfg_attr(not(test), allow(dead_code))] // Crate-owned replay tests exercise this write door.
    #[allow(clippy::unused_self, clippy::trivially_copy_pass_by_ref)] // Preserve the tested method shape while narrowing visibility.
    pub(crate) fn write_preserved_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: &SourceFidelity,
        writer: &mut dyn Write,
    ) -> Result<WritePath, CodecError> {
        let records = source_records(ir, source_fidelity)?;
        Ok(Self::write_preserved_with_annotations(
            ir,
            &source_fidelity.annotations,
            &records,
            writer,
        )?
        .path())
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
    ) -> Result<Written, CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE));
        let Some(expected) = expected else {
            return Self::write_semantic(
                ir,
                annotations,
                records,
                SemanticFidelity::ReplaySkipped(ReplaySkipped::BaselineMissing),
                writer,
            );
        };
        if decode::document_local_sha256(ir) != *expected {
            return Self::write_semantic(
                ir,
                annotations,
                records,
                SemanticFidelity::ReplaySkipped(ReplaySkipped::DigestMismatch),
                writer,
            );
        }
        let Some(record) = records.iter().find(|record| record.id.0 == SOURCE_IMAGE_ID) else {
            return Self::write_semantic(
                ir,
                annotations,
                records,
                SemanticFidelity::ReplaySkipped(ReplaySkipped::ImageMissing),
                writer,
            );
        };
        let Some(data) = record.data.as_ref() else {
            return Self::write_semantic(
                ir,
                annotations,
                records,
                SemanticFidelity::ReplaySkipped(ReplaySkipped::ImageMissing),
                writer,
            );
        };
        let hash = sha256_hex(data);
        if data.len() as u64 != record.byte_len || hash != record.sha256 {
            return Err(CodecError::Malformed(
                "retained SLDPRT source image failed integrity validation".into(),
            ));
        }
        writer.write_all(data)?;
        Ok(Written::Replayed)
    }

    /// Runs the semantic writer and names the path it stands for: `Patched` when
    /// retained source records fed the write, `Synthesized` when the document was
    /// built from the neutral IR alone.
    fn write_semantic(
        ir: &CadIr,
        annotations: &Annotations,
        records: &[SourceRecord<'_>],
        fidelity: SemanticFidelity,
        writer: &mut dyn Write,
    ) -> Result<Written, CodecError> {
        let dialect = writer::write_semantic_with_records(ir, annotations, records, writer)?;
        Ok(Written::Semantic {
            path: if records.is_empty() {
                WritePath::Synthesized
            } else {
                WritePath::Patched
            },
            dialect,
            fidelity,
        })
    }
}

/// Why an eligible retained-image replay took the semantic path instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaySkipped {
    BaselineMissing,
    DigestMismatch,
    ImageMissing,
}

impl ReplaySkipped {
    fn fidelity(self) -> FidelityResolution {
        FidelityResolution::Degraded {
            reason: match self {
                Self::BaselineMissing => "preserved SLDPRT source digest baseline is unavailable",
                Self::DigestMismatch => {
                    "decoded model no longer matches the preserved SLDPRT source digest"
                }
                Self::ImageMissing => "preserved SLDPRT source image is unavailable",
            }
            .into(),
        }
    }
}

/// Fidelity result for a semantic write. A replay skip carries the sole typed
/// reason from which both fidelity and any applicable loss are derived.
enum SemanticFidelity {
    Resolution(FidelityResolution),
    ReplaySkipped(ReplaySkipped),
}

impl SemanticFidelity {
    fn resolution(&self) -> FidelityResolution {
        match self {
            Self::Resolution(resolution) => resolution.clone(),
            Self::ReplaySkipped(reason) => reason.fidelity(),
        }
    }
}

/// One completed write.
enum Written {
    /// The retained source image, byte for byte. The dialect is the source's
    /// own, which this write neither chooses nor needs to name.
    Replayed,
    /// A semantic write, in the dialect the emitted `swSolidWorks` envelope
    /// declares.
    Semantic {
        path: WritePath,
        dialect: DialectId,
        fidelity: SemanticFidelity,
    },
}

impl Written {
    /// Which branch ran. The semantic writer can reproduce its input byte for
    /// byte, so the output cannot say and only this value can.
    fn path(&self) -> WritePath {
        match self {
            Self::Replayed => WritePath::VerbatimReplay,
            Self::Semantic { path, .. } => *path,
        }
    }

    fn fidelity(&self) -> FidelityResolution {
        match self {
            Self::Replayed => FidelityResolution::Replayed,
            Self::Semantic { fidelity, .. } => fidelity.resolution(),
        }
    }

    fn replay_skipped(&self) -> Option<ReplaySkipped> {
        match self {
            Self::Semantic {
                fidelity: SemanticFidelity::ReplaySkipped(reason),
                ..
            } => Some(*reason),
            _ => None,
        }
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
        let classification = dialect::classify_layers(&scan);
        let mut summary = container::summarize(&scan, classification.layers().clone());
        classification.append_losses(&mut summary.losses);
        Ok(summary)
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(ctx, root)
    }
}

impl EncoderBackend for SldprtCodec {
    const FORMAT: &'static str = dialect::FORMAT;
    const TARGET_DOMAIN: EncoderTargetDomain = EncoderTargetDomain::Catalog(dialect::TARGETS);

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedEncoderTarget,
    ) -> Result<ExportPlan, CodecError> {
        let ResolvedEncoderTarget::Native(resolved) = target else {
            unreachable!("a catalog encoder receives only native target resolutions")
        };
        writer::target::plan(input, &resolved)
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
    if let Some(source) = source_fidelity.retained_record(SOURCE_IMAGE_ID) {
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
    use super::SOURCE_IMAGE_ID;

    #[test]
    fn source_record_join_borrows_the_retained_source_image() {
        let payload = vec![0x5a; 4096];
        let payload_ptr = payload.as_ptr();
        let mut fidelity = cadmpeg_ir::SourceFidelity::default();
        fidelity.retained_records = vec![cadmpeg_ir::source_fidelity::RetainedSourceRecord {
            id: SOURCE_IMAGE_ID.into(),
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
