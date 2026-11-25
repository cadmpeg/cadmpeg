// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write `SolidWorks` `.sldprt` part documents, and read the
//! component structure of `.sldasm` assembly documents.
//!
//! [`SldprtCodec`] decodes B-rep topology, analytic and NURBS geometry,
//! tessellation, appearances, selected document attributes, feature history,
//! and feature-input records into [`cadmpeg_ir::CadIr`]. It preserves source
//! blocks and records provenance so supported edits can retain native data.
//!
//! <!-- generated: capability sldprt -->
//! Support: L3 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidworks-sldprt)).
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
//! println!("{} faces", decoded.ir().geometry.faces.len());
//! for loss in &decoded.report().losses {
//!     eprintln!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Decode reports can accompany a usable model. Untyped support carriers become
//! opaque geometry linked to retained bytes, while their resolvable topology
//! remains in the IR. No usable geometry yields metadata with diagnostics.
//! Set [`DecodeOptions::depth`] to [`DecodeDepth::Container`] to read the
//! container only: the document then states no model record.
//! Semantic Parasolid graph errors propagate as decode errors.
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
//! groups related body streams by site and selects the resolved active site.
//! Without a resolved active site, the first site is a deterministic merge
//! accumulator. Alternate sites become configuration-specific bodies. Parasolid
//! lengths are metres; decoded `CadIr` coordinates are millimetres. Directions,
//! normals, and ratios remain dimensionless.
//!
//! # Encode
//!
//! ```no_run
//! use std::fs::File;
//!
//! use cadmpeg_codec_sldprt::SldprtCodec;
//! use cadmpeg_ir::codec::write::target::TargetRequest;
//! use cadmpeg_ir::codec::write::Encoder;
//! use cadmpeg_ir::{Codec, DecodeOptions};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.sldprt")?;
//! let decoded = SldprtCodec.decode(&mut input, &DecodeOptions::default())?;
//! let (ir, _report, fidelity) = decoded.into_parts();
//! let mut output = File::create("part-edited.sldprt")?;
//! SldprtCodec
//!     .plan(
//!         cadmpeg_ir::codec::write::EncodeInput::new(&ir, Some(&fidelity)),
//!         TargetRequest::Inherit,
//!     )?
//!     .write_to(&mut output)?;
//! # Ok(())
//! # }
//! ```
//!
//! [`SldprtCodec`] implements [`cadmpeg_ir::codec::write::Encoder`] through
//! `plan` → `write_to`. Encoding
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
//! [`DecodeOptions::depth`]: cadmpeg_ir::DecodeOptions::depth
//! [`DecodeDepth::Container`]: cadmpeg_core::decode::DecodeDepth::Container

mod annotations;
mod appearance;
mod archive;
mod assembly;
mod brep;
mod carchive;
mod classification;
mod codec_digests;
mod container;
mod decode;
mod design_v11;
mod dialect;
pub mod exact_ledger;
mod feature_schema;
mod framed_string;
#[doc(hidden)]
pub mod fuzz;
mod history;
mod lane_refusal;
/// Byte-offset constants generated from `docs/layouts/sldprt.toml`.
mod layout;
mod loss;
mod metadata;
mod native;
mod parasolid;
mod pmi;
mod records;
mod resolved_features;
mod rollback;
mod scan;
mod swift;
mod tessellation;
mod writer;
mod writer_patch;
mod writer_transform;

use cadmpeg_core::decode::{DecodeContext, DecodeDepth, View};
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ContainerSummary;
use std::io::Write;

use cadmpeg_ir::codec::write::{
    target::{Catalog, ResolvedWrite},
    ArenaCoverage, ArenaDispositions, Consumption, EncodeInput, EncoderBackend, ExportBody,
    NativeDisposition, NativeDispositions, PatchConsumption, WritePath,
};
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::source_fidelity::CodecDigests;
use cadmpeg_ir::{report::check::Finding, Annotations, SourceFidelity};

/// Retained-record id of the whole source part, the byte-replay baseline.
const SOURCE_IMAGE_ID: &str = "sldprt:file:source-image#0";

/// The identity of the retained source image, composed from its literal parts.
fn source_image_id() -> UnknownId {
    UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "file", "source-image"),
        cadmpeg_ir::identity_key!("0"),
    )
}

/// Codec for `SolidWorks` `.sldprt` part documents.
#[derive(Debug, Default, Clone, Copy)]
pub struct SldprtCodec;

/// A joined native-record reference whose retained payload stays owned by the
/// source-fidelity sidecar throughout export.
struct SourceRecord<'a> {
    id: UnknownId,
    data: Option<&'a [u8]>,
}

