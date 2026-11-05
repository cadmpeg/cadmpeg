// SPDX-License-Identifier: Apache-2.0
//! Order emitted entities and remap every typed reference.

use super::{
    attributes, map_parameter_references, reference_marker, BodyPresentation, DirectoryLineFont,
    Entity,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

pub(super) fn source_order(
    ir: &CadIr,
    entities: &mut Vec<Entity>,
    presentations: &mut BTreeMap<usize, BodyPresentation>,
) -> Result<(), CodecError> {
    let Some(records) = ir
        .native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("entities"))
    else {
        return Ok(());
    };
    let targets = attributes::targets(ir, entities);
    let mut sequences = BTreeMap::new();
    for record in records {
        let Some([index]) = targets.get(record.id()).map(Vec::as_slice) else {
            continue;
        };
        let Some(entity) = entities.get(*index) else {
            continue;
        };
        if record.field("entity_type").and_then(|value| value.as_u64())
            != Some(u64::from(entity.type_code))
            || record.field("form").and_then(|value| value.as_i64()) != Some(entity.form)
        {
            continue;
        }
        let Some(sequence) = record
            .field("directory_sequence")
            .and_then(|value| value.as_u64())
        else {
            continue;
        };
        sequences
            .entry(*index)
            .and_modify(|value: &mut u64| *value = (*value).min(sequence))
            .or_insert(sequence);
    }
    let mut permutation = (0..entities.len()).collect::<Vec<_>>();
    permutation.sort_by_key(|index| {
        (
            !sequences.contains_key(index),
            sequences.get(index).copied(),
            *index,
        )
    });
    reorder(entities, presentations, permutation)
}

pub(super) fn reorder(
    entities: &mut Vec<Entity>,
    presentations: &mut BTreeMap<usize, BodyPresentation>,
    permutation: Vec<usize>,
) -> Result<(), CodecError> {
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
                    CodecError::malformed("IGES entity ordering found a dangling parameter pointer")
                })
        })?;
    }
    super::remap_presentations(presentations, &remap);
    Ok(())
}
