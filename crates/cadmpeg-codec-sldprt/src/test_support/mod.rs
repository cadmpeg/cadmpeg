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

pub(crate) mod appearance;
pub(crate) mod container;
pub(crate) mod history;
pub(crate) mod ir;
pub(crate) mod native;
pub(crate) mod parasolid;
pub(crate) mod pmi;
pub(crate) mod tessellation;