impl SldprtCodec {
    /// Replay the retained source image when the document is the one the
    /// decode returned (`fidelity`'s content digest, §3.1, Rule B), and write
    /// it semantically otherwise.
    ///
    /// The returned [`WritePath`] is the branch this function took, not a
    /// judgement made about its output afterwards. The semantic writer can
    /// reproduce the input byte for byte when nothing it rewrites has moved, so
    /// the output cannot say which branch ran and only this value can. Where
    /// the semantic write refuses, or lands on another dialect than the
    /// source's, [`writer::target`] takes the next path of §3.4. A part read
    /// from CADIR JSON whose history arenas the retained source did not
    /// restore is written as the stored part, and `export:edit.dropped` names
    /// the file.
    fn write_preserved_with_annotations(
        ir: &CadIr,
        fidelity: &SourceFidelity,
        records: &[SourceRecord<'_>],
        writer: &mut dyn Write,
    ) -> Result<Written, CodecError> {
        let image = records
            .iter()
            .find(|record| record.id.as_str() == SOURCE_IMAGE_ID)
            .and_then(|record| record.data);
        let (annotations, digests) = (&fidelity.annotations, &fidelity.codec_digests);
        if fidelity.edited(ir)? {
            if let Some((image, loss)) = image
                .filter(|_| !ir.model.omitted_arenas_held())
                .zip(Self::unrestored_loss())
            {
                writer.write_all(image)?;
                return Ok(Written::Stored { losses: vec![loss] });
            }
            return Self::write_semantic(
                ir,
                annotations,
                digests,
                records,
                &[],
                SemanticFidelity::ReplaySkipped(ReplaySkipped::DigestMismatch),
                writer,
            );
        }
        let Some(data) = image else {
            return Self::write_semantic(
                ir,
                annotations,
                digests,
                records,
                &[],
                SemanticFidelity::ReplaySkipped(ReplaySkipped::ImageMissing),
                writer,
            );
        };
        writer.write_all(data)?;
        Ok(Written::Replayed)
    }

    /// The loss of a part read from CADIR JSON written as its stored part,
    /// because the retained source did not restore the history arenas its
    /// edits are written into.
    fn unrestored_loss() -> Option<cadmpeg_ir::report::loss::LossNote> {
        cadmpeg_ir::codec::write::edit_dropped(
            cadmpeg_ir::schema::EntityKind::Feature,
            "SLDPRT",
            "keeps the stored part, without any edit of the document read from CADIR, because \
             the retained source part does not restore the feature history the edits are \
             written into, so no digest isolates an edited record",
            [SOURCE_IMAGE_ID],
        )
    }

    /// Runs the semantic writer and names the path it stands for: `Patched` when
    /// retained source records fed the write, `Synthesized` when the document was
    /// built from the neutral IR alone. `digests` are the codec digests the
    /// decode recorded. `shaped` names the edits that shape the stored solid
    /// beyond those the write compares with its retained source.
    fn write_semantic(
        ir: &CadIr,
        annotations: &Annotations,
        digests: &CodecDigests,
        records: &[SourceRecord<'_>],
        shaped: &[history::write::brep_agreement::ShapingEdit],
        fidelity: SemanticFidelity,
        writer: &mut dyn Write,
    ) -> Result<Written, CodecError> {
        if !ir.model.omitted_arenas_held()
            && (!records.is_empty() || ir.native.namespace("sldprt").is_some())
        {
            return Err(CodecError::NotImplemented(
                "the document was read from CADIR, which does not carry the feature, sketch, \
                 parameter, and configuration arenas the SLDPRT writer applies native history \
                 edits from, and no retained source part restored them; write it from the \
                 decoded source file"
                    .into(),
            ));
        }
        // An edit the full decoder does not read back from the written part,
        // or that changes what no edit touches, is written again with its
        // feature's stored record.
        let image = records
            .iter()
            .find(|record| record.id.as_str() == SOURCE_IMAGE_ID)
            .and_then(|record| record.data);
        let round_trip = |source: &cadmpeg_ir::codec::Decoded| {
            let mut bytes = Vec::new();
            writer::write_semantic_with_records(
                &source.ir,
                &source.source_fidelity.annotations,
                &source.source_fidelity.codec_digests,
                records,
                &[],
                &[],
                &mut bytes,
            )
            .ok()?;
            (image != Some(bytes.as_slice())).then_some(bytes)
        };
        let ((bytes, output), read_back) = history::write::read_back::write_until_read_back(
            ir,
            digests,
            &mut |withheld| {
                let mut bytes = Vec::new();
                let output = writer::write_semantic_with_records(
                    ir,
                    annotations,
                    digests,
                    records,
                    withheld,
                    shaped,
                    &mut bytes,
                )?;
                Ok((bytes, output))
            },
            &round_trip,
        )?;
        writer.write_all(&bytes)?;
        let writer::SemanticOutput {
            dialect,
            coverage,
            unwritten,
            unwritten_edits,
            written,
            claims: _,
            source: _,
            losses,
        } = output;
        let unwritten_edits = unwritten_edits.render(&read_back);
        let native = ir
            .native
            .namespace("sldprt")
            .map(|namespace| native::SldprtNative::load(namespace).map_err(CodecError::from))
            .transpose()?;
        let stale_solid = history::write::brep_agreement::agreement(
            ir,
            digests,
            native.as_ref(),
            image.map(history::write::brep_agreement::Source::Image),
            &written,
            &read_back.misread,
        )?
        .losses;
        Ok(Written::Semantic {
            path: if records.is_empty() {
                SemanticPath::Synthesized
            } else {
                SemanticPath::Patched
            },
            dialect,
            fidelity,
            coverage,
            stale_solid,
            unwritten_edits,
            unwritten,
            losses,
        })
    }
}

/// Why an eligible retained-image replay took the semantic path instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaySkipped {
    DigestMismatch,
    ImageMissing,
    /// The patch path cannot write the edited document, which is
    /// synthesized without its `SolidWorks` history.
    PatchDeclined,
}

impl ReplaySkipped {
    fn consumption(self) -> Consumption {
        Consumption::Degraded {
            reason: match self {
                Self::DigestMismatch => {
                    "decoded model no longer matches the preserved SLDPRT source digest"
                }
                Self::ImageMissing => "preserved SLDPRT source image is unavailable",
                Self::PatchDeclined => {
                    "the SLDPRT patch path cannot write the edited document, which is synthesized \
                     without its SolidWorks history"
                }
            }
            .into(),
        }
    }
}

/// How a semantic write consumed the fidelity it was given. A replay skip
/// carries the sole typed reason from which both the consumption and any
/// applicable loss are derived.
enum SemanticFidelity {
    NotConsumed,
    ReplaySkipped(ReplaySkipped),
}

impl SemanticFidelity {
    fn consumption(&self) -> Consumption {
        match self {
            Self::NotConsumed => Consumption::NotConsumed,
            Self::ReplaySkipped(reason) => reason.consumption(),
        }
    }
}

/// The two ways the semantic writer can produce a document. A verbatim replay
/// is not one of them, so a replay can never carry a semantic skip reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemanticPath {
    /// Retained source records fed the write.
    Patched,
    /// The document was built from the neutral IR alone.
    Synthesized,
}

