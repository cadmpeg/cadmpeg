// SPDX-License-Identifier: Apache-2.0
//! Spatial sketch membership and model-space bounds.

use cadmpeg_core::{
    decode::{DecodeContext, View},
    CodecError,
};
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

use crate::{
    pmdc::{
        reference_list_with_context, state_content_header, Cursor, PmDcContentHeader,
        PmDcReferenceList,
    },
    record_identity::{push_record, Located, RecordPayload, RecordTypeId},
    record_issue::{RecordIssue, RecordIssueFamily},
    rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind},
};

pub(crate) const TYPE: [u8; 16] = [
    0xda, 0xd8, 0xe9, 0x3a, 0xd4, 0x11, 0x3a, 0x2c, 0xc0, 0x00, 0xad, 0x96, 0x7a, 0x14, 0x68, 0x4f,
];

pub(crate) type Record = Located<Payload>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Payload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) state: i32,
    pub(crate) count_value: u32,
    pub(crate) entities: PmDcReferenceList,
    pub(crate) values: [u32; 2],
    pub(crate) lower: [FiniteReal; 3],
    pub(crate) upper: [FiniteReal; 3],
}

impl RecordPayload for Payload {
    const KIND: &'static str = "spatial-sketch";
}

#[derive(Default)]
pub(crate) struct Inventory {
    pub(crate) records: Vec<Record>,
    pub(crate) issues: Vec<RecordIssue>,
}

pub(crate) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<Inventory, CodecError> {
    let mut inventory = Inventory::default();
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc
            || segment
                .registry
                .is_none_or(|registry| registry.version_major != 31)
        {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in table.records.iter().filter(|record| record.type_id == TYPE) {
            match parse(ctx, record.payload) {
                Ok(payload) => push_record(
                    ctx,
                    &mut inventory.records,
                    payload,
                    TYPE,
                    segment.pair.token.key(),
                    record.ordinal,
                    "admit Inventor spatial sketch record",
                )?,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    ctx.charge_entities(1, "admit Inventor spatial sketch issue")?;
                    inventory.issues.push(RecordIssue {
                        family: RecordIssueFamily::Sketch {
                            type_id: RecordTypeId::from_bytes(TYPE),
                        },
                        segment_token: segment.pair.token.key().clone(),
                        record_ordinal: record.ordinal,
                        detail: crate::issue_detail(error)?,
                    });
                }
            }
        }
    }
    Ok(inventory)
}

fn parse(ctx: &DecodeContext<'_>, source: View<'_>) -> Result<Payload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = state_content_header(&mut cursor, 31)?;
    let state = cursor.u32("spatial sketch state")?.cast_signed();
    let count_value = cursor.u32("spatial sketch count value")?;
    let entities = reference_list_with_context(ctx, &mut cursor, 8, "spatial sketch entities")?;
    let values = cursor.u32_array("spatial sketch values")?;
    let lower = [
        cursor.f64("spatial sketch lower x")?,
        cursor.f64("spatial sketch lower y")?,
        cursor.f64("spatial sketch lower z")?,
    ];
    let upper = [
        cursor.f64("spatial sketch upper x")?,
        cursor.f64("spatial sketch upper y")?,
        cursor.f64("spatial sketch upper z")?,
    ];
    cursor.finish("spatial sketch")?;
    Ok(Payload {
        save_version_major: 31,
        header,
        state,
        count_value,
        entities,
        values,
        lower,
        upper,
    })
}

#[cfg(test)]
mod tests;
