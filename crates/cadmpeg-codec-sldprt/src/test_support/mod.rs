// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for crate tests.

#![allow(clippy::unwrap_used)]

use std::io::Write;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::EncodeError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{report::export::WritePath, CadIr, SourceFidelity};

use crate::SldprtCodec;

/// Plans an inherited write through the sealed encoder and writes its bytes.
pub(crate) fn plan_inherited_write(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, cadmpeg_ir::codec::write::EncodeError> {
    let plan = SldprtCodec.plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)?;
    Ok(plan.write_to(writer)?.write_path().clone())
}

/// The messages of a write's unwritten-attribute losses, by code.
#[derive(Debug, Default)]
pub(crate) struct UnwrittenLosses {
    /// `export:edit.dropped`: records kept as the source stored them.
    pub(crate) dropped: Vec<String>,
    /// `WriterAttributesOmitted`: attributes with no record.
    pub(crate) omitted: Vec<String>,
}

impl UnwrittenLosses {
    pub(crate) fn of(losses: &[cadmpeg_ir::report::loss::LossNote]) -> Self {
        let mut found = Self::default();
        for loss in losses {
            if loss.code.namespace() == "export" && loss.code.local_code() == "edit.dropped" {
                found.dropped.push(loss.message.clone());
            } else if loss.code == crate::loss::SldprtLossCode::WriterAttributesOmitted.kind() {
                found.omitted.push(loss.message.clone());
            }
        }
        found
    }

    /// One message of the code `held` selects states `reason` and names `id`
    /// as a whole token, and no message of the other code names it.
    pub(crate) fn assert_charged(&self, held: crate::writer::Held, reason: &str, id: &str) {
        let (charged, other) = match held {
            crate::writer::Held::Source => (&self.dropped, &self.omitted),
            crate::writer::Held::Nothing => (&self.omitted, &self.dropped),
        };
        let names = |message: &String| message.split([' ', ',']).any(|token| token == id);
        assert!(
            charged
                .iter()
                .any(|message| message.contains(reason) && names(message)),
            "{held:?} {reason} {id}: {self:?}"
        );
        assert!(!other.iter().any(names), "{held:?} {id}: {self:?}");
    }

    /// No message of either code names `id`.
    pub(crate) fn assert_uncharged(&self, id: &str) {
        let names = |message: &String| message.split([' ', ',']).any(|token| token == id);
        assert!(
            !self.dropped.iter().chain(&self.omitted).any(names),
            "{id}: {self:?}"
        );
    }
}

/// Plans and writes an inherited write. Returns its bytes and its
/// unwritten-attribute losses.
pub(crate) fn write_with_unwritten_attributes(
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> Result<(Vec<u8>, UnwrittenLosses), cadmpeg_ir::codec::write::EncodeError> {
    let plan = SldprtCodec.plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)?;
    let losses = UnwrittenLosses::of(&plan.report().losses);
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)?;
    Ok((bytes, losses))
}

/// Supply an authored native envelope for semantic fixture assertions.
pub(crate) fn plan_fixture_with_native_envelope(
    input: EncodeInput<'_>,
    request: TargetRequest<'_>,
) -> Result<cadmpeg_ir::codec::write::ExportPlan, EncodeError> {
    let (ir, fidelity) = fixture_native_envelope_context(input).map_err(EncodeError::Backend)?;
    SldprtCodec.plan(EncodeInput::new(&ir, Some(&fidelity)), request)
}

