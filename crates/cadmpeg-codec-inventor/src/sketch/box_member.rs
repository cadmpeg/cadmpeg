// SPDX-License-Identifier: Apache-2.0
//! Box frame and box group members of a planar sketch.
//!
//! A box frame record stores the width, height, corner, and placement of a
//! rectangular box. A box group record lists the corner point, the four box
//! lines, and the frame record. Neither stores a curve. The points and lines
//! that they name are ordinary sketch entity records.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use super::text::TextMember;
use super::{PmDcSketchEntity, PmDcSketchEntityKind, SketchInventory};
use crate::compact_matrix::CompactMatrix;
use crate::loss::InventorLossCode;
use crate::pmdc::{content_header, Cursor, PmDcReference};

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

/// Entity-flag bit that a box line carries.
const BOX_LINE_FLAG: u32 = 0x8_0000;

/// The references of an admitted box member record that carry a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BoxMember {
    /// A box frame and its sketch reference.
    Frame { sketch: PmDcReference },
    /// A box group and its seven references.
    Group {
        references: [PmDcReference; GROUP_REFERENCES],
    },
}

/// A role of a box member reference that the records do not satisfy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BoxRole {
    /// The frame sketch reference names the sketch that lists the frame.
    FrameSketch,
    /// Group references 1 and 7 name one point of the sketch.
    GroupCorner,
    /// Group reference 6 names an admitted box frame of the segment.
    GroupFrame,
    /// Group references 2 to 5 name lines of the sketch with the box-line bit.
    GroupLines,
}

impl BoxRole {
    /// The role assumption, as the loss text states it.
    fn assumption(self) -> &'static str {
        match self {
            Self::FrameSketch => {
                "the sketch reference of a box frame names the sketch that lists the frame"
            }
            Self::GroupCorner => {
                "references 1 and 7 of a box group name one point of the sketch that lists the group"
            }
            Self::GroupFrame => "reference 6 of a box group names an admitted box frame",
            Self::GroupLines => {
                "references 2 to 5 of a box group name lines of the sketch with entity-flag bit 0x80000"
            }
        }
    }
}

/// Whether `type_id` names a box member record.
pub(crate) fn is_box_member_type(type_id: [u8; 16]) -> bool {
    matches!(type_id, FRAME_TYPE | GROUP_TYPE)
}

/// Admits a box member record whose body matches its grammar to the last byte.
pub(super) fn admit(
    type_id: [u8; 16],
    payload: View<'_>,
    version: u8,
) -> Result<BoxMember, CodecError> {
    if version != VERSION {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc box member has unsupported segment version {version}"
        )));
    }
    let mut cursor = Cursor::new(payload);
    content_header(&mut cursor, version)?;
    let member = match type_id {
        FRAME_TYPE => frame_body(&mut cursor)?,
        GROUP_TYPE => group_body(&mut cursor)?,
        _ => {
            return Err(CodecError::malformed("Inventor PmDc box member type"));
        }
    };
    cursor.finish("box member")?;
    Ok(member)
}

/// The body of a box frame: the owning sketch reference, width, height, corner
/// position, a u32 value, a compact placement matrix, and a fixed tail.
fn frame_body(cursor: &mut Cursor<'_>) -> Result<BoxMember, CodecError> {
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
    Ok(BoxMember::Frame { sketch })
}

/// The body of a box group: a u32 value and seven references.
fn group_body(cursor: &mut Cursor<'_>) -> Result<BoxMember, CodecError> {
    cursor.u32("box group value")?;
    let mut references = Vec::with_capacity(GROUP_REFERENCES);
    for _ in 0..GROUP_REFERENCES {
        references.push(cursor.reference("box group reference")?);
    }
    let references = references
        .try_into()
        .map_err(|_| CodecError::malformed("Inventor PmDc box group reference count"))?;
    Ok(BoxMember::Group { references })
}

