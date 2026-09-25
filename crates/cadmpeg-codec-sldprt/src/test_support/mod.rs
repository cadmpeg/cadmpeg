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

/// Check public refusal, then exercise the internal history section encoder.
pub(crate) fn serialize_history_after_refusal(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<cadmpeg_core::dialect::DialectId, CodecError> {
    let error = plan_inherited_write(ir, fidelity, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(&error, CodecError::NotImplemented(detail) if detail.starts_with("SLDPRT writer cannot regenerate B-rep after")),
        "{error}"
    );
    let records = crate::source_records(ir, fidelity)?;
    crate::writer::write_semantic_with_records(ir, &fidelity.annotations, &records, writer)
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

pub(crate) mod appearance;
pub(crate) mod container;
pub(crate) mod history;
pub(crate) mod ir;
pub(crate) mod native;
pub(crate) mod parasolid;
pub(crate) mod pmi;
pub(crate) mod tessellation;