fn fixture_native_envelope_context(
    input: EncodeInput<'_>,
) -> Result<(CadIr, SourceFidelity), CodecError> {
    let mut ir = input.ir.clone();
    if input.fidelity.is_none() {
        ir.set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "sldprt",
            &[],
        )?;
        ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(crate::dialect::SldprtDialect::classify(None)),
            std::collections::BTreeMap::default(),
        ));
    }
    let fidelity = input.fidelity.cloned();
    let source = match fidelity
        .as_ref()
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
    {
        Some(record) => record
            .data()
            .ok_or_else(|| {
                CodecError::NotImplemented(
                    "authored fixture requires available native envelope source bytes".into(),
                )
            })?
            .to_vec(),
        None => {
            assert!(
                input.fidelity.is_none()
                    || ir
                        .source
                        .as_ref()
                        .is_none_or(|source| source.format() != "sldprt"),
                "a missing native source image is a contract state, not an authored fixture"
            );
            let mut source = container::outer_header();
            source.extend(container::make_block(
                0,
                "FixtureEnvelopeTokenSource",
                b"authored envelope fixture",
            ));
            source
        }
    };
    let scan = container::scan(&source);
    let source = if crate::container::native_layout::source_envelope(&source, &scan.blocks, &[])?
        .is_some()
    {
        source
    } else {
        container::native_envelope_fixture(&source)
    };
    let mut fidelity = fidelity.unwrap_or_else(|| {
        SourceFidelity::new(cadmpeg_ir::hash::digest::Sha256Digest::digest(&source))
    });
    let _previous_image = fidelity.remove_retained_record(crate::SOURCE_IMAGE_ID);
    fidelity.insert_retained_record(
        crate::source_image_id(),
        cadmpeg_ir::RetainedSourceRecord::from_bytes(
            "fixture-native-envelope",
            0,
            cadmpeg_ir::source_fidelity::RetainedBytes::Inline { data: source },
        )
        .expect("authored source image extent"),
    )?;
    if ir
        .source
        .as_ref()
        .is_none_or(|source| source.format() != "sldprt")
    {
        ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(crate::dialect::SldprtDialect::classify(None)),
            std::collections::BTreeMap::default(),
        ));
    }
    ir.state_root_definition(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        crate::decode::root_definition_id(),
        cadmpeg_ir::products::ExternalDocument::default(),
    )?;
    Ok((ir, fidelity))
}

pub(crate) fn plan_fixture_write_with_native_envelope(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, EncodeError> {
    let plan = plan_fixture_with_native_envelope(
        EncodeInput::new(ir, Some(fidelity)),
        TargetRequest::Inherit,
    )?;
    Ok(plan.write_to(writer)?.write_path().clone())
}

fn decode_bytes(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    SldprtCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

/// Writes through the public encoder with an authored native envelope, and
/// checks that no stale-solid loss calls an edit written. Returns the report
/// and the written part read back.
fn write_without_stale_solid(
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> (
    cadmpeg_ir::report::export::ExportReport,
    cadmpeg_ir::codec::DecodeResult,
) {
    let mut output = Vec::new();
    let report = plan_fixture_with_native_envelope(
        EncodeInput::new(ir, Some(fidelity)),
        TargetRequest::Inherit,
    )
    .unwrap()
    .write_to(&mut output)
    .unwrap();
    assert_eq!(
        loss_messages(
            &report,
            crate::loss::SldprtLossCode::WriterSolidStaleAfterEdit
        ),
        Vec::<String>::new()
    );
    (report, decode_bytes(output))
}

fn loss_messages(
    report: &cadmpeg_ir::report::export::ExportReport,
    code: crate::loss::SldprtLossCode,
) -> Vec<String> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .map(|loss| loss.message.clone())
        .collect()
}

/// Writes through the public encoder with an authored native envelope a
/// document whose feature edits the output keeps as stored, and checks that
/// no stale-solid loss calls an edit written. Returns the messages of the
/// unwritten feature edits and the written part read back.
pub(crate) fn write_unwritten_feature_edits(
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> (Vec<String>, cadmpeg_ir::codec::DecodeResult) {
    let (report, written) = write_without_stale_solid(ir, fidelity);
    (
        loss_messages(
            &report,
            crate::loss::SldprtLossCode::WriterFeatureEditUnwritten,
        ),
        written,
    )
}

/// Writes through the public encoder with an authored native envelope and
/// returns the messages of the feature edits the output keeps as stored. The
/// write carries no other edit than a rename, so the output reads back as the
/// source document under the edited names, and no stale-solid loss calls the
/// edit written.
pub(crate) fn unwritten_feature_edits(ir: &CadIr, fidelity: &SourceFidelity) -> Vec<String> {
    let (report, written) = write_without_stale_solid(ir, fidelity);
    let unwritten = loss_messages(
        &report,
        crate::loss::SldprtLossCode::WriterFeatureEditUnwritten,
    );
    let messages = report
        .losses
        .iter()
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    // The write reads the source image the authored envelope wraps.
    let (_, written_from) = fixture_native_envelope_context(EncodeInput::new(ir, Some(fidelity)))
        .expect("the authored envelope context");
    let source = written_from
        .retained_record(crate::SOURCE_IMAGE_ID)
        .and_then(|record| record.data())
        .unwrap()
        .to_vec();
    let source = decode_bytes(source);
    let renames = source
        .ir()
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let name = ir
                .model
                .features
                .iter()
                .find(|edited| edited.id == feature.id)?
                .name
                .clone()?;
            let old = feature.name.clone().filter(|old| *old != name)?;
            Some((feature.id.as_str().to_owned(), old, name))
        })
        .collect::<Vec<_>>();
    let expected = readback::renamed(source.ir(), &renames);
    assert_eq!(
        readback::unnamed(
            readback::beyond(
                readback::diff(&expected, written.ir()),
                &readback::baseline(&source)
            ),
            &messages
        ),
        Vec::new()
    );
    unwritten
}

/// Write an edit that shapes the solid through the public encoder, and check
/// that the export report names the stored solid as stale.
pub(crate) fn write_history_over_stale_solid(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<cadmpeg_ir::report::export::ExportReport, cadmpeg_ir::codec::write::EncodeError> {
    let (ir, fidelity) = fixture_native_envelope_context(EncodeInput::new(ir, Some(fidelity)))
        .map_err(cadmpeg_ir::codec::write::EncodeError::Backend)?;
    let report = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )?
        .write_to(writer)?;
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::SldprtLossCode::WriterSolidStaleAfterEdit.kind()),
        "{:?}",
        report.losses
    );
    Ok(report)
}

