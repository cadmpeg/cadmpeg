// SPDX-License-Identifier: Apache-2.0
//! Text annotation members of a planar sketch.
//!
//! A text member record stores the text, the anchor that binds the text to a
//! sketch point, or the group of box points and lines that surround the text.
//! None of the three stores a curve. The points and lines that they name are
//! ordinary sketch entity records.

use cadmpeg_ir::report::loss::LossNote;

use crate::loss::InventorLossCode;

/// The record type of a sketch text.
const TEXT_TYPE: [u8; 16] = [
    0x01, 0x3c, 0xbb, 0x1f, 0xd2, 0x11, 0x4d, 0x68, 0xa0, 0x00, 0x9e, 0x9a, 0x3c, 0x3a, 0xa0, 0x76,
];
/// The record type of the anchor that binds a text to a sketch point.
const ANCHOR_TYPE: [u8; 16] = [
    0x7f, 0x34, 0x10, 0x25, 0xae, 0x4a, 0x79, 0x96, 0x1d, 0x90, 0x47, 0x86, 0x9d, 0x5f, 0x4e, 0x48,
];
/// The record type of the group of box entities of a text.
const GROUP_TYPE: [u8; 16] = [
    0x0e, 0xad, 0x98, 0x7a, 0xb9, 0x49, 0x5c, 0xee, 0xfc, 0x18, 0x67, 0x87, 0xa2, 0xa2, 0x06, 0x43,
];

/// A text member record of a sketch. The record body stays unparsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextMember {
    pub(crate) segment_token: cadmpeg_ir::ids::IdentityKey,
    pub(crate) record_ordinal: u32,
}

/// Whether `type_id` names a sketch text member record.
pub(crate) fn is_text_member_type(type_id: [u8; 16]) -> bool {
    matches!(type_id, TEXT_TYPE | ANCHOR_TYPE | GROUP_TYPE)
}

/// The loss for text member records that stay native.
pub(crate) fn member_loss(record_type: [u8; 16], count: usize) -> LossNote {
    InventorLossCode::SketchTextMemberNative.note(format!(
        "Retained {count} Inventor sketch text member record(s) of type {}: the record stores no curve, the curves that it names remain ordinary sketch entities, and no text outline enters a sketch profile.",
        crate::pmdc::type_id_string(record_type),
    ))
}
