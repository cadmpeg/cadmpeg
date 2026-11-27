// SPDX-License-Identifier: Apache-2.0
//! Planar sketch statements from decoded Inventor geometry.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{Construction, ConstructionInput, InputReference};
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sketch::{
    DefiningRole, EntityDefinition, EntityForm, EntityRole, Planar, Sketch, SketchEntity,
    SketchPlane, SketchSolve,
};
use cadmpeg_ir::design::projection::sketch::{planar_curve, stored_frame};
use cadmpeg_ir::design::selection::{Anchor, Picks, Selection};
use cadmpeg_ir::features::FeatureOperation as LegacyOperation;
use cadmpeg_ir::sketches::{
    SketchEntity as LegacyEntity, SketchGeometryDefinition as Geometry, SketchId,
};
use cadmpeg_ir::CadIr;

mod constraints;

type EndpointRoles<'a> = BTreeMap<(&'a SketchId, &'a str), Vec<EntityRole<Planar>>>;

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
    let mut losses = Vec::new();
    let roles = endpoint_roles(&ir.model.sketch_entities);
    let kinds = ir
        .design
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.kind.clone()))
        .collect();
    for feature in &ir.model.features {
        let (id, block) = match feature.evaluation.definition().operation() {
            LegacyOperation::Sketch { sketch } => (sketch.id(), false),
            LegacyOperation::SketchBlockDefinition { sketch } => (sketch.as_ref(), true),
            _ => continue,
        };
        let Some(source) = id.and_then(|id| ir.model.sketches.iter().find(|item| &item.id == id))
        else {
            continue;
        };
        let Some(target) = ir
            .design
            .features
            .iter_mut()
            .find(|item| item.id == feature.id)
        else {
            continue;
        };
        ctx.charge_entities(1, "project Inventor v11 sketch")?;
        let (inputs, resolved) = match stored_frame(source.placement) {
            Some(frame) => (Vec::new(), Some(frame)),
            None => (
                vec![ConstructionInput {
                    role: None,
                    reference: Some(InputReference::Picks(Selection {
                        at: Anchor::Input,
                        picks: Picks::Unread,
                        native: None,
                    })),
                    value: None,
                    reversed: None,
                }],
                None,
            ),
        };
        ir.design.sketches.push(Sketch {
            id: source.id.clone(),
            owner: feature.id.clone(),
            name: source.name.clone(),
            plane: if block {
                SketchPlane::Block
            } else {
                SketchPlane::Placed(Box::new(Construction {
                    inputs,
                    after: Vec::new(),
                    solution: None,
                    mode: None,
                    resolved,
                    resolved_in: BTreeMap::default(),
                }))
            },
            solve: SketchSolve {
                state: None,
                status: EvaluationStatus::Current,
            },
            native_options: BTreeMap::default(),
            source_settings: BTreeMap::default(),
            native_ref: source.native_ref.clone(),
        });
        target.definition.operation = FeatureOperation::Sketch(source.id.clone());
        for entity in ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == source.id)
        {
            ctx.charge_entities(1, "project Inventor v11 sketch entity")?;
            ir.design.sketch_entities.push(SketchEntity {
                id: entity.id().clone(),
                sketch: source.id.clone(),
                definition: planar_curve(&entity.geometry).map_or_else(
                    || EntityDefinition::Defined {
                        form: Box::new(EntityForm::Native(match entity.geometry.definition() {
                            Geometry::Native { native_kind } => native_kind.clone(),
                            _ => cadmpeg_core::nonblank_literal!("inventor:sketch_entity"),
                        })),
                        solved: None,
                    },
                    EntityDefinition::Free,
                ),
                roles: if matches!(entity.geometry.definition(), Geometry::Point { .. }) {
                    entity
                        .native_ref
                        .as_deref()
                        .and_then(|native| roles.get(&(&entity.sketch, native)))
                        .cloned()
                        .unwrap_or_default()
                } else {
                    Vec::new()
                },
                construction: if entity.construction
                    || matches!(entity.geometry.definition(), Geometry::Line { .. })
                {
                    Some(entity.construction)
                } else {
                    None
                },
                centerline: None,
                native: None,
                native_options: BTreeMap::default(),
                source_settings: BTreeMap::default(),
                native_ref: entity.native_ref.clone(),
            });
        }
        for constraint in ir
            .model
            .sketch_constraints
            .iter()
            .filter(|item| item.sketch == source.id)
        {
            ctx.charge_entities(1, "project Inventor v11 sketch constraint")?;
            losses.extend(constraints::loss(constraint, &kinds));
            ir.design
                .sketch_constraints
                .push(constraints::project(constraint, &kinds)?);
        }
    }
    ir.design.sketches.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketch_entities.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketch_constraints.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(losses)
}

fn endpoint_roles(entities: &[LegacyEntity]) -> EndpointRoles<'_> {
    let mut roles: EndpointRoles<'_> = BTreeMap::new();
    for curve in entities {
        let [start, end] = curve.endpoint_refs.as_slice() else {
            continue;
        };
        for (reference, role) in [(start, DefiningRole::Start), (end, DefiningRole::End)] {
            roles
                .entry((&curve.sketch, reference.as_str()))
                .or_default()
                .push(EntityRole {
                    of: curve.id().clone(),
                    role,
                });
        }
    }
    for roles in roles.values_mut() {
        roles.sort_by(|left, right| left.of.cmp(&right.of));
    }
    roles
}

#[cfg(test)]
mod tests;