/// Runs the semantic writer over an authored native envelope and returns what
/// it reports.
pub(crate) fn write_in_native_envelope(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<crate::writer::SemanticOutput, CodecError> {
    let (ir, fidelity) = fixture_native_envelope_context(EncodeInput::new(ir, Some(fidelity)))?;
    let records = crate::source_records(&ir, &fidelity)?;
    crate::writer::write_semantic_with_records(
        &ir,
        &fidelity.annotations,
        &fidelity.codec_digests,
        &records,
        &[],
        writer,
    )
}

pub(crate) fn make_source_image_unavailable(fidelity: &mut SourceFidelity) {
    make_retained_record_unavailable(fidelity, crate::SOURCE_IMAGE_ID);
}

/// Replace retained record `id` by its digest: the fidelity names the record
/// and holds none of its bytes.
pub(crate) fn make_retained_record_unavailable(fidelity: &mut SourceFidelity, id: &str) {
    let unavailable = {
        let record = fidelity
            .retained_record(id)
            .expect("decode retains the record");
        let digest = cadmpeg_ir::hash::digest::Sha256Digest::try_from(record.sha256().as_str())
            .expect("decoded record digest");
        cadmpeg_ir::RetainedSourceRecord::from_bytes(
            record.stream().to_owned(),
            record.offset(),
            cadmpeg_ir::source_fidelity::RetainedBytes::Digest {
                byte_len: record.byte_len(),
                sha256: digest,
            },
        )
        .expect("record extent")
    };
    fidelity
        .remove_retained_record(id)
        .expect("decode retains the record");
    fidelity
        .insert_retained_record(
            id.to_owned().try_into().expect("record identity"),
            unavailable,
        )
        .expect("record identity is unique");
}

pub(crate) mod allocation;
pub(crate) mod appearance;
pub(crate) mod carchive;
pub(crate) mod container;
pub(crate) mod history;
pub(crate) mod ir;
pub(crate) mod native;
pub(crate) mod nist;
pub(crate) mod parasolid;
pub(crate) mod pmi;
pub(crate) mod readback;
pub(crate) mod sketch_records;
pub(crate) mod surfaces;
pub(crate) mod swift;
pub(crate) mod tessellation;

/// The machine-local digest of `ir` without `scope`, by which a test compares
/// two decodes of one part; see [`cadmpeg_ir::hash::document_local_sha256`].
pub(crate) fn document_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let digest = cadmpeg_ir::hash::document_local_sha256(
        &ctx,
        ir,
        ir.source.as_ref(),
        "sldprt",
        "sldprt:file:source-image#0",
    )?;
    ctx.finish_session()?;
    Ok(digest)
}
