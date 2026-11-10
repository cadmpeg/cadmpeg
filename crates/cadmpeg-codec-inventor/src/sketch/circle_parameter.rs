// SPDX-License-Identifier: Apache-2.0
//! Circle parameter members of a planar sketch.
//!
//! A circle parameter record is a planar constraint that binds a numeric
//! parameter to one circular entity. It stores no curve. The circle that it
//! names is an ordinary sketch entity record, and the sketch geometry does not
//! depend on the record.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::report::loss::LossNote;

use super::{
    PmDcSketch, PmDcSketchConstraint, PmDcSketchConstraintKind, PmDcSketchEntity,
    PmDcSketchEntityKind, SketchInventory,
};
use crate::loss::InventorLossCode;

/// The record type of a circle parameter.
pub(super) const TYPE: [u8; 16] = [
    0x6d, 0x37, 0xc7, 0x43, 0xb5, 0x48, 0x53, 0xfb, 0x86, 0xf9, 0x13, 0x92, 0x00, 0x29, 0xb0, 0xd0,
];

/// Whether `constraint` is a circle parameter whose roles hold for the sketch
/// `sketch` that lists it: the maps are empty, the parameter reference is
/// non-null, and the entity reference selects a circular entity of that sketch.
fn roles_hold(
    constraint: &PmDcSketchConstraint,
    sketch: &PmDcSketch,
    entities: &HashMap<(&str, u32), &PmDcSketchEntity>,
) -> bool {
    let PmDcSketchConstraintKind::CircleParameter { entity, .. } = constraint.kind else {
        return false;
    };
    let header = &constraint.header;
    let Some(entity) = entity
        .index()
        .checked_sub(1)
        .and_then(|ordinal| entities.get(&(sketch.identity.segment_token.as_str(), ordinal)))
    else {
        return false;
    };
    header.scalar_map.metadata().is_none()
        && header.scalar_map.entries().is_empty()
        && header.reference_map.metadata().is_none()
        && header.reference_map.entries().is_empty()
        && header
            .parameter
            .is_some_and(|parameter| parameter.index() != 0)
        && matches!(entity.kind, PmDcSketchEntityKind::Circle { .. })
        && sketch
            .identity
            .record_ordinal
            .checked_add(1)
            .is_some_and(|index| entity.sketch.index() == index)
}

/// The keys of the circle parameters that the sketches of `inventory` list
/// once each and whose roles hold.
pub(super) fn admitted_keys(inventory: &SketchInventory) -> HashSet<(&str, u32)> {
    let constraints = inventory
        .constraints
        .iter()
        .filter(|constraint| {
            matches!(
                constraint.kind,
                PmDcSketchConstraintKind::CircleParameter { .. }
            )
        })
        .map(|constraint| {
            (
                (
                    constraint.identity.segment_token.as_str(),
                    constraint.identity.record_ordinal,
                ),
                constraint,
            )
        })
        .collect::<HashMap<_, _>>();
    if constraints.is_empty() {
        return HashSet::new();
    }
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
    let mut listers: HashMap<(&str, u32), usize> = HashMap::new();
    for sketch in &inventory.sketches {
        for ordinal in sketch
            .entities
            .references()
            .iter()
            .filter_map(|reference| reference.record_ordinal())
        {
            *listers
                .entry((sketch.identity.segment_token.as_str(), ordinal))
                .or_insert(0) += 1;
        }
    }
    let mut admitted = HashSet::new();
    for sketch in &inventory.sketches {
        for ordinal in sketch
            .entities
            .references()
            .iter()
            .filter_map(|reference| reference.record_ordinal())
        {
            let key = (sketch.identity.segment_token.as_str(), ordinal);
            if listers.get(&key) == Some(&1)
                && constraints
                    .get(&key)
                    .is_some_and(|constraint| roles_hold(constraint, sketch, &entities))
            {
                admitted.insert(key);
            }
        }
    }
    admitted
}

/// The loss for circle parameter records that stay native.
pub(crate) fn loss(constraints: &[PmDcSketchConstraint]) -> Option<LossNote> {
    let count = constraints
        .iter()
        .filter(|constraint| {
            matches!(
                constraint.kind,
                PmDcSketchConstraintKind::CircleParameter { .. }
            )
        })
        .count();
    (count != 0).then(|| {
        InventorLossCode::SketchCircleParameterNative.note(format!(
            "Retained {count} Inventor circle parameter record(s): the record stores no curve, its parameter role stays unresolved, and the circle that it names remains an ordinary sketch entity."
        ))
    })
}