impl SemanticPath {
    fn write_path(self, consumption: Consumption) -> WritePath {
        match self {
            Self::Patched => WritePath::Patched {
                consumption: PatchConsumption::Independent(consumption),
            },
            Self::Synthesized => WritePath::Synthesized { consumption },
        }
    }
}

/// One completed write.
enum Written {
    /// The retained source image, byte for byte. The dialect is the source's
    /// own, which this write neither chooses nor needs to name.
    Replayed,
    /// The retained source image, byte for byte, written for an edited
    /// document that neither the patch nor the synthesis path writes in the
    /// source's dialect, or whose history its retained source does not
    /// restore (§3.4, Paths item 4): `losses` name each edit it keeps as
    /// stored, or the file where no record is isolated.
    Stored {
        losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    },
    /// A semantic write, in the dialect the emitted `swSolidWorks` envelope
    /// declares.
    Semantic {
        path: SemanticPath,
        dialect: DialectId,
        fidelity: SemanticFidelity,
        coverage: ArenaDispositions,
        /// One loss per construction edit that shapes the solid: the output
        /// keeps the stored solid, which shows the state before the edit.
        stale_solid: Vec<cadmpeg_ir::report::loss::LossNote>,
        /// One loss per edit the native records cannot carry: the output keeps
        /// the stored record or value.
        unwritten_edits: Vec<cadmpeg_ir::report::loss::LossNote>,
        /// The metadata attributes the write did not write.
        unwritten: writer::UnwrittenAttributes,
        /// The write's other losses.
        losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    },
}

impl Written {
    /// Which branch ran. The semantic writer can reproduce its input byte for
    /// byte, so the output cannot say and only this value can.
    fn path(&self) -> WritePath {
        match self {
            Self::Replayed | Self::Stored { .. } => WritePath::VerbatimReplay,
            Self::Semantic { path, fidelity, .. } => path.write_path(fidelity.consumption()),
        }
    }

    /// Which model arenas the output carries. A replay runs only while the
    /// document digest matches its decode baseline, so it carries every arena.
    fn coverage(&self) -> ArenaCoverage {
        match self {
            Self::Replayed => ArenaCoverage::Complete,
            // The stored part may represent a record of any arena; the loss
            // names the edits it does not carry.
            Self::Stored { .. } => ArenaCoverage::Declared(writer::stored_part_coverage()),
            Self::Semantic { coverage, .. } => ArenaCoverage::Declared(*coverage),
        }
    }

