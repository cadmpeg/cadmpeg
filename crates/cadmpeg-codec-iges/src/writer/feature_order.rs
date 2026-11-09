// SPDX-License-Identifier: Apache-2.0
//! Keep the Directory order of emitted features consistent with CADIR.

use super::{BodyPresentation, Entity};
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

pub(super) fn order(
    ir: &CadIr,
    entities: &mut Vec<Entity>,
    presentations: &mut BTreeMap<usize, BodyPresentation>,
) -> Result<(), crate::writer::WriteError> {
    let ordinals = ir
        .model
        .features
        .iter()
        .flat_map(|feature| {
            std::iter::once((feature.id.as_str(), feature.ordinal)).chain(
                feature
                    .native_ref
                    .as_deref()
                    .map(|id| (id, feature.ordinal)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut slots = Vec::new();
    let mut ordered = Vec::new();
    for (index, entity) in entities.iter().enumerate() {
        if let Some(ordinal) = entity
            .source_targets
            .iter()
            .find_map(|id| ordinals.get(id.as_str()))
        {
            slots.push(index);
            ordered.push((index, *ordinal));
        }
    }
    ordered.sort_by_key(|(_, ordinal)| *ordinal);
    let mut permutation = (0..entities.len()).collect::<Vec<_>>();
    for (slot, (old, _)) in slots.into_iter().zip(ordered) {
        permutation[slot] = old;
    }
    super::entity_order::reorder(entities, presentations, permutation)
}
