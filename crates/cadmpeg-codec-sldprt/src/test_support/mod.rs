// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for crate tests.

#![allow(clippy::unwrap_used)]

use std::io::Write;

use crate::SldprtCodec;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{report::export::WritePath, CadIr, SourceFidelity};

/// Plans an inherited write through the sealed encoder and writes its bytes.
pub(crate) fn plan_inherited_write(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, CodecError> {
    let plan = SldprtCodec.plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)?;
    Ok(plan.write_to(writer)?.write_path().clone())
}

/// Supply an authored native envelope for semantic fixture assertions.
pub(crate) fn plan_fixture_with_native_envelope(
    input: EncodeInput<'_>,
    request: TargetRequest<'_>,
) -> Result<cadmpeg_ir::codec::write::ExportPlan, CodecError> {
    let (ir, fidelity) = fixture_native_envelope_context(input)?;
    SldprtCodec.plan(EncodeInput::new(&ir, Some(&fidelity)), request)
}

fn fixture_native_envelope_context(
    input: EncodeInput<'_>,
) -> Result<(CadIr, SourceFidelity), CodecError> {
    let mut ir = input.ir.clone();
    if input.fidelity.is_none() {
        ir.set_native_unknowns(
            &cadmpeg_test_support::service_decode_context(),
            "sldprt",
            &[],
        )?;
        ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(crate::dialect::SldprtDialect::classify(None)),
            std::collections::BTreeMap::default(),
        ));
    }
    let cloned = input.fidelity.cloned();
    let source = match cloned
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
    let source =
        if crate::container::native_layout::source_envelope(&source, &scan.blocks)?.is_some() {
            source
        } else {
            container::native_envelope_fixture(&source)
        };
    let mut fidelity = cloned.unwrap_or_else(|| {
        SourceFidelity::with_annotations(
            cadmpeg_ir::Annotations::default(),
            cadmpeg_ir::hash::digest::Sha256Digest::digest(&source),
        )
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
    Ok((ir, fidelity))
}

pub(crate) fn plan_fixture_write_with_native_envelope(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, CodecError> {
    let plan = plan_fixture_with_native_envelope(
        EncodeInput::new(ir, Some(fidelity)),
        TargetRequest::Inherit,
    )?;
    Ok(plan.write_to(writer)?.write_path().clone())
}

/// Exercise the sealed writer after construction controls changed.
pub(crate) fn serialize_edited_history(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<(), CodecError> {
    let plan = SldprtCodec.plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)?;
    plan.write_to(writer)?;
    Ok(())
}

/// A declined native layout writes the document through synthesis and reports it.
/// Returns the reason the patch path declined.
pub(crate) fn assert_synthesis_fallback(ir: &CadIr, fidelity: &SourceFidelity) -> String {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let reason = match plan.report().synthesis_cause() {
        Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined { reason }) => reason.clone(),
        other => panic!("expected a declined patch, got {other:?}"),
    };
    assert!(!reason.is_empty());
    assert!(!plan.report().losses.is_empty());
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = SldprtCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.bodies.len(), ir.model.bodies.len());
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len());
    reason
}

/// Independently synthesize current content and compare every Written arena.
pub(crate) fn assert_source_free_synthesis_agrees(
    ir: &CadIr,
    retained_output: &CadIr,
) -> cadmpeg_ir::codec::DecodeResult {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let synthesized = SldprtCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    cadmpeg_test_support::contract::assert_written_arenas_agree(
        &report,
        retained_output,
        synthesized.ir(),
    );
    synthesized
}

pub(crate) fn make_source_image_unavailable(fidelity: &mut SourceFidelity) {
    let unavailable = {
        let record = fidelity
            .retained_record(crate::SOURCE_IMAGE_ID)
            .expect("decode retains the source image");
        let digest = cadmpeg_ir::hash::digest::Sha256Digest::try_from(record.sha256().as_str())
            .expect("decoded source image digest");
        cadmpeg_ir::RetainedSourceRecord::from_bytes(
            record.stream().to_owned(),
            record.offset(),
            cadmpeg_ir::source_fidelity::RetainedBytes::Digest {
                byte_len: record.byte_len(),
                sha256: digest,
            },
        )
        .expect("source image extent")
    };
    fidelity
        .remove_retained_record(crate::SOURCE_IMAGE_ID)
        .expect("decode retains the source image");
    fidelity
        .insert_retained_record(
            crate::SOURCE_IMAGE_ID
                .to_owned()
                .try_into()
                .expect("source image identity"),
            unavailable,
        )
        .expect("source image identity is unique");
}

pub(crate) mod allocation;
pub(crate) mod appearance;
pub(crate) mod carchive;
pub(crate) mod container;
pub(crate) mod history;
pub(crate) mod ir;
pub(crate) mod native;
pub(crate) mod parasolid;
pub(crate) mod pmi;
pub(crate) mod surfaces;
pub(crate) mod tessellation;

pub(crate) mod dimensions;
