// SPDX-License-Identifier: Apache-2.0
//! The native records the generated Design1 segment re-emits: every retained
//! record that no dedicated generator authors again (#209 §2.3 W1, W2).

use std::collections::BTreeSet;

use cadmpeg_ir::document::CadIr;

use super::mesh::EncodedMesh;
use super::sketches::SketchStream;
use crate::native::F3dNative;

/// Native sketch records of authored sketches whose neutral entity or
/// constraint the sketch generator does not write: no stream carries them.
pub(super) fn unwritten_sketch_records(target: &CadIr, sketches: &SketchStream) -> BTreeSet<String> {
    let authored: BTreeSet<_> = sketches.sketches.iter().collect();
    let entities: BTreeSet<_> = sketches.entities.iter().collect();
    let constraints: BTreeSet<_> = sketches.constraints.iter().collect();
    target
        .model
        .sketch_entities
        .iter()
        .filter(|entity| authored.contains(&entity.sketch) && !entities.contains(entity.id()))
        .flat_map(|entity| [entity.native_ref.as_deref(), entity.geometry_ref.as_deref()])
        .chain(
            target
                .model
                .sketch_constraints
                .iter()
                .filter(|constraint| {
                    authored.contains(&constraint.sketch) && !constraints.contains(&constraint.id)
                })
                .map(|constraint| constraint.native_ref.as_deref()),
        )
        .flatten()
        .map(str::to_owned)
        .collect()
}

/// The native records for Design1, or `None` when no generator re-authors a
/// retained record. A mesh segment re-emits its own graph. An authored sketch
/// regenerates every native point, curve, text and relation its neutral
/// entities and constraints name; Design1 writing them again would carry
/// them a second time, without the placement that the sketch segment holds.
pub(super) fn design1_native(
    native: &F3dNative,
    target: &CadIr,
    meshes: &[EncodedMesh],
    sketches: Option<&SketchStream>,
) -> Option<F3dNative> {
    let mut owned: BTreeSet<(String, u32)> = meshes
        .iter()
        .filter_map(|mesh| mesh.native_records.as_ref())
        .flat_map(|(segment, indices)| indices.iter().map(|index| (segment.clone(), *index)))
        .collect();
    let authored: BTreeSet<_> = sketches
        .map(|sketches| sketches.sketches.iter().collect())
        .unwrap_or_default();
    let regenerated: BTreeSet<&str> = target
        .model
        .sketch_entities
        .iter()
        .filter(|entity| authored.contains(&&entity.sketch))
        .flat_map(|entity| [entity.native_ref.as_deref(), entity.geometry_ref.as_deref()])
        .chain(
            target
                .model
                .sketch_constraints
                .iter()
                .filter(|constraint| authored.contains(&&constraint.sketch))
                .map(|constraint| constraint.native_ref.as_deref()),
        )
        .flatten()
        .collect();
    let mut claim = |id: &str, record_index: u32| {
        let regenerates = regenerated.contains(id);
        if regenerates {
            if let Some(segment) = crate::ids::design_segment(id) {
                owned.insert((segment.to_owned(), record_index));
            }
        }
        !regenerates
    };
    let mut design = native.clone();
    design
        .sketch_points
        .retain(|record| claim(&record.id, record.record_index));
    design
        .sketch_curve_identities
        .retain(|record| claim(&record.id, record.record_index));
    design
        .sketch_texts
        .retain(|record| claim(&record.id, record.record_index));
    design
        .sketch_relations
        .retain(|record| claim(&record.id, record.record_index));
    if owned.is_empty() {
        return None;
    }
    let owns = |segment: Option<&str>, index: u64| {
        segment.is_some_and(|segment| {
            u32::try_from(index)
                .is_ok_and(|index| owned.contains(&(segment.to_owned(), index)))
        })
    };
    design.design_record_headers.retain(|header| {
        !owns(
            crate::ids::design_segment(&header.id),
            u64::from(header.record_index),
        )
    });
    design.design_entity_headers.retain(|header| {
        let segment = crate::ids::design_segment(&header.id);
        !header
            .member_values()
            .any(|member| owns(segment, u64::from(*member)))
    });
    for kind in &mut design.design_types {
        let segment = crate::ids::design_segment(kind.id()).map(str::to_owned);
        let entities: Vec<u64> = kind
            .entities
            .values()
            .copied()
            .filter(|entity| !owns(segment.as_deref(), *entity))
            .collect();
        if entities.len() != kind.entities.values().count() {
            kind.set_entities(crate::records::identity::ReferenceRun::unlocated(entities));
        }
    }
    Some(design)
}
