// SPDX-License-Identifier: Apache-2.0
//! State-dependent point association tails.

use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

use super::{PmDcSketchEntityKind, PmDcSketchEntityPayload, PointTail, SketchInventory};
use crate::loss::InventorLossCode;
use crate::pmdc::{reference_list, Cursor, PmDcContentHeader, PmDcReference};

#[derive(Deserialize)]
pub(super) struct SketchEntityWire {
    save_version_major: u8,
    header: PmDcContentHeader,
    entity_flags: u32,
    sketch: PmDcReference,
    kind: PmDcSketchEntityKind,
}

impl TryFrom<SketchEntityWire> for PmDcSketchEntityPayload {
    type Error = &'static str;

    fn try_from(wire: SketchEntityWire) -> Result<Self, Self::Error> {
        let entity = Self {
            save_version_major: wire.save_version_major,
            header: wire.header,
            entity_flags: wire.entity_flags,
            sketch: wire.sketch,
            kind: wire.kind,
        };
        if !entity.tail_matches_version() {
            return Err("point tail form does not match save_version_major");
        }
        Ok(entity)
    }
}

impl PmDcSketchEntityPayload {
    pub(crate) fn tail_matches_version(&self) -> bool {
        let PmDcSketchEntityKind::Point { tail, .. } = &self.kind else {
            return true;
        };
        match tail {
            PointTail::Absent => true,
            PointTail::Paired(_) => self.save_version_major >= 25,
            PointTail::Present { state, links, .. } => {
                links.is_some() == (self.save_version_major >= 25)
                    && (*state != 2 || self.save_version_major < 25)
            }
        }
    }
}

pub(crate) fn paired_association_loss(
    inventory: &SketchInventory,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    let count = inventory
        .entities
        .iter()
        .filter(|entity| {
            matches!(
                entity.kind,
                PmDcSketchEntityKind::Point {
                    tail: PointTail::Paired(_),
                    ..
                }
            )
        })
        .count();
    (count != 0).then(|| {
        InventorLossCode::SketchPairedAssociationUnprojected.note(format!(
            "{count} Inventor sketch-point paired association(s) remain native; scalar and operand roles are unresolved."
        ))
    })
}

/// The two scalars and two entity operands of a state-2 point tail, with the
/// metadata words of both pair lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PointPairedAssociation {
    scalar_metadata: [u32; 2],
    scalars: [FiniteReal; 2],
    operand_metadata: [u32; 2],
    operands: [PmDcReference; 2],
}

impl PointPairedAssociation {
    /// The two entity operands, in record order.
    pub(crate) fn operands(&self) -> &[PmDcReference; 2] {
        &self.operands
    }
}

fn pair_list_header(cursor: &mut Cursor<'_>) -> Result<[u32; 2], CodecError> {
    let marker = [
        cursor.u16("point pair marker")?,
        cursor.u16("point pair marker")?,
    ];
    let count = cursor.u32("point pair count")?;
    if marker != [2, 0x3000] || count != 2 {
        return Err(CodecError::malformed(
            "Inventor point paired association requires a type-2 pair list",
        ));
    }
    Ok([
        cursor.u32("point pair metadata")?,
        cursor.u32("point pair metadata")?,
    ])
}

pub(super) fn parse_tail(cursor: &mut Cursor<'_>, version: u8) -> Result<PointTail, CodecError> {
    if cursor.remaining() == 0 {
        return Ok(PointTail::Absent);
    }
    let state = cursor.u32("point tail state")?;
    if state == 2 && version >= 25 {
        let scalar_metadata = pair_list_header(cursor)?;
        let scalars = [
            cursor.f64("point association scalar")?,
            cursor.f64("point association scalar")?,
        ];
        let operand_metadata = pair_list_header(cursor)?;
        let mut operand = || {
            let marker = [
                cursor.u16("point operand list marker")?,
                cursor.u16("point operand list marker")?,
            ];
            let count = cursor.u32("point operand list count")?;
            if marker != [2, 0x3000] || count != 0 {
                return Err(CodecError::malformed(
                    "Inventor point paired association requires empty operand auxiliary lists",
                ));
            }
            cursor.reference("point association operand entity")
        };
        let operands = [operand()?, operand()?];
        return Ok(PointTail::Paired(PointPairedAssociation {
            scalar_metadata,
            scalars,
            operand_metadata,
            operands,
        }));
    }
    let associations = reference_list(cursor, 2, "point association list")?;
    let links = if version >= 25 {
        Some(reference_list(cursor, 2, "point link list")?)
    } else {
        None
    };
    Ok(PointTail::Present {
        state,
        associations,
        links,
    })
}

#[cfg(test)]
mod tests;
