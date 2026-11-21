// SPDX-License-Identifier: Apache-2.0
//! Planar sketch statements from decoded Inventor geometry.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
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
use cadmpeg_ir::design::selection::{Anchor, Picks, Selection};
use cadmpeg_ir::features::FeatureOperation as LegacyOperation;
use cadmpeg_ir::sketches::{SketchGeometryDefinition as Geometry, SketchPlacement};
use cadmpeg_ir::units::{UnitVector2, UnitVector3};
use cadmpeg_ir::CadIr;

mod constraints;

pub(super) fn project(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
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
                definition: curve(&entity.geometry).map_or_else(
                    || EntityDefinition::Defined {
                        form: Box::new(EntityForm::Native(cadmpeg_core::nonblank_literal!(
                            "inventor:sketch_entity"
                        ))),
                        solved: None,
                    },
                    EntityDefinition::Free,
                ),
                roles: endpoint_roles(entity, &ir.model.sketch_entities),
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
            ir.design
                .sketch_constraints
                .push(constraints::project(constraint));
        }
    }
    ir.design.sketches.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketch_entities.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketch_constraints.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

fn endpoint_roles(
    point: &cadmpeg_ir::sketches::SketchEntity,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Vec<EntityRole<Planar>> {
    let Some(native) = point.native_ref.as_ref() else {
        return Vec::new();
    };
    if !matches!(point.geometry.definition(), Geometry::Point { .. }) {
        return Vec::new();
    }
    let mut roles = Vec::new();
    for curve in entities.iter().filter(|curve| curve.sketch == point.sketch) {
        let [start, end] = curve.endpoint_refs.as_slice() else {
            continue;
        };
        for (reference, role) in [(start, DefiningRole::Start), (end, DefiningRole::End)] {
            if reference == native {
                roles.push(EntityRole {
                    of: curve.id().clone(),
                    role,
                });
            }
        }
    }
    roles
}

fn curve(geometry: &cadmpeg_ir::sketches::SketchGeometry) -> Option<PlanarCurve> {
    Some(match geometry.definition() {
        Geometry::Point { position } => PlanarCurve::Point(*position),
        Geometry::Line { start, end } => PlanarCurve::Line {
            start: *start,
            end: *end,
        },
        Geometry::ReferenceLine { origin, direction } => PlanarCurve::InfiniteLine {
            through: *origin,
            direction: normalized_direction([direction.get().u, direction.get().v])?,
        },
        Geometry::Circle { center, radius } => PlanarCurve::Circle {
            center: *center,
            radius: *radius,
        },
        Geometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => PlanarCurve::Arc {
            center: *center,
            radius: *radius,
            start: *start_angle,
            end: *end_angle,
        },
        Geometry::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => PlanarCurve::Ellipse {
            center: *center,
            major: direction(*major_angle)?,
            radii: [radii.major(), radii.minor()],
            range: *bounds,
        },
        Geometry::Nurbs { curve } => PlanarCurve::Nurbs(curve.clone()),
        _ => return None,
    })
}

fn normalized_direction(value: [f64; 2]) -> Option<UnitVector2> {
    use cadmpeg_ir::math::portable::Portable;
    let length = value[0].portable_hypot(value[1]);
    UnitVector2::new(value.map(|component| component / length))
}

fn direction(angle: cadmpeg_ir::scalar::Angle) -> Option<UnitVector2> {
    use cadmpeg_ir::math::portable::Portable;
    let (sin, cos) = angle.get().portable_sin_cos();
    UnitVector2::new([cos, sin])
}

#[cfg(test)]
mod tests;
