// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for crate tests.

#![allow(clippy::unwrap_used)]

use std::io::Write;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::EncodeError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{CadIr, SourceFidelity};

use crate::SldprtCodec;

/// One write in a session, which keeps the written part as the write read
/// it back.
struct SessionWrite<'s, 'f> {
    session: &'s crate::SldprtWriteSession<'f>,
    read_back: std::cell::Cell<Option<std::sync::Arc<cadmpeg_ir::codec::DecodeResult>>>,
}

impl cadmpeg_ir::codec::write::EncoderBackend for SessionWrite<'_, '_> {
    const FORMAT: cadmpeg_ir::codec::FormatId =
        <SldprtCodec as cadmpeg_ir::codec::write::EncoderBackend>::FORMAT;
    const NAME: &'static str = <SldprtCodec as cadmpeg_ir::codec::write::EncoderBackend>::NAME;
    const REQUEST: cadmpeg_core::decode::DecodeDepth =
        <SldprtCodec as cadmpeg_ir::codec::write::EncoderBackend>::REQUEST;
    const REUSES_SOURCE: bool =
        <SldprtCodec as cadmpeg_ir::codec::write::EncoderBackend>::REUSES_SOURCE;
    const DEFINITION_PER_FILE: bool =
        <SldprtCodec as cadmpeg_ir::codec::write::EncoderBackend>::DEFINITION_PER_FILE;
    type Target = cadmpeg_ir::codec::write::target::Catalog;
    const TARGET: cadmpeg_ir::codec::write::target::Catalog =
        <SldprtCodec as cadmpeg_ir::codec::write::EncoderBackend>::TARGET;

    fn plan_resolved(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        input: EncodeInput<'_>,
        target: cadmpeg_ir::codec::write::target::ResolvedWrite<'_>,
    ) -> Result<cadmpeg_ir::codec::write::ExportBody, CodecError> {
        let (body, read_back) = crate::writer::target::plan(ctx, input, &target, self.session)?;
        self.read_back.set(read_back);
        Ok(body)
    }

    fn restore(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        input: EncodeInput<'_>,
        target: &cadmpeg_ir::codec::write::target::ResolvedWrite<'_>,
        notes: &mut Vec<String>,
        losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    ) -> Result<Option<CadIr>, CodecError> {
        crate::writer::saved::restore(ctx, input, target, notes, losses)
    }
}

/// Plans the write of `input` in `session` as `SldprtCodec` plans a
/// write that names no target, and returns the plan and the written part as the write
/// read it back, when it did.
pub(crate) fn plan_in_session(
    session: &crate::SldprtWriteSession<'_>,
    input: EncodeInput<'_>,
) -> Result<
    (
        cadmpeg_ir::codec::write::ExportPlan,
        Option<std::sync::Arc<cadmpeg_ir::codec::DecodeResult>>,
    ),
    EncodeError,
> {
    let write = SessionWrite {
        session,
        read_back: std::cell::Cell::new(None),
    };
    let plan = write.plan(input)?;
    Ok((plan, write.read_back.into_inner()))
}

