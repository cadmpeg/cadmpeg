// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{Construction, FrameValue};
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::ops::sketch::{
    EntityDefinition, EntityForm, Planar, PlanarCurve, Sketch, SketchEntity, SketchPlane,
    SketchSolve,
};
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;
use cadmpeg_ir::units::{UnitVector2, UnitVector3};
use cadmpeg_ir::CadIr;

use crate::error::Error;

pub(in crate::reader) fn project(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), Error> {
    for feature in &ir.model.features {
        let FeatureOperation::Sketch { sketch } = feature.evaluation.definition().operation()
        else {
            continue;
        };
        let Some(source) = sketch
            .id()
            .and_then(|id| ir.model.sketches.iter().find(|sketch| &sketch.id == id))
        else {
            continue;
        };
        let (origin, normal, u_axis) = source
            .placement
            .resolved()
            .ok_or_else(|| Error::Decode("thin sketch frame is unresolved".into()))?;
        let axis = |value| {
            UnitVector3::normalized_nonzero(value)
                .ok_or_else(|| Error::Decode("thin sketch axis is degenerate".into()))
        };
        let x_axis = axis(u_axis)?;
        let z_axis = axis(normal)?;
        let y_axis = UnitVector3::normalized(z_axis.as_raw().cross(*x_axis.as_raw()))
            .ok_or_else(|| Error::Decode("thin sketch frame is degenerate".into()))?;
        ctx.charge_entities(1, "Onshape design sketch")?;
        super::super::reserve_history(&mut ir.design.sketches, 1)?;
        ir.design.sketches.push(Sketch {
            id: source.id.clone(),
            owner: feature.id.clone(),
            name: source.name.clone(),
            plane: SketchPlane::Placed(Box::new(Construction {
                resolved: Some(FrameValue {
                    origin,
                    x_axis,
                    y_axis,
                    z_axis,
                }),
                ..Construction::default()
            })),
            solve: SketchSolve {
                state: None,
                status: EvaluationStatus::Current,
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
            native_ref: source.native_ref.clone(),
        });
        for entity in ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == source.id)
        {
            ctx.charge_entities(1, "Onshape design sketch entity")?;
            super::super::reserve_history(&mut ir.design.sketch_entities, 1)?;
            ir.design.sketch_entities.push(SketchEntity {
                id: entity.id().clone(),
                sketch: source.id.clone(),
                definition: definition(&entity.geometry)?,
                roles: Vec::new(),
                construction: Some(entity.construction),
                centerline: None,
                native: None,
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
                native_ref: entity.native_ref.clone(),
            });
        }
    }
    Ok(())
}

fn definition(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> Result<EntityDefinition<Planar>, Error> {
    Ok(EntityDefinition::Free(match geometry.definition() {
        Geometry::Point { position } => PlanarCurve::Point(*position),
        Geometry::Line { start, end } => PlanarCurve::Line {
            start: *start,
            end: *end,
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
        } => {
            let (sin, cos) = major_angle.get().portable_sin_cos();
            PlanarCurve::Ellipse {
                center: *center,
                major: UnitVector2::new([cos, sin])
                    .ok_or_else(|| Error::Decode("thin ellipse axis is degenerate".into()))?,
                radii: [radii.major(), radii.minor()],
                range: *bounds,
            }
        }
        Geometry::Nurbs { curve } => PlanarCurve::Nurbs(curve.clone()),
        Geometry::Native { native_kind } => {
            return Ok(EntityDefinition::Defined {
                form: Box::new(EntityForm::Native(
                    NonBlankString::new(format!("onshape:{}", native_kind.as_str()))
                        .ok_or_else(|| Error::Decode("thin native curve kind is blank".into()))?,
                )),
                solved: None,
            });
        }
        _ => return Err(Error::Decode("thin sketch curve is unresolved".into())),
    }))
}
