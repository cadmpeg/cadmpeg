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
}

/// Plans and writes an inherited write. Returns its bytes and its
/// unwritten-attribute losses.
pub(crate) fn write_with_unwritten_attributes(
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> Result<(Vec<u8>, UnwrittenLosses), CodecError> {
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
pub(crate) mod surfaces;
pub(crate) mod swift;
pub(crate) mod tessellation;