/// Plans a write that names no target, which inherits the dialect of a
/// same-format source, through the sealed encoder and writes its bytes.
pub(crate) fn plan_inherited_write(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, cadmpeg_ir::codec::write::EncodeError> {
    let plan = SldprtCodec.plan(EncodeInput::new(ir, Some(fidelity)))?;
    Ok(plan.write_to(writer)?.write_path().clone())
}

/// Plans a write that names no target and whose patch path declines the
/// edited document:
/// the writer synthesizes it without its `SolidWorks` history (§3.4, Paths).
/// Checks that the output decodes, and returns the synthesis cause the
/// report states, which says why the patch declined.
pub(crate) fn plan_inherited_write_declined(ir: &CadIr, fidelity: &SourceFidelity) -> String {
    let mut output = Vec::new();
    let report = SldprtCodec
        .plan(EncodeInput::new(ir, Some(fidelity)))
        .unwrap()
        .write_to(&mut output)
        .unwrap();
    declined(&report, output)
}

/// [`plan_inherited_write_declined`] with an authored native envelope.
pub(crate) fn plan_fixture_write_with_native_envelope_declined(
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> String {
    let mut output = Vec::new();
    let report = plan_fixture_with_native_envelope(
        EncodeInput::new(ir, Some(fidelity)),
        TargetRequest::Unnamed,
    )
    .unwrap()
    .write_to(&mut output)
    .unwrap();
    declined(&report, output)
}

fn declined(report: &cadmpeg_ir::report::export::ExportReport, output: Vec<u8>) -> String {
    assert!(
        matches!(report.write_path(), WritePath::Synthesized { .. }),
        "{:?}",
        report.write_path()
    );
    decode_bytes(output);
    let Some(cadmpeg_ir::report::export::SynthesisCause::PatchDeclined { reason }) =
        report.synthesis_cause()
    else {
        panic!("the write states no synthesis cause: {:?}", report.losses)
    };
    reason.clone()
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
    let plan = SldprtCodec.plan(EncodeInput::new(ir, Some(fidelity)))?;
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
    SldprtCodec.plan(EncodeInput::new(&ir, Some(&fidelity)).with_target(request))
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
        TargetRequest::Unnamed,
    )?;
    Ok(plan.write_to(writer)?.write_path().clone())
}

fn decode_bytes(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    SldprtCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

/// Writes through the sealed encoder with an authored native envelope, and
/// checks that no stale-solid loss calls an edit written. Returns the report,
/// the written part read back, and the source fidelity the envelope wraps.
fn write_without_stale_solid(
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> (
    cadmpeg_ir::report::export::ExportReport,
    std::sync::Arc<cadmpeg_ir::codec::DecodeResult>,
    SourceFidelity,
) {
    let (ir, fidelity) = fixture_native_envelope_context(EncodeInput::new(ir, Some(fidelity)))
        .expect("the authored envelope context");
    let (plan, read_back) = plan_in_session(
        &crate::SldprtWriteSession::new(Some(&fidelity)),
        EncodeInput::new(&ir, Some(&fidelity)),
    )
    .unwrap();
    let mut output = Vec::new();
    let report = plan.write_to(&mut output).unwrap();
    assert_eq!(
        loss_messages(
            &report,
            crate::loss::SldprtLossCode::WriterSolidStaleAfterEdit
        ),
        Vec::<String>::new()
    );
    let written = read_back.unwrap_or_else(|| {
        std::sync::Arc::new(
            crate::decode::decode_for_read_back(
                &output,
                cadmpeg_core::decode::ResourceLimits::desktop(),
            )
            .expect("the written part decodes"),
        )
    });
    (report, written, fidelity)
}

/// Whether `kind` is `export:edit.dropped`, of any entity kind.
pub(crate) fn is_edit_dropped(kind: &cadmpeg_ir::report::loss::LossKind) -> bool {
    kind.local_code() == "edit.dropped"
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
) -> (Vec<String>, std::sync::Arc<cadmpeg_ir::codec::DecodeResult>) {
    let (report, written, _) = write_without_stale_solid(ir, fidelity);
    (
        report
            .losses
            .iter()
            .filter(|loss| is_edit_dropped(&loss.code))
            .map(|loss| loss.message.clone())
            .collect(),
        written,
    )
}

/// Writes through the sealed encoder with an authored native envelope and
/// returns the messages of the feature edits the output keeps as stored, and
/// the written part read back. The write carries no other edit than a
/// rename, so the output reads back as the source document under the edited
/// names, and no stale-solid loss calls the edit written.
pub(crate) fn unwritten_feature_edits(
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> (Vec<String>, std::sync::Arc<cadmpeg_ir::codec::DecodeResult>) {
    let (report, written, written_from) = write_without_stale_solid(ir, fidelity);
    let unwritten = report
        .losses
        .iter()
        .filter(|loss| is_edit_dropped(&loss.code))
        .map(|loss| loss.message.clone())
        .collect::<Vec<_>>();
    let messages = report
        .losses
        .iter()
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    // The write reads the source image the authored envelope wraps.
    let image = written_from
        .retained_record(crate::SOURCE_IMAGE_ID)
        .and_then(|record| record.data())
        .unwrap();
    let decoded;
    let source = match nist::decoded(image) {
        Some(specimen) => specimen,
        None => {
            decoded = decode_bytes(image.to_vec());
            &decoded
        }
    };
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
                &readback::baseline(source)
            ),
            &messages
        ),
        Vec::new()
    );
    (unwritten, written)
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
        .plan(EncodeInput::new(&ir, Some(&fidelity)))?
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

/// Runs the semantic writer over an authored native envelope in a write
/// session under `limits` and returns what it reports.
pub(crate) fn write_in_native_envelope(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    limits: cadmpeg_core::decode::ResourceLimits,
    writer: &mut dyn Write,
) -> Result<crate::writer::SemanticOutput, CodecError> {
    let (ir, fidelity) = fixture_native_envelope_context(EncodeInput::new(ir, Some(fidelity)))?;
    let records = crate::source_records(&ir, &fidelity)?;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy { limits };
    let ctx = cadmpeg_core::decode::DecodeContext::new(
        &arena,
        &policy,
        cadmpeg_core::decode::DecodeDepth::Full,
    );
    let source = crate::writer::RetainedImage::open(&ctx, &records)?;
    crate::writer::write_semantic_with_records_cached(
        &ir,
        &fidelity.annotations,
        &fidelity.codec_digests,
        &records,
        crate::writer::SemanticWriteControls {
            withheld: &[],
            shaped: &[],
            ctx: &ctx,
            source: source.as_ref(),
        },
        &mut crate::writer::SemanticWriteCache::default(),
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
