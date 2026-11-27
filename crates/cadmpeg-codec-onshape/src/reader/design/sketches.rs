// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{Construction, FrameValue};
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::ops::sketch::{
    EntityDefinition, EntityForm, Planar, PlanarCurve, Sketch, SketchEntity, SketchPlane,
    SketchSolve,
};
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::sketches::SketchGeometryDefinition as Geometry;
use cadmpeg_ir::units::{UnitVector2, UnitVector3};
use cadmpeg_ir::CadIr;

/// Project every sketch of `ir` whose frame and curves the design states,
/// and return the sketch features whose sketch it does not state, with the
/// reason.
pub(in crate::reader) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<BTreeMap<FeatureId, String>, CodecError> {
    let mut failures = BTreeMap::new();
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
        match sketch_records(source, feature, &ir.model.sketch_entities) {
            Ok((sketch, entities)) => {
                ctx.charge_entities(
                    cadmpeg_core::decode::u64_from_index(entities.len()) + 1,
                    "Onshape design sketch",
                )?;
                reserve(&mut ir.design.sketches, 1)?;
                reserve(&mut ir.design.sketch_entities, entities.len())?;
                ir.design.sketches.push(sketch);
                ir.design.sketch_entities.extend(entities);
            }
            Err(reason) => {
                failures.insert(feature.id.clone(), reason);
            }
        }
    }
    Ok(failures)
}

fn reserve<T>(rows: &mut Vec<T>, count: usize) -> Result<(), CodecError> {
    rows.try_reserve(count)
        .map_err(|_| CodecError::malformed("Onshape design allocation failed"))
}

/// The design sketch of `source`, owned by `feature`, and its entities.
fn sketch_records(
    source: &cadmpeg_ir::sketches::Sketch,
    feature: &cadmpeg_ir::features::Feature,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Result<(Sketch, Vec<SketchEntity<Planar>>), String> {
    let (origin, normal, u_axis) = source
        .placement
        .resolved()
        .ok_or("the sketch frame is unresolved")?;
    let axis =
        |value| UnitVector3::normalized_nonzero(value).ok_or("the sketch axis is degenerate");
    let x_axis = axis(u_axis)?;
    let z_axis = axis(normal)?;
    let y_axis = UnitVector3::normalized(z_axis.as_raw().cross(*x_axis.as_raw()))
        .ok_or("the sketch frame is degenerate")?;
    let sketch = Sketch {
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
            inputs: Vec::new(),
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
        native_ref: source.native_ref.clone(),
    };
    let entities = entities
        .iter()
        .filter(|entity| entity.sketch == source.id)
        .map(|entity| {
            Ok(SketchEntity {
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
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((sketch, entities))
}

fn definition(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> Result<EntityDefinition<Planar>, String> {
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
                    .ok_or("the sketch ellipse axis is degenerate")?,
                radii: [radii.major(), radii.minor()],
                range: *bounds,
            }
        }
        Geometry::Nurbs { curve } => PlanarCurve::Nurbs(curve.clone()),
        Geometry::Native { native_kind } => {
            return Ok(EntityDefinition::Defined {
                form: Box::new(EntityForm::Native(
                    NonBlankString::new(format!("onshape:{}", native_kind.as_str()))
                        .ok_or("the sketch native curve kind is blank")?,
                )),
                solved: None,
            });
        }
        _ => return Err("the sketch curve kind has no design form".into()),
    }))
}
