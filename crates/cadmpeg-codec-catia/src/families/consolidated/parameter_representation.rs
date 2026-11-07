// SPDX-License-Identifier: Apache-2.0
//! Framed curve representations selected by consolidated parameter relations.

use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::compact_unsigned::CompactUnsigned;
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRecord};

use super::allocation::AllocationJournal;
use super::records::class25::{Class25Operand, Class25ParameterMap};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Class25Representations<'a> {
    pub(crate) target: Option<&'a ConsolidatedRecord>,
    pub(crate) source: Option<&'a ConsolidatedRecord>,
}

pub(crate) fn class25_representations<'a>(
    journal: &AllocationJournal<'a>,
    origin: usize,
    relation: &Class25ParameterMap,
) -> Class25Representations<'a> {
    Class25Representations {
        target: curve_representation(journal, origin, relation.source_operand),
        source: curve_representation(journal, origin, relation.target_operand),
    }
}

fn curve_representation<'a>(
    journal: &AllocationJournal<'a>,
    origin: usize,
    operand: Class25Operand,
) -> Option<&'a ConsolidatedRecord> {
    let target = match operand.encoding {
        AllocationReferenceEncoding::WidthCoded => {
            journal.resolve_weighted_distance(origin, operand.value)
        }
        AllocationReferenceEncoding::BackwardDistance => {
            journal.resolve_record_distance(origin, operand.value)
        }
        AllocationReferenceEncoding::OwnedChild => {
            journal.resolve_owned_child(origin, operand.value)
        }
        AllocationReferenceEncoding::Selector2
        | AllocationReferenceEncoding::TaggedU8
        | AllocationReferenceEncoding::TaggedU16 => None,
    }?;
    matches!(
        (target.family(), target.class()),
        (ConsolidatedFamily::A, 0x20) | (ConsolidatedFamily::B, 0x18 | 0x19 | 0x20)
    )
    .then_some(target)
}

pub(crate) fn b18_support<'a>(
    data: &[u8],
    journal: &AllocationJournal<'a>,
    origin: usize,
) -> Option<&'a ConsolidatedRecord> {
    let record = journal.record(origin)?;
    if record.family() != ConsolidatedFamily::B || record.class() != 0x18 {
        return None;
    }
    let payload = data.get(record.payload()?)?;
    let mut at = 0;
    let distance = CompactUnsigned::read(payload, &mut at)?;
    let target = journal.resolve_weighted_distance(origin, distance.value())?;
    matches!(
        (target.family(), target.class()),
        (ConsolidatedFamily::A, 0x34)
            | (
                ConsolidatedFamily::B,
                0x27 | 0x28 | 0x29 | 0x2a | 0x2b | 0x2d
            )
    )
    .then_some(target)
}

#[cfg(test)]
mod tests;
