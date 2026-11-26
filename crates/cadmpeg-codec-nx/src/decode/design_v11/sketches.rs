// SPDX-License-Identifier: Apache-2.0
//! NX sketches (§6.1): each sketch a history feature of its own holds, with
//! its entities. A sketch feature that holds no decoded sketch charges a
//! loss naming it.
//!
//! An entity states its planar curve where it has one, and otherwise its
//! native class in the `nx` namespace with no solved curve; the decoder reads
//! each NX sketch entity as a native point record. The decoder reads no
//! sketch support, so a sketch's plane states the stored frame where the
//! decoder reads one, and otherwise one input with no role and an `Unread`
//! selection of the support (§5.9).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::design::construction::{kinds, Construction, ConstructionInput, InputReference};
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::ops::sketch::{
    EntityDefinition, EntityForm, Planar, Sketch, SketchEntity, SketchPlane, SketchSolve,
};
use cadmpeg_ir::design::projection::sketch::{native_entity_kind, planar_curve, stored_frame};
use cadmpeg_ir::design::selection::{Anchor, Picks, Selection};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureId, FeatureOperation, SketchFeatureBinding,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{Sketch as LegacySketch, SketchEntity as LegacyEntity, SketchId};

use crate::loss::NxLossCode;

/// The sketch binding of `feature`, where it is a sketch feature of the
/// history.
pub(super) fn sketch_of(feature: &LegacyFeature) -> Option<&SketchFeatureBinding> {
    match feature.evaluation.definition().operation() {
        FeatureOperation::Sketch { sketch, .. } => Some(sketch),
        _ => None,
    }
}

/// The loss of sketch feature `feature`, which names `sketch` and does not
/// hold it: its design feature states no sketch.
pub(super) fn unheld(
    feature: &FeatureId,
    sketch: Option<&SketchId>,
    owners: &BTreeMap<SketchId, FeatureId>,
) -> LossNote {
    let message = match sketch.and_then(|sketch| Some((sketch, owners.get(sketch)?))) {
        Some((sketch, holder)) => format!(
            "NX sketch feature {feature} names sketch {sketch}, which the earlier sketch \
             feature {holder} holds, so its design feature states no sketch."
        ),
        None => format!(
            "NX sketch feature {feature} holds no decoded sketch, so its design feature states \
             no sketch."
        ),
    };
    NxLossCode::SketchDesignUnprojected.note(message)
}

/// The history feature that owns each decoded sketch: the first feature, in
/// construction order, that names it.
pub(super) fn owners(ir: &CadIr, history: &[&LegacyFeature]) -> BTreeMap<SketchId, FeatureId> {
    let stored = ir
        .model
        .sketches
        .iter()
        .map(|sketch| &sketch.id)
        .collect::<BTreeSet<_>>();
    let mut owners = BTreeMap::new();
    for feature in history {
        if let Some(sketch) = sketch_of(feature).and_then(SketchFeatureBinding::id) {
            if stored.contains(sketch) {
                owners
                    .entry(sketch.clone())
                    .or_insert_with(|| feature.id.clone());
            }
        }
    }
    owners
}

/// States each decoded sketch that `owners` names, with its entities, and
/// charges a loss naming each sketch, entity and constraint record that has
/// no design record.
pub(super) fn project(
    ir: &mut CadIr,
    owners: &BTreeMap<SketchId, FeatureId>,
    losses: &mut Vec<LossNote>,
) {
    let mut sketches = Vec::new();
    for sketch in &ir.model.sketches {
        match owners.get(&sketch.id) {
            Some(owner) => sketches.push(project_sketch(sketch, owner)),
            None => losses.push(unprojected("sketch", sketch.id.as_str())),
        }
    }
    let mut entities = Vec::new();
    for entity in &ir.model.sketch_entities {
        if owners.contains_key(&entity.sketch) {
            entities.push(SketchEntity::<Planar> {
                id: entity.id().clone(),
                sketch: entity.sketch.clone(),
                definition: definition(entity),
                roles: Vec::new(),
                construction: entity.construction.then_some(true),
                centerline: None,
                native: None,
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
                native_ref: entity.native_ref.clone(),
            });
        } else {
            losses.push(unprojected("sketch entity", entity.id().as_str()));
        }
    }
    for constraint in &ir.model.sketch_constraints {
        losses.push(unprojected("sketch constraint", constraint.id.as_str()));
    }
    let design = &mut ir.design;
    design.sketches.extend(sketches);
    design.sketch_entities.extend(entities);
    design.sketches.sort_by(|a, b| a.id.cmp(&b.id));
    design.sketch_entities.sort_by(|a, b| a.id.cmp(&b.id));
}

fn unprojected(record: &str, id: &str) -> LossNote {
    NxLossCode::SketchDesignUnprojected.note(format!(
        "NX {record} {id} has no design record and is not written."
    ))
}

fn project_sketch(sketch: &LegacySketch, owner: &FeatureId) -> Sketch {
    let resolved = stored_frame(sketch.placement);
    let inputs = if resolved.is_some() {
        Vec::new()
    } else {
        vec![ConstructionInput {
            role: None,
            reference: Some(InputReference::Picks(Selection {
                at: Anchor::Input,
                picks: Picks::Unread,
                native: None,
            })),
            value: None,
            reversed: None,
        }]
    };
    Sketch {
        id: sketch.id.clone(),
        owner: owner.clone(),
        name: sketch.name.clone(),
        plane: SketchPlane::Placed(Box::new(Construction::<kinds::Frame> {
            inputs,
            resolved,
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved_in: BTreeMap::new(),
        })),
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: sketch.native_ref.clone(),
    }
}

/// The definition of `entity`: its planar curve, else its native class in the
/// `nx` namespace with no solved curve.
fn definition(entity: &LegacyEntity) -> EntityDefinition<Planar> {
    planar_curve(&entity.geometry).map_or_else(
        || EntityDefinition::Defined {
            form: Box::new(EntityForm::Native(native_entity_kind(
                "nx",
                &entity.geometry,
            ))),
            solved: None,
        },
        EntityDefinition::Free,
    )
}

#[cfg(test)]
mod tests;
