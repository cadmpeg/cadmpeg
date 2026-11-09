// SPDX-License-Identifier: Apache-2.0
//! Order emitted entities and remap every typed reference.

use super::{
    attributes, map_parameter_references, reference_marker, BodyPresentation, DirectoryLineFont,
    Entity,
};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn source_order(
    ir: &CadIr,
    entities: &mut Vec<Entity>,
    presentations: &mut BTreeMap<usize, BodyPresentation>,
) -> Result<(), crate::writer::WriteError> {
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
) -> Result<(), crate::writer::WriteError> {
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
                    crate::writer::WriteError::Unwritable(
                        "IGES entity ordering found a dangling parameter pointer".into(),
                    )
                })
        })?;
    }
    super::remap_presentations(presentations, &remap);
    Ok(())
}

/// The native records that the output regenerates from neutral (#209 §2.3
/// W2): each Directory record and each direction record whose entity the
/// output writes.
pub(super) fn regenerated_records(ir: &CadIr, entities: &[Entity]) -> BTreeSet<String> {
    let Some(namespace) = ir.native.namespace("iges") else {
        return BTreeSet::new();
    };
    let targets = attributes::targets(ir, entities);
    let mut regenerated = BTreeSet::new();
    for record in namespace.arenas().get("entities").into_iter().flatten() {
        if targets.contains_key(record.id()) {
            regenerated.insert(record.id().to_owned());
        }
    }
    for record in namespace.arenas().get("directions").into_iter().flatten() {
        let source = record
            .field("source_entity")
            .and_then(|value| value.as_str().map(str::to_owned));
        if targets.contains_key(record.id())
            || source.is_some_and(|source| targets.contains_key(&source))
        {
            regenerated.insert(record.id().to_owned());
        }
    }
    regenerated
}
