// SPDX-License-Identifier: Apache-2.0
//! Planar sketch statements from decoded Inventor geometry.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{
    Construction, ConstructionInput, FrameValue, InputReference,
};
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sketch::{
    DefiningRole, EntityDefinition, EntityForm, EntityRole, Planar, PlanarCurve, Sketch,
    SketchEntity, SketchPlane, SketchSolve,
};
use cadmpeg_ir::design::projection::sketch::{curve_ends, planar_curve};
use cadmpeg_ir::design::selection::{Anchor, Picks, Selection};
use cadmpeg_ir::features::FeatureOperation as LegacyOperation;
use cadmpeg_ir::math::{portable::Portable, Point2};
use cadmpeg_ir::sketches::{
    SketchEntity as LegacyEntity, SketchEntityId, SketchGeometryDefinition as Geometry, SketchId,
    SketchPlacement,
};
use cadmpeg_ir::units::{FinitePoint2, UnitVector3};
use cadmpeg_ir::CadIr;

mod constraints;

type EndpointRoles<'a> =
    BTreeMap<&'a SketchId, BTreeMap<&'a str, Vec<(EntityRole<Planar>, FinitePoint2)>>>;

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
    let mut losses = Vec::new();
    let roles = endpoint_roles(&ir.model.sketch_entities);
    let stored_points = unique_index(ir.model.sketch_entities.iter().filter_map(|entity| {
        if !matches!(entity.geometry.definition(), Geometry::Point { .. }) {
            return None;
        }
        Some(((&entity.sketch, entity.native_ref.as_deref()?), entity.id()))
    }));
    let mut taken = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| entity.id().clone())
        .collect::<HashSet<_>>();
    let tolerance = ir.tolerances.linear.get();
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
        let (inputs, resolved) = match source.placement {
            SketchPlacement::Resolved { .. } => {
                let (origin, normal, u_axis) = source
                    .placement
                    .resolved()
                    .ok_or_else(|| CodecError::malformed("Inventor sketch frame"))?;
                let x_axis = UnitVector3::normalized_nonzero(u_axis)
                    .ok_or_else(|| CodecError::malformed("Inventor sketch x axis"))?;
                let z_axis = UnitVector3::normalized_nonzero(normal)
                    .ok_or_else(|| CodecError::malformed("Inventor sketch normal"))?;
                let frame = FrameValue {
                    origin,
                    x_axis,
                    z_axis,
                    y_axis: UnitVector3::normalized(z_axis.as_raw().cross(*x_axis.as_raw()))
                        .ok_or_else(|| CodecError::malformed("Inventor sketch y axis"))?,
                };
                (Vec::new(), Some(frame))
            }
            SketchPlacement::Unresolved {} => (
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
                roles: match (entity.geometry.definition(), entity.native_ref.as_deref()) {
                    (Geometry::Point { position }, Some(native))
                        if stored_points.get(&(&entity.sketch, native))
                            == Some(&Some(entity.id())) =>
                    {
                        roles_at(&roles, (&entity.sketch, native), *position, tolerance)
                    }
                    _ => Vec::new(),
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
        for (&native, candidates) in roles.get(&source.id).into_iter().flatten() {
            if stored_points.contains_key(&(&source.id, native)) {
                continue;
            }
            let Some((id, position)) = candidates.iter().find_map(|(role, position)| {
                let side = if role.role == DefiningRole::Start {
                    "start"
                } else {
                    "end"
                };
                let id = SketchEntityId::mint(format!("{}/{side}-point", role.of)).ok()?;
                (!taken.contains(&id)).then_some((id, *position))
            }) else {
                continue;
            };
            ctx.charge_entities(1, "project Inventor v11 sketch end point")?;
            taken.insert(id.clone());
            ir.design.sketch_entities.push(SketchEntity {
                id,
                sketch: source.id.clone(),
                definition: EntityDefinition::Free(PlanarCurve::Point(position)),
                roles: agreeing_roles(candidates, position, tolerance),
                construction: None,
                centerline: None,
                native: None,
                native_options: BTreeMap::default(),
                source_settings: BTreeMap::default(),
                native_ref: Some(native.to_owned()),
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
        let (Some(ends), [start, end]) =
            (curve_ends(&curve.geometry), curve.endpoint_refs.as_slice())
        else {
            continue;
        };
        for ((reference, role), [u, v]) in [(start, DefiningRole::Start), (end, DefiningRole::End)]
            .into_iter()
            .zip(ends)
        {
            let Some(position) = FinitePoint2::new(Point2::new(u, v)) else {
                continue;
            };
            roles
                .entry(&curve.sketch)
                .or_default()
                .entry(reference.as_str())
                .or_default()
                .push((
                    EntityRole {
                        of: curve.id().clone(),
                        role,
                    },
                    position,
                ));
        }
    }
    roles
}

fn roles_at(
    roles: &EndpointRoles<'_>,
    key: (&SketchId, &str),
    point: FinitePoint2,
    tolerance: f64,
) -> Vec<EntityRole<Planar>> {
    roles
        .get(key.0)
        .and_then(|points| points.get(key.1))
        .map_or_else(Vec::new, |candidates| {
            agreeing_roles(candidates, point, tolerance)
        })
}

fn agreeing_roles(
    candidates: &[(EntityRole<Planar>, FinitePoint2)],
    point: FinitePoint2,
    tolerance: f64,
) -> Vec<EntityRole<Planar>> {
    let mut roles = candidates
        .iter()
        .filter_map(|(role, position)| {
            let distance = (point.u - position.u).portable_hypot(point.v - position.v);
            (distance <= tolerance).then(|| role.clone())
        })
        .collect::<Vec<_>>();
    roles.sort_by(|a, b| a.of.cmp(&b.of));
    roles
}

#[cfg(test)]
mod tests;