    /// What the output does with each native arena (§3.4, Rule W3). A replay
    /// holds every record of the source; a semantic write states the records
    /// of the arenas it loads from neutral and native content. Neither holds a
    /// record of another format.
    fn native(&self) -> NativeDispositions {
        let mut native = NativeDispositions::dropped();
        match self {
            Self::Replayed | Self::Stored { .. } => {
                native.namespace("sldprt", NativeDisposition::Written);
            }
            Self::Semantic { .. } => {
                for arena in crate::native::SLDPRT_ARENA_NAMES {
                    native.arena("sldprt", arena, NativeDisposition::Regenerated);
                }
            }
        }
        native
    }

    /// The semantic path that ran instead of an eligible replay, with why.
    fn replay_skipped(&self) -> Option<(SemanticPath, ReplaySkipped)> {
        match self {
            Self::Semantic {
                path,
                fidelity: SemanticFidelity::ReplaySkipped(reason),
                ..
            } => Some((*path, *reason)),
            _ => None,
        }
    }
}

impl CodecBackend for SldprtCodec {
    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);

    fn validate_native(
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        _source: Option<&cadmpeg_ir::SourceFidelity>,
    ) -> Result<Vec<Finding>, CodecError> {
        resolved_features::validate::validate_native(ctx, ir)
    }

    fn detect_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        prefix: cadmpeg_core::decode::View<'_>,
    ) -> Result<Confidence, cadmpeg_core::CodecError> {
        let prefix = prefix.window();
        if container::looks_like_sldprt(ctx, prefix)? {
            Ok(Confidence::High)
        } else {
            Ok(Confidence::No)
        }
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(ctx, root)?;
        let classification = dialect::classify_layers(&scan)?;
        let mut summary = container::summarize(&scan, classification.layers().clone());
        classification.append_losses(&mut summary.losses);
        summary.losses.extend(
            scan.given_up
                .iter()
                .map(cadmpeg_ir::unknown::part::GivenUpPart::loss),
        );
        summary.losses.extend(
            scan.noncanonical.iter().map(|deviation| {
                loss::SldprtLossCode::ContainerNoncanonical.note(deviation.clone())
            }),
        );
        Ok(summary)
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        decode::decode(ctx, root)
    }
}

impl EncoderBackend for SldprtCodec {
    const FORMAT: FormatId = <Self as CodecBackend>::FORMAT;
    /// Patches the retained source, so it plans from the design records it
    /// rewrites.
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(dialect::TARGETS, Some(0));

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        writer::target::plan(input, &target)
    }

    fn restore(
        &self,
        input: EncodeInput<'_>,
        target: &ResolvedWrite<'_>,
        notes: &mut Vec<String>,
        losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    ) -> Result<Option<CadIr>, CodecError> {
        writer::saved::restore(input, target, notes, losses)
    }
}

fn source_records<'a>(
    ir: &CadIr,
    source_fidelity: &'a SourceFidelity,
) -> Result<Vec<SourceRecord<'a>>, CodecError> {
    let mut records = ir
        .native_unknowns_iter("sldprt")
        .map(|reference| {
            let reference = reference?;
            let retained = source_fidelity
                .retained_record(reference.id.as_str())
                .ok_or_else(|| {
                    cadmpeg_ir::native::NativeConvertError::MissingRetainedSourceRecord(
                        reference.id.as_str().to_owned(),
                    )
                })?;
            Ok(SourceRecord {
                id: reference.id,
                data: retained.data(),
            })
        })
        .collect::<Result<Vec<_>, cadmpeg_ir::native::NativeConvertError>>()?;
    if let Some(source) = source_fidelity.retained_record(SOURCE_IMAGE_ID) {
        records.push(SourceRecord {
            id: source_image_id(),
            data: source.data(),
        });
    }
    Ok(records)
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests {
    use super::SOURCE_IMAGE_ID;

    #[test]
    fn source_record_join_borrows_the_retained_source_image() {
        let payload = vec![0x5a; 4096];
        let payload_ptr = payload.as_ptr();
        let mut fidelity = cadmpeg_ir::SourceFidelity::new(
            cadmpeg_ir::hash::digest::Sha256Digest::digest(&payload),
        );
        let id: cadmpeg_ir::ids::UnknownId = SOURCE_IMAGE_ID
            .to_owned()
            .try_into()
            .expect("source image identity");
        let record = cadmpeg_ir::source_fidelity::RetainedSourceRecord::from_bytes(
            "source",
            0,
            cadmpeg_ir::source_fidelity::RetainedBytes::Inline { data: payload },
        )
        .expect("source image extent");
        fidelity
            .insert_retained_record(id, record)
            .expect("source image identity is unique");

        let records = crate::source_records(
            &cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted"),
            &fidelity,
        )
        .unwrap();
        let retained = records[0].data.expect("retained source bytes");
        assert_eq!(retained.as_ptr(), payload_ptr);
    }
}
