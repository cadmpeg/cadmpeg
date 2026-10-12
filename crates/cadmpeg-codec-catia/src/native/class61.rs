// SPDX-License-Identifier: Apache-2.0
//! Complete counted class-0x61 allocation rosters and occurrence bits.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::native::projection::native_allocation_reference_encoding;
use crate::native::CatiaAllocationReferenceEncoding;
use crate::wire::bytes::allocation_reference;
use crate::wire::records::{
    ConsolidatedFamily, ConsolidatedFrameFlag, ConsolidatedRawFrame, ConsolidatedRecord,
};

/// One source-local reference and its occurrence-aligned packed bit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Class61Member {
    pub(super) value: u32,
    pub(super) encoding: CatiaAllocationReferenceEncoding,
    pub(super) orientation_bit: bool,
}

/// A byte-closed counted roster; ownership and final member identities remain separate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Class61Wire")]
pub(crate) struct CatiaConsolidatedClass61Record {
    pub(super) id: String,
    pub(super) source_index: u64,
    pub(super) source_offset: u64,
    pub(super) frame: ConsolidatedRawFrame<u64>,
    pub(super) zero_prefix: bool,
    pub(super) roster_control: u8,
    pub(super) members: Vec<Class61Member>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Class61Wire {
    id: String,
    source_index: u64,
    source_offset: u64,
    frame: ConsolidatedRawFrame<u64>,
    zero_prefix: bool,
    roster_control: u8,
    members: Vec<Class61Member>,
}

struct PackedRoster<'a> {
    count: usize,
    zero_prefix: bool,
    packed: &'a [u8],
}

impl PackedRoster<'_> {
    fn control(&self) -> u8 {
        self.packed[0] & 0x0f
    }

    fn member_bit(&self, index: usize) -> bool {
        let bit = index + 4;
        self.packed[bit / 8] & (1 << (bit % 8)) != 0
    }
}

fn packed_roster(payload: &[u8]) -> Option<PackedRoster<'_>> {
    if payload.len() > usize::from(u8::MAX) {
        return None;
    }
    let count = usize::from(payload.first()?.checked_sub(0x80)?);
    if count == 0 {
        return None;
    }
    let mut at = 1;
    for _ in 0..count {
        allocation_reference(payload, &mut at)?;
    }
    let width = (count + 4).div_ceil(8);
    let tail = payload.get(at..)?;
    let zero_prefix = tail.len() == width + 2 && tail.first() == Some(&0);
    let tail = if zero_prefix { tail.get(1..)? } else { tail };
    let (&terminal, packed) = tail.split_last()?;
    let used_last_bits = (count + 4) % 8;
    if terminal != 0x03
        || packed.len() != width
        || used_last_bits != 0 && packed.last()? >> used_last_bits != 0
    {
        return None;
    }
    Some(PackedRoster {
        count,
        zero_prefix,
        packed,
    })
}

fn valid_payload(record: &CatiaConsolidatedClass61Record) -> bool {
    if record.frame.flag != ConsolidatedFrameFlag::Flag03 {
        return false;
    }
    let Some(roster) = packed_roster(&record.frame.payload) else {
        return false;
    };
    if record.members.len() != roster.count
        || record.zero_prefix != roster.zero_prefix
        || record.roster_control != roster.control()
    {
        return false;
    }
    let mut at = 1;
    record.members.iter().enumerate().all(|(index, member)| {
        allocation_reference(&record.frame.payload, &mut at).is_some_and(|reference| {
            reference.value == member.value
                && native_allocation_reference_encoding(reference.encoding) == member.encoding
                && member.orientation_bit == roster.member_bit(index)
        })
    })
}

impl TryFrom<Class61Wire> for CatiaConsolidatedClass61Record {
    type Error = &'static str;

    fn try_from(wire: Class61Wire) -> Result<Self, Self::Error> {
        let record = Self {
            id: wire.id,
            source_index: wire.source_index,
            source_offset: wire.source_offset,
            frame: wire.frame,
            zero_prefix: wire.zero_prefix,
            roster_control: wire.roster_control,
            members: wire.members,
        };
        if !valid_payload(&record) {
            return Err("class-0x61 members or packed fields disagree with the complete frame");
        }
        Ok(record)
    }
}

pub(super) fn records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedClass61Record>, CodecError> {
    let mut output = Vec::new();
    for record in records.iter().filter(|record| {
        record.family() == ConsolidatedFamily::B
            && record.class() == 0x61
            && record.flag() == ConsolidatedFrameFlag::Flag03
    }) {
        let Some(payload) = record.payload().and_then(|range| bytes.get(range)) else {
            continue;
        };
        let Some(roster) = packed_roster(payload) else {
            continue;
        };
        ctx.charge_work(u64_from_index(roster.count), "catia_class61_reference_work")?;
        let mut members = Vec::new();
        let mut at = 1;
        for index in 0..roster.count {
            let Some(reference) = allocation_reference(payload, &mut at) else {
                return Err(CodecError::malformed(
                    "validated class-0x61 reference disappeared",
                ));
            };
            ctx.push_vec(
                &mut members,
                Class61Member {
                    value: reference.value,
                    encoding: native_allocation_reference_encoding(reference.encoding),
                    orientation_bit: roster.member_bit(index),
                },
                "catia_class61_members",
            )?;
        }
        let frame = ConsolidatedRawFrame::from_record(
            record,
            ctx.copy_slice(payload, "catia_class61_payload")?,
        )?;
        let value = CatiaConsolidatedClass61Record {
            id: ctx.format_retained(
                format_args!("catia:consolidated:class61-record#{}", output.len()),
                "catia_class61_id",
            )?,
            source_index: u64_from_index(record.source_index()),
            source_offset: u64_from_index(record.source_range().start),
            frame: frame.into(),
            zero_prefix: roster.zero_prefix,
            roster_control: roster.control(),
            members,
        };
        ctx.push_vec(&mut output, value, "catia_class61_records")?;
    }
    Ok(output)
}

#[cfg(test)]
pub(super) fn validate(
    records: &[CatiaConsolidatedClass61Record],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, record) in records.iter().enumerate() {
        let source_order_valid = index == 0
            || (
                records[index - 1].source_index,
                records[index - 1].source_offset,
            ) < (record.source_index, record.source_offset);
        if record.id != format!("catia:consolidated:class61-record#{index}")
            || !valid_payload(record)
            || !source_order_valid
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "class-0x61 record `{}` has an invalid counted roster",
                record.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
