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
pub(crate) fn assert_synthesis_fallback(ir: &CadIr, fidelity: &SourceFidelity) {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(matches!(plan.report().synthesis_cause(),
        Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined { reason }) if !reason.is_empty()));
    assert!(!plan.report().losses.is_empty());
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = SldprtCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.bodies.len(), ir.model.bodies.len());
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len());
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
