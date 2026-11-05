// SPDX-License-Identifier: Apache-2.0
//! Keep the Directory order of emitted features consistent with CADIR.

use super::{
    map_parameter_references, reference_marker, BodyPresentation, DirectoryLineFont, Entity,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

pub(super) fn order(
    ir: &CadIr,
    entities: &mut Vec<Entity>,
    presentations: &mut BTreeMap<usize, BodyPresentation>,
) -> Result<(), CodecError> {
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
    if permutation.iter().enumerate().all(|(new, old)| new == *old) {
        return Ok(());
    }
    let remap = permutation
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect::<BTreeMap<_, _>>();
    let mut old = std::mem::take(entities)
        .into_iter()
        .map(Some)
        .collect::<Vec<_>>();
    *entities = permutation
        .into_iter()
        .map(|index| old[index].take().expect("entity permutation is unique"))
        .collect();
    for entity in entities.iter_mut() {
        if let Some(index) = &mut entity.structure {
            *index = remap[index];
        }
        if let Some(index) = &mut entity.transform_link {
            *index = remap[index];
        }
        if let Some(DirectoryLineFont::Definition(index)) = &mut entity.line_font {
            *index = remap[index];
        }
        for links in [&mut entity.property_links, &mut entity.association_links] {
            for index in links {
                *index = remap[index];
            }
        }
        entity.parameter_body = map_parameter_references(&entity.parameter_body, |index| {
            remap
                .get(&index)
                .copied()
                .map(reference_marker)
                .ok_or_else(|| {
                    CodecError::malformed(
                        "IGES feature ordering found a dangling parameter pointer",
                    )
                })
        })?;
    }
    super::remap_presentations(presentations, &remap);
    Ok(())
}
