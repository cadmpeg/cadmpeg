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
    let source =
        if crate::container::native_layout::source_envelope(&source, &scan.blocks)?.is_some() {
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
) -> Result<WritePath, CodecError> {
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

/// Writes through the public encoder with an authored native envelope a
/// document whose feature edits the output keeps as stored, and checks that
/// no stale-solid loss calls an edit written. Returns the messages of the
/// unwritten feature edits and the written part read back.
pub(crate) fn write_unwritten_feature_edits(
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> (Vec<String>, cadmpeg_ir::codec::DecodeResult) {
    let mut output = Vec::new();
    let report = plan_fixture_with_native_envelope(
        EncodeInput::new(ir, Some(fidelity)),
        TargetRequest::Inherit,
    )
    .unwrap()
    .write_to(&mut output)
    .unwrap();
    let messages = |code: crate::loss::SldprtLossCode| {
        report
            .losses
            .iter()
            .filter(|loss| loss.code == code.kind())
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        messages(crate::loss::SldprtLossCode::WriterSolidStaleAfterEdit),
        Vec::<String>::new()
    );
    (
        messages(crate::loss::SldprtLossCode::WriterFeatureEditUnwritten),
        decode_bytes(output),
    )
}

/// Writes through the public encoder with an authored native envelope and
/// returns the messages of the feature edits the output keeps as stored. The
/// write carries no other edit than a rename, so the output reads back as the
/// source document under the edited names, and no stale-solid loss calls the
/// edit written.
pub(crate) fn unwritten_feature_edits(ir: &CadIr, fidelity: &SourceFidelity) -> Vec<String> {
    let (unwritten, written) = write_unwritten_feature_edits(ir, fidelity);
    let source = fidelity
        .retained_record(crate::SOURCE_IMAGE_ID)
        .and_then(|record| record.data())
        .unwrap()
        .to_vec();
    let mut expected = decode_bytes(source).ir().clone();
    for feature in &mut expected.model.features {
        let Some(name) = ir
            .model
            .features
            .iter()
            .find(|edited| edited.id == feature.id)
            .and_then(|edited| edited.name.clone())
        else {
            continue;
        };
        let Some(old) = feature
            .name
            .replace(name.clone())
            .filter(|old| *old != name)
        else {
            continue;
        };
        // A reference `name@feature` names the feature by name.
        let (from, to) = (format!("@{old}"), format!("@{name}"));
        for parameter in &mut expected.model.parameters {
            parameter.expression = parameter.expression.replace(&from, &to);
            for value in parameter.properties.values_mut() {
                *value = value.replace(&from, &to);
            }
        }
    }
    assert_eq!(
        readback::differences(&expected, written.ir(), &[]),
        Vec::<String>::new()
    );
    unwritten
}

/// Write an edit that shapes the solid through the public encoder, and check
/// that the export report names the stored solid as stale.
pub(crate) fn write_history_over_stale_solid(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<cadmpeg_ir::report::export::ExportReport, CodecError> {
    let (ir, fidelity) = fixture_native_envelope_context(EncodeInput::new(ir, Some(fidelity)))?;
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
pub(crate) mod nist;
pub(crate) mod parasolid;
pub(crate) mod pmi;
pub(crate) mod readback;
pub(crate) mod surfaces;
pub(crate) mod swift;
pub(crate) mod tessellation;