/// Checks the role of every reference of the admitted box members against the
/// sketches and entities of the inventory and records each mismatch on the
/// member. A mismatch does not change admission.
pub(super) fn check_roles(inventory: &mut SketchInventory) {
    let failures = {
        let entities = inventory
            .entities
            .iter()
            .map(|entity| {
                (
                    (
                        entity.identity.segment_token.as_str(),
                        entity.identity.record_ordinal,
                    ),
                    entity,
                )
            })
            .collect::<HashMap<_, _>>();
        let mut listing: HashMap<(&str, u32), Vec<u32>> = HashMap::new();
        for sketch in &inventory.sketches {
            for reference in sketch.entities.references() {
                if let Some(ordinal) = reference.record_ordinal() {
                    listing
                        .entry((sketch.identity.segment_token.as_str(), ordinal))
                        .or_default()
                        .push(sketch.identity.record_ordinal);
                }
            }
        }
        let frames = inventory
            .text_members
            .iter()
            .filter(|member| matches!(member.box_member, Some(BoxMember::Frame { .. })))
            .map(|member| (member.segment_token.as_str(), member.record_ordinal))
            .collect::<HashSet<_>>();
        inventory
            .text_members
            .iter()
            .map(|member| {
                let token = member.segment_token.as_str();
                let key = (token, member.record_ordinal);
                let listed_by = listing.get(&key).map_or(&[][..], Vec::as_slice);
                let mut failed = Vec::new();
                match &member.box_member {
                    Some(BoxMember::Frame { sketch }) => {
                        let owner = sketch.record_ordinal();
                        if listed_by.iter().any(|lister| Some(*lister) != owner) {
                            failed.push(BoxRole::FrameSketch);
                        }
                    }
                    Some(BoxMember::Group { references }) => {
                        let at = |reference: &PmDcReference| {
                            reference
                                .record_ordinal()
                                .and_then(|ordinal| entities.get(&(token, ordinal)).copied())
                        };
                        let corner = at(&references[0]);
                        let sketch = listed_by
                            .first()
                            .map(|lister| lister + 1)
                            .or_else(|| corner.map(|entity| entity.sketch.index()))
                            .or_else(|| at(&references[1]).map(|entity| entity.sketch.index()));
                        let same_sketch =
                            |entity: &PmDcSketchEntity| Some(entity.sketch.index()) == sketch;
                        let corner_ok = references[0].record_ordinal()
                            == references[6].record_ordinal()
                            && corner.is_some_and(|entity| {
                                matches!(entity.kind, PmDcSketchEntityKind::Point { .. })
                                    && same_sketch(entity)
                            });
                        if !corner_ok {
                            failed.push(BoxRole::GroupCorner);
                        }
                        let frame_ok = references[5]
                            .record_ordinal()
                            .is_some_and(|ordinal| frames.contains(&(token, ordinal)));
                        if !frame_ok {
                            failed.push(BoxRole::GroupFrame);
                        }
                        let lines_ok = references[1..5].iter().all(|reference| {
                            at(reference).is_some_and(|entity| {
                                matches!(entity.kind, PmDcSketchEntityKind::Line { .. })
                                    && entity.entity_flags & BOX_LINE_FLAG != 0
                                    && same_sketch(entity)
                            })
                        });
                        if !lines_ok {
                            failed.push(BoxRole::GroupLines);
                        }
                    }
                    None => {}
                }
                failed
            })
            .collect::<Vec<_>>()
    };
    for (member, failed) in inventory.text_members.iter_mut().zip(failures) {
        member.role_failures = failed;
    }
}

/// The losses for box member records whose reference roles do not hold. One
/// loss names each failed role assumption.
pub(crate) fn role_losses(members: &[TextMember]) -> Vec<LossNote> {
    let mut counts = BTreeMap::new();
    for role in members.iter().flat_map(|member| &member.role_failures) {
        *counts.entry(*role).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .map(|(role, count)| {
            InventorLossCode::SketchBoxRoleMismatch.note(format!(
                "Retained {count} Inventor sketch box member record(s) that break a role assumption: {}. The records stay native and the entities that they name remain ordinary sketch entities.",
                role.assumption(),
            ))
        })
        .collect()
}

/// The loss for box member records that stay native.
pub(crate) fn member_loss(count: usize) -> LossNote {
    InventorLossCode::SketchBoxMemberNative.note(format!(
        "Retained {count} Inventor sketch box member record(s): the record stores no curve, the box points and lines that it names remain ordinary sketch entities, and no box outline enters a sketch profile.",
    ))
}
