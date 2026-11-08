// SPDX-License-Identifier: Apache-2.0
//! Box frame and box group members of a planar sketch.
//!
//! A box frame record stores the width, height, corner, and placement of a
//! rectangular box. A box group record lists the corner point, the four box
//! lines, and the frame record. Neither stores a curve. The points and lines
//! that they name are ordinary sketch entity records.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::compact_matrix::CompactMatrix;
use crate::loss::InventorLossCode;
use crate::pmdc::{content_header, Cursor};

/// The record type of a box frame.
const FRAME_TYPE: [u8; 16] = [
    0x08, 0xdb, 0x93, 0xdc, 0xc1, 0x49, 0xf2, 0x6f, 0xf2, 0x29, 0xec, 0xb1, 0x80, 0x4d, 0x97, 0x84,
];
/// The record type of the group of a box frame and its box entities.
const GROUP_TYPE: [u8; 16] = [
    0x5f, 0x33, 0xec, 0x8f, 0x6c, 0x4f, 0xb6, 0xbe, 0x9f, 0xcc, 0x71, 0x96, 0x3a, 0xd1, 0xb9, 0xdf,
];
/// The segment major version whose box member bodies are admitted.
const VERSION: u8 = 19;
/// The count of references that a box group stores.
const GROUP_REFERENCES: usize = 7;

/// Whether `type_id` names a box member record.
pub(crate) fn is_box_member_type(type_id: [u8; 16]) -> bool {
    matches!(type_id, FRAME_TYPE | GROUP_TYPE)
}

/// Admits a box member record whose body matches its grammar to the last byte.
pub(super) fn admit(type_id: [u8; 16], payload: View<'_>, version: u8) -> Result<(), CodecError> {
    if version != VERSION {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc box member has unsupported segment version {version}"
        )));
    }
    let mut cursor = Cursor::new(payload);
    content_header(&mut cursor, version)?;
    match type_id {
        FRAME_TYPE => frame_body(&mut cursor)?,
        GROUP_TYPE => group_body(&mut cursor)?,
        _ => {
            return Err(CodecError::malformed("Inventor PmDc box member type"));
        }
    }
    cursor.finish("box member")
}

/// The body of a box frame: the owning sketch reference, width, height, corner
/// position, a u32 value, a compact placement matrix, and a fixed tail.
fn frame_body(cursor: &mut Cursor<'_>) -> Result<(), CodecError> {
    let sketch = cursor.reference("box frame sketch reference")?;
    if sketch.index() == 0 {
        return Err(CodecError::malformed(
            "Inventor PmDc box frame has a null sketch reference",
        ));
    }
    let width = cursor.f64("box frame width")?.get();
    let height = cursor.f64("box frame height")?.get();
    if width <= 0.0 || height <= 0.0 {
        return Err(CodecError::malformed(
            "Inventor PmDc box frame size is not positive",
        ));
    }
    cursor.f64("box frame corner x")?;
    cursor.f64("box frame corner y")?;
    cursor.u32("box frame value")?;
    let value_mask = cursor.u16("box frame placement value mask")?;
    let zero_mask = cursor.u16("box frame placement zero mask")?;
    CompactMatrix::try_new(value_mask, zero_mask, |_| {
        cursor.f64("box frame placement value")
    })?;
    cursor.u32("box frame tail value")?;
    cursor.u16("box frame tail flags")?;
    cursor.f64("box frame tail scale")?;
    cursor.u8("box frame tail state")?;
    Ok(())
}

/// The body of a box group: a u32 value and seven references.
fn group_body(cursor: &mut Cursor<'_>) -> Result<(), CodecError> {
    cursor.u32("box group value")?;
    for _ in 0..GROUP_REFERENCES {
        cursor.reference("box group reference")?;
    }
    Ok(())
}

/// The loss for box member records that stay native.
pub(crate) fn member_loss(count: usize) -> LossNote {
    InventorLossCode::SketchBoxMemberNative.note(format!(
        "Retained {count} Inventor sketch box member record(s): the record stores no curve, the box points and lines that it names remain ordinary sketch entities, and no box outline enters a sketch profile.",
    ))
}
