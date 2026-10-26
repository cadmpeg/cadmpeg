// SPDX-License-Identifier: Apache-2.0
//! Ordered plate, bend, and corner children of a flange.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::pmdc::{
    reference_list_with_context, state_content_header, Cursor, PmDcContentHeader, PmDcReferenceList,
};
use crate::record_identity::{Located, RecordPayload};

pub(crate) const TYPE: [u8; 16] = [
    0x08, 0xdc, 0xdd, 0xc3, 0xd3, 0x11, 0xd0, 0x97, 0x60, 0x00, 0xa7, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];

pub(crate) type Flange = Located<FlangePayload>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FlangePayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    state: i32,
    pub(crate) children: PmDcReferenceList,
    value: u32,
}

impl RecordPayload for FlangePayload {
    const KIND: &'static str = "sheet-metal-flange";
}

pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<FlangePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = state_content_header(&mut cursor, version)?;
    let state = cursor.i32("flange state")?;
    let children = reference_list_with_context(ctx, &mut cursor, 2, "flange children")?;
    let value = cursor.u32("flange value")?;
    cursor.finish("sheet-metal flange")?;
    Ok(FlangePayload {
        save_version_major: version,
        header,
        state,
        children,
        value,
    })
}

pub(crate) fn valid<'a>(flange: &Flange, type_at: impl Fn(u32) -> Option<&'a str>) -> bool {
    flange.identity.type_id.as_str() == crate::pmdc::type_id_string(TYPE)
        && type_at(flange.identity.record_ordinal)
            .is_some_and(|actual| actual == crate::pmdc::type_id_string(TYPE))
        && [flange.header.next, flange.header.context]
            .into_iter()
            .all(|reference| {
                reference
                    .record_ordinal()
                    .is_none_or(|ordinal| type_at(ordinal).is_some())
            })
        && flange.children.references().iter().all(|reference| {
            reference.record_ordinal().is_none_or(|ordinal| {
                type_at(ordinal).is_some_and(|actual| {
                    actual == crate::pmdc::type_id_string(super::super::FEATURE_TYPE)
                })
            })
        })
}

#[cfg(test)]
mod tests;
