// SPDX-License-Identifier: Apache-2.0
//! Located symbolic-thread construction witness with bounded wire admission.

use crate::om::symbolic_thread::{SymbolicThreadWitness, PARAMETER_CLASS};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ConstructionWire")]
pub(in crate::native) struct SymbolicThreadConstruction {
    field: SymbolicThreadWitness,
    source_offset: u64,
}

#[derive(Deserialize)]
struct ConstructionWire {
    #[serde(flatten)]
    field: SymbolicThreadWitness,
    source_offset: u64,
    byte_len: u64,
}

impl TryFrom<ConstructionWire> for SymbolicThreadConstruction {
    type Error = &'static str;

    fn try_from(wire: ConstructionWire) -> Result<Self, Self::Error> {
        let construction = Self::new(wire.field, wire.source_offset)?;
        if u64_from_index(construction.field.byte_len()) != wire.byte_len {
            return Err("byte_len must describe the exact duplicate tuple and reference frame");
        }
        Ok(construction)
    }
}

impl Serialize for SymbolicThreadConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct ConstructionRef<'a> {
            #[serde(flatten)]
            field: &'a SymbolicThreadWitness,
            source_offset: u64,
            byte_len: u64,
        }
        ConstructionRef {
            field: &self.field,
            source_offset: self.source_offset,
            byte_len: u64_from_index(self.field.byte_len()),
        }
        .serialize(serializer)
    }
}

impl SymbolicThreadConstruction {
    pub(in crate::native) fn new(
        field: SymbolicThreadWitness,
        source_offset: u64,
    ) -> Result<Self, &'static str> {
        source_offset
            .checked_add(u64_from_index(field.byte_len()))
            .ok_or("source_offset: symbolic-thread witness span overflow")?;
        Ok(Self {
            field,
            source_offset,
        })
    }

    pub(in crate::native) fn point_mm(&self) -> [f64; 3] {
        self.field.point_mm()
    }
}

pub(super) fn read_construction(
    ctx: &DecodeContext<'_>,
    section: &crate::om::Section<'_>,
    record: crate::om::operation_record::OperationRecord<'_>,
    frames: &[crate::om::OperationPayloadTextFrame<'_>],
    entry_offset: u64,
) -> Result<Option<SymbolicThreadConstruction>, CodecError> {
    let class = record
        .payload()
        .first()
        .and_then(|selector| usize::from(*selector).checked_sub(1))
        .and_then(|index| section.types.get(index));
    ctx.charge_work(
        u64_from_index(PARAMETER_CLASS.len() + frames.len()),
        "NX symbolic thread owner and mode",
    )?;
    if class.is_none_or(|class| class.name != PARAMETER_CLASS) {
        return Ok(None);
    }
    let mut cuts = frames
        .iter()
        .filter(|frame| matches!(frame.value.as_str(), "CUT" | "Cut"));
    let Some(cut) = cuts.next() else {
        return Ok(None);
    };
    if cuts.next().is_some() {
        return Ok(None);
    }
    let Some((field, offset)) =
        SymbolicThreadWitness::read(ctx, record.payload_view(), cut.offset)?
    else {
        return Ok(None);
    };
    let source_offset = entry_offset
        .checked_add(u64_from_index(offset))
        .ok_or_else(|| ctx.refuse_codec_limit("NX symbolic thread witness offset", 0, 1))?;
    SymbolicThreadConstruction::new(field, source_offset)
        .map(Some)
        .map_err(|_| ctx.refuse_codec_limit("NX symbolic thread witness span", 0, 1))
}

#[cfg(test)]
mod tests;
