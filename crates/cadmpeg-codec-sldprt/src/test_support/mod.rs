// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for crate tests.

#![allow(clippy::unwrap_used)]

use std::io::Write;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{report::export::WritePath, CadIr, SourceFidelity};

use crate::SldprtCodec;

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
            Default::default(),
        ));
    }
    let mut fidelity = input.fidelity.cloned().unwrap_or_default();
    let source = match fidelity.retained_record(crate::SOURCE_IMAGE_ID) {
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
            Default::default(),
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

/// Check public refusal, then exercise the internal history section encoder.
pub(crate) fn serialize_history_in_native_envelope_after_refusal(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<cadmpeg_core::dialect::DialectId, CodecError> {
    let (ir, fidelity) = fixture_native_envelope_context(EncodeInput::new(ir, Some(fidelity)))?;
    let error = plan_inherited_write(&ir, &fidelity, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(&error, CodecError::NotImplemented(detail) if detail.starts_with("SLDPRT writer cannot regenerate B-rep after")),
        "{error}"
    );
    let records = crate::source_records(&ir, &fidelity)?;
    crate::writer::write_semantic_with_records(&ir, &fidelity.annotations, &records, writer)
        .map(|output| output.dialect)
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
pub(crate) mod container;
pub(crate) mod history;
pub(crate) mod ir;
pub(crate) mod native;
pub(crate) mod parasolid;
pub(crate) mod pmi;
pub(crate) mod tessellation;
