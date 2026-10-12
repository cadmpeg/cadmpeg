// SPDX-License-Identifier: Apache-2.0
//! Complete class-0x60 counted allocation-reference vectors.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use super::CatiaAllocationReferenceEncoding;
use crate::native::projection::native_allocation_reference_encoding;
use crate::wire::bytes::allocation_reference;
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRawFrame, ConsolidatedRecord};

/// One source-local allocation reference with its addressing form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Class60Reference {
    pub(super) value: u32,
    pub(super) encoding: CatiaAllocationReferenceEncoding,
}

/// A complete counted reference list; the frame preserves each token's exact width.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Class60Wire")]
pub(crate) struct CatiaConsolidatedClass60Record {
    pub(super) id: String,
    pub(super) source_index: u64,
    pub(super) source_offset: u64,
    pub(super) frame: ConsolidatedRawFrame<u64>,
    pub(super) references: Vec<Class60Reference>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Class60Wire {
    id: String,
    source_index: u64,
    source_offset: u64,
    frame: ConsolidatedRawFrame<u64>,
    references: Vec<Class60Reference>,
}

impl TryFrom<Class60Wire> for CatiaConsolidatedClass60Record {
    type Error = &'static str;
    fn try_from(wire: Class60Wire) -> Result<Self, Self::Error> {
        if !valid_payload(&wire.frame.payload, &wire.references) {
            return Err("class-0x60 reference vector does not match its complete frame");
        }
        Ok(Self {
            id: wire.id,
            source_index: wire.source_index,
            source_offset: wire.source_offset,
            frame: wire.frame,
            references: wire.references,
        })
    }
}

fn valid_payload(payload: &[u8], references: &[Class60Reference]) -> bool {
    if payload.len() > usize::from(u8::MAX) {
        return false;
    }
    let Some(count) = payload.first().and_then(|lead| lead.checked_sub(0x80)) else {
        return false;
    };
    if usize::from(count) != references.len() {
        return false;
    }
    let mut at = 1;
    for expected in references {
        let Some(reference) = allocation_reference(payload, &mut at) else {
            return false;
        };
        if reference.value != expected.value
            || native_allocation_reference_encoding(reference.encoding) != expected.encoding
        {
            return false;
        }
    }
    at == payload.len()
}

pub(super) fn records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedClass60Record>, CodecError> {
    let mut output = Vec::new();
    for record in records
        .iter()
        .filter(|record| record.family() == ConsolidatedFamily::B && record.class() == 0x60)
    {
        let Some(range) = record.payload() else {
            continue;
        };
        let Some(payload) = bytes.get(range) else {
            continue;
        };
        let Some(count) = payload.first().and_then(|lead| lead.checked_sub(0x80)) else {
            continue;
        };
        let mut references = Vec::new();
        let mut at = 1;
        for _ in 0..count {
            let Some(reference) = allocation_reference(payload, &mut at) else {
                break;
            };
            ctx.push_vec(
                &mut references,
                Class60Reference {
                    value: reference.value,
                    encoding: native_allocation_reference_encoding(reference.encoding),
                },
                "catia_class60_references",
            )?;
        }
        if !valid_payload(payload, &references) {
            continue;
        }
        let frame = ConsolidatedRawFrame::from_record(
            record,
            ctx.copy_slice(payload, "catia_class60_payload")?,
        )?;
        let value = CatiaConsolidatedClass60Record {
            id: ctx.format_retained(
                format_args!("catia:consolidated:class60-record#{}", output.len()),
                "catia_class60_id",
            )?,
            source_index: u64_from_index(record.source_index()),
            source_offset: u64_from_index(record.source_range().start),
            frame: frame.into(),
            references,
        };
        ctx.push_vec(&mut output, value, "catia_class60_records")?;
    }
    Ok(output)
}

#[cfg(test)]
pub(super) fn validate(
    records: &[CatiaConsolidatedClass60Record],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, record) in records.iter().enumerate() {
        let source_order_valid = index == 0
            || (
                records[index - 1].source_index,
                records[index - 1].source_offset,
            ) < (record.source_index, record.source_offset);
        if record.id != format!("catia:consolidated:class60-record#{index}")
            || !valid_payload(&record.frame.payload, &record.references)
            || !source_order_valid
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "class-0x60 record `{}` has an invalid counted reference vector",
                record.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
