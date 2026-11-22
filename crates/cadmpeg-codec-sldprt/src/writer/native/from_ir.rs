// SPDX-License-Identifier: Apache-2.0
//! The user features of a generated part as the native tree writes them,
//! from the CADIR model and the history summary's feature records.

use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch as IrSketch, SketchEntityId, SketchGeometryDefinition, SketchLocus,
};

use super::extrude::Extrusion;
use super::sketch::{Curve, Dimension, Operand, Point, Relation, Sketch};
use super::tree::Plane;
use super::Feature;
use crate::records::SketchRelationKind;
use crate::resolved_features::write_generate::{
    generated_dimension, generated_marker_relations, GeneratedDimension, GeneratedMarkerRelation,
};
use crate::resolved_features::write_prepare::{
    generated_profile_stream, generated_sketch_owner_id, generated_sketch_owner_record,
    unique_planar_sketch_owner, GENERATED_NAME,
};

/// Largest departure of a sketch normal from a default plane's normal, as
/// one minus the absolute cosine.
const PLANE_NORMAL_TOLERANCE: f64 = 1.0e-9;

/// Largest distance, in millimetres, of a sketch origin from a default plane.
const PLANE_OFFSET_TOLERANCE_MM: f64 = 1.0e-6;

/// Largest distance, in metres, between two loci written as one point record.
const POINT_MERGE_TOLERANCE_M: f64 = 1.0e-9;

/// Millimetres to metres.
const METRES_PER_MILLIMETRE: f64 = 0.001;

/// The user features of a generated part, or why the tree writes none of
/// them.
#[derive(Debug)]
pub(in crate::writer) enum Features {
    /// Every sketch and, in a part whose every feature the tree writes, every
    /// extrusion, in source order.
    Native(Vec<Feature>),
    /// The sketches stay in the generated lane; the text names the first
    /// sketch the tree cannot write and why.
    Lane(String),
}

/// The user features of `ir`: every planar sketch of the first configuration
/// as a native sketch, when each lies on a default plane and holds only
/// points, lines, arcs and circles that are not construction geometry, and
/// the part holds no spatial sketch; and every extrusion, when the part's one
/// body comes from its sketches and one-sided blind extrusions alone.
pub(in crate::writer) fn features(
    ir: &cadmpeg_ir::CadIr,
    native: Option<&crate::native::SldprtNative>,
) -> Result<Features, CodecError> {
    if ir.model.sketches.is_empty() {
        return Ok(Features::Native(Vec::new()));
    }
    if let Some(spatial) = ir.model.spatial_sketches.first() {
        return Ok(Features::Lane(format!(
            "spatial sketch {} has no native sketch form here",
            spatial.id.as_str()
        )));
    }
    let Some(native) = native else {
        return Ok(Features::Lane("the write holds no history summary".into()));
    };
    let mut out = Vec::new();
    let mut sketch_sides = Vec::new();
    for sketch in &ir.model.sketches {
        if sketch
            .configuration
            .as_deref()
            .is_some_and(|key| key != "0")
        {
            return Ok(Features::Lane(format!(
                "sketch {} belongs to configuration {}",
                sketch.id.as_str(),
                sketch.configuration.as_deref().unwrap_or_default()
            )));
        }
        let owner = unique_planar_sketch_owner(ir, &sketch.id)?;
        let record = generated_sketch_owner_record(native, owner, sketch.id.as_str())?;
        let feature_id = generated_sketch_owner_id(record, sketch.id.as_str())?;
        let name = if record.name.is_empty() {
            sketch.name.as_deref().unwrap_or(sketch.id.as_str())
        } else {
            record.name.as_str()
        };
        let name = GENERATED_NAME.fit(name).text.to_owned();
        let geometry =
            geometry(ir, sketch).and_then(|(plane, points, curves, handles, flipped)| {
                let (relations, dimensions) = constraints(ir, sketch, &handles)?;
                Ok((plane, points, curves, relations, dimensions, flipped))
            });
        let (plane, points, curves, relations, dimensions, flipped) = match geometry {
            Ok(geometry) => geometry,
            Err(reason) => {
                return Ok(Features::Lane(format!(
                    "sketch {}: {reason}",
                    sketch.id.as_str()
                )))
            }
        };
        sketch_sides.push((sketch.id.clone(), flipped));
        out.push((
            owner.ordinal,
            sketch.id.clone(),
            Feature::Sketch(Sketch {
                name,
                feature_id,
                plane,
                points,
                curves,
                relations,
                dimensions,
                body: generated_profile_stream(ir, sketch)?,
            }),
        ));
    }
    if let Some(extrusions) = extrusions(ir, native, &sketch_sides) {
        out.extend(extrusions);
    }
    out.sort_by_key(|(ordinal, ..)| *ordinal);
    // An extrusion's profile is the position, among the features, of the
    // sketch it extrudes.
    let sketches = out
        .iter()
        .map(|(_, key, feature)| matches!(feature, Feature::Sketch(_)).then_some(key))
        .collect::<Vec<_>>();
    let mut features = Vec::new();
    for (_, key, mut feature) in out.iter().cloned() {
        if let Feature::Extrusion(extrusion) = &mut feature {
            let Some(position) = sketches.iter().position(|sketch| *sketch == Some(&key)) else {
                return Ok(Features::Lane(format!(
                    "extrusion {} extrudes no written sketch",
                    extrusion.name
                )));
            };
            extrusion.profile = position;
        }
        features.push(feature);
    }
    Ok(Features::Native(features))
}

/// A user feature with its ordinal and the sketch it is or extrudes.
type Ordered = (u64, cadmpeg_ir::sketches::SketchId, Feature);

/// The one-sided blind extrusions of `ir`, each with its ordinal and the
/// sketch it extrudes, when the part's one body comes from its written
/// sketches and such extrusions alone: the first extrusion makes the body,
/// later ones join to it or cut it, each extrudes every profile of one
/// written sketch from the sketch plane, without draft.
fn extrusions(
    ir: &cadmpeg_ir::CadIr,
    native: &crate::native::SldprtNative,
    sketches: &[(cadmpeg_ir::sketches::SketchId, bool)],
) -> Option<Vec<Ordered>> {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeStart, FeatureDefinition,
        FeatureOperation, LinearTermination, PlanarProfileRef, ProfileRef,
    };
    if ir.geometry.bodies.len() != 1 {
        return None;
    }
    let mut out = Vec::new();
    for feature in &ir.model.features {
        let FeatureDefinition::Operation(operation) = feature.evaluation.definition() else {
            return None;
        };
        let (profile, direction, start, extent, op, solid) = match operation {
            FeatureOperation::Sketch { .. } => continue,
            FeatureOperation::Extrude {
                profile,
                direction,
                start,
                extent,
                op,
                solid,
                ..
            } => (profile, direction, start, extent, op, solid),
            _ => return None,
        };
        let sketch = match profile {
            ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)) => sketch,
            ProfileRef::Planar(PlanarProfileRef::SketchProfiles { sketch, profiles }) => {
                let count = ir
                    .model
                    .sketches
                    .iter()
                    .find(|candidate| candidate.id == *sketch)
                    .map_or(0, |candidate| candidate.profiles.len());
                let every = (0..count)
                    .all(|index| u32::try_from(index).is_ok_and(|index| profiles.contains(&index)));
                if count == 0 || !every || profiles.len() != count {
                    return None;
                }
                sketch
            }
            _ => return None,
        };
        let &(_, flipped) = sketches.iter().find(|(candidate, _)| candidate == sketch)?;
        let ExtrudeExtent::OneSided { side } = extent else {
            return None;
        };
        let LinearTermination::Blind { length } = side.termination else {
            return None;
        };
        let against = match direction {
            ExtrudeDirection::ProfileNormal {} => false,
            ExtrudeDirection::ReversedProfileNormal {} => true,
            _ => return None,
        };
        let first = out.is_empty();
        let cut = match op {
            BooleanOp::NewBody if first => false,
            BooleanOp::Join if !first => false,
            BooleanOp::Cut if !first => true,
            _ => return None,
        };
        if side.draft.is_some()
            || !matches!(start, ExtrudeStart::ProfilePlane {})
            || *solid == Some(false)
        {
            return None;
        }
        let record_id = feature.native_ref.clone().unwrap_or_else(|| {
            crate::history::write::features::generated_feature_record_id(&feature.id)
        });
        let record = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|record| record.id == record_id)?;
        let feature_id = record.source_value()?;
        let name = if record.name.is_empty() {
            feature.name.as_deref().unwrap_or(feature.id.as_str())
        } else {
            record.name.as_str()
        };
        let depth = length.get();
        out.push((
            feature.ordinal,
            sketch.clone(),
            Feature::Extrusion(Extrusion {
                name: GENERATED_NAME.fit(name).text.to_owned(),
                feature_id,
                profile: 0,
                cut,
                depth: depth.abs() * METRES_PER_MILLIMETRE,
                reversed: against ^ flipped ^ (depth < 0.0),
            }),
        ));
    }
    (!out.is_empty()).then_some(out)
}

/// The operand handle of each entity and of each locus that is a point.
#[derive(Debug, Default)]
struct Handles {
    entities: Vec<(SketchEntityId, Operand)>,
    loci: Vec<(SketchLocus, usize)>,
}

impl Handles {
    fn entity(&self, entity: &SketchEntityId) -> Result<Operand, String> {
        self.entities
            .iter()
            .find(|(candidate, _)| candidate == entity)
            .map(|(_, operand)| *operand)
            .ok_or_else(|| format!("relation operand {} is no written entity", entity.as_str()))
    }

    fn locus(&self, locus: &SketchLocus) -> Result<Operand, String> {
        if let SketchLocus::Entity(entity) = locus {
            return self.entity(entity);
        }
        self.loci
            .iter()
            .find(|(candidate, _)| candidate == locus)
            .map(|(_, point)| Operand::Point(*point))
            .ok_or_else(|| format!("relation locus {locus:?} is no written point"))
    }
}

/// A sketch's plane, records, handles, and whether its normal opposes its
/// plane's.
type Geometry = (Plane, Vec<Point>, Vec<Curve>, Handles, bool);

/// The sketch's plane, its point and curve records and the operand handles
/// of its entities, or why it has none.
fn geometry(ir: &cadmpeg_ir::CadIr, sketch: &IrSketch) -> Result<Geometry, String> {
    let (origin, normal, u_axis) = sketch
        .resolved_placement()
        .map(|(origin, normal, u_axis)| (origin.get(), unit(normal.get()), unit(u_axis.get())))
        .ok_or("the sketch has no resolved placement")?;
    let v_axis = normal.cross(u_axis);
    let (plane, flipped) = [Plane::Front, Plane::Top, Plane::Right]
        .into_iter()
        .find_map(|plane| {
            let cosine = dot(normal, plane.axes()[2]);
            (1.0 - cosine.abs() <= PLANE_NORMAL_TOLERANCE).then_some((plane, cosine < 0.0))
        })
        .ok_or("the sketch plane is no default plane")?;
    let [plane_u, plane_v, plane_n] = plane.axes();
    if dot_point(origin, plane_n).abs() > PLANE_OFFSET_TOLERANCE_MM {
        return Err("the sketch plane is offset from its default plane".into());
    }
    let to_plane = |point: Point2| -> [f64; 2] {
        let model = Point3::new(
            origin.x + point.u * u_axis.x + point.v * v_axis.x,
            origin.y + point.u * u_axis.y + point.v * v_axis.y,
            origin.z + point.u * u_axis.z + point.v * v_axis.z,
        );
        [
            dot_point(model, plane_u) * METRES_PER_MILLIMETRE,
            dot_point(model, plane_v) * METRES_PER_MILLIMETRE,
        ]
    };
    let mut points = Vec::<Point>::new();
    let mut point = |at: [f64; 2], kind: u32| -> usize {
        if let Some(index) = points.iter().position(|candidate| {
            (candidate.at[0] - at[0]).abs() <= POINT_MERGE_TOLERANCE_M
                && (candidate.at[1] - at[1]).abs() <= POINT_MERGE_TOLERANCE_M
        }) {
            return index;
        }
        points.push(Point { at, kind });
        points.len() - 1
    };
    let mut lines = Vec::new();
    let mut arcs = Vec::new();
    let mut handles = Handles::default();
    for entity in ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
    {
        if entity.construction {
            return Err(format!(
                "entity {} is construction geometry",
                entity.id().as_str()
            ));
        }
        match entity.geometry.definition() {
            SketchGeometryDefinition::Point { position } => {
                let at = point(to_plane(position.get()), 0);
                handles
                    .entities
                    .push((entity.id().clone(), Operand::Point(at)));
            }
            SketchGeometryDefinition::Line { start, end } => {
                let start = point(to_plane(start.get()), 1);
                let end = point(to_plane(end.get()), 1);
                handles
                    .entities
                    .push((entity.id().clone(), Operand::Line(lines.len())));
                handles.loci.extend([
                    (SketchLocus::Start(entity.id().clone()), start),
                    (SketchLocus::End(entity.id().clone()), end),
                ]);
                lines.push(Curve::Line { start, end });
            }
            SketchGeometryDefinition::Circle { center, radius } => {
                let centre = to_plane(center.get());
                let radius = radius.get() * METRES_PER_MILLIMETRE;
                let on = point([centre[0] + radius, centre[1]], 1);
                let centre = point(centre, 1);
                handles
                    .entities
                    .push((entity.id().clone(), Operand::Arc(arcs.len())));
                handles
                    .loci
                    .push((SketchLocus::Center(entity.id().clone()), centre));
                arcs.push(Curve::Circle { on, centre });
            }
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let center = center.get();
                let radius = radius.get();
                let at = |angle: f64| {
                    to_plane(Point2::new(
                        center.u + radius * angle.cos(),
                        center.v + radius * angle.sin(),
                    ))
                };
                let (first, second) = (at(start_angle.get()), at(end_angle.get()));
                let (first, second) = if flipped {
                    (second, first)
                } else {
                    (first, second)
                };
                let start = point(first, 2);
                let end = point(second, 2);
                let centre = point(to_plane(center), 2);
                let (at_start, at_end) = if flipped { (end, start) } else { (start, end) };
                handles
                    .entities
                    .push((entity.id().clone(), Operand::Arc(arcs.len())));
                handles.loci.extend([
                    (SketchLocus::Start(entity.id().clone()), at_start),
                    (SketchLocus::End(entity.id().clone()), at_end),
                    (SketchLocus::Center(entity.id().clone()), centre),
                ]);
                arcs.push(Curve::Arc { start, end, centre });
            }
            _ => {
                return Err(format!(
                    "entity {} has no native curve record here",
                    entity.id().as_str()
                ))
            }
        }
    }
    lines.extend(arcs);
    Ok((plane, points, lines, handles, flipped))
}

/// The sketch's relations and dimensions as native relation-table rows and
/// dimension objects, or why the native sketch cannot hold one of them.
/// Coincidences the shared point records carry write no row.
fn constraints(
    ir: &cadmpeg_ir::CadIr,
    sketch: &IrSketch,
    handles: &Handles,
) -> Result<(Vec<Relation>, Vec<Dimension>), String> {
    let mut relations = Vec::new();
    let mut dimensions = Vec::new();
    for constraint in ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == sketch.id)
    {
        let definition = constraint.definition.kind();
        for relation in generated_marker_relations(definition) {
            let (kind, operands) = match relation {
                GeneratedMarkerRelation::Unary(kind, entity) => {
                    (kind, vec![handles.entity(entity)?])
                }
                GeneratedMarkerRelation::Binary(kind, first, second) => {
                    (kind, vec![handles.entity(first)?, handles.entity(second)?])
                }
                GeneratedMarkerRelation::Loci(kind, first, second) => {
                    (kind, vec![handles.locus(first)?, handles.locus(second)?])
                }
                GeneratedMarkerRelation::Midpoint(point, entity) => (
                    SketchRelationKind::Midpoint,
                    vec![handles.locus(point)?, handles.entity(entity)?],
                ),
                GeneratedMarkerRelation::AtIntersection(..)
                | GeneratedMarkerRelation::Symmetric(..) => {
                    return Err(format!(
                        "constraint {} has three operands",
                        constraint.id.as_str()
                    ))
                }
            };
            relations.push(Relation {
                code: kind.native_code(),
                operands,
            });
        }
        let Some(dimension) = generated_dimension(ir, definition) else {
            continue;
        };
        let dimension = dimension.map_err(|error| error.to_string())?;
        let radius = matches!(
            definition,
            cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Radius { .. }
        );
        let (class, code, operands, parameter) = match dimension {
            GeneratedDimension::PointPoint(first, second, parameter) => (
                "sgPntPntDist",
                SketchRelationKind::Distance,
                vec![handles.locus(first)?, handles.locus(second)?],
                parameter,
            ),
            GeneratedDimension::PointLine(point, line, parameter) => (
                "sgPntLineDist",
                SketchRelationKind::Distance,
                vec![handles.locus(point)?, handles.entity(line)?],
                parameter,
            ),
            GeneratedDimension::LineLine(first, second, parameter) => (
                "sgLLDist",
                SketchRelationKind::Distance,
                vec![handles.entity(first)?, handles.entity(second)?],
                parameter,
            ),
            GeneratedDimension::Horizontal(first, second, parameter) => (
                "sgPntPntHorDist",
                SketchRelationKind::Distance,
                vec![handles.locus(first)?, handles.locus(second)?],
                parameter,
            ),
            GeneratedDimension::Vertical(first, second, parameter) => (
                "sgPntPntVertDist",
                SketchRelationKind::Distance,
                vec![handles.locus(first)?, handles.locus(second)?],
                parameter,
            ),
            GeneratedDimension::Angle(first, second, parameter) => (
                "sgAnglDim",
                SketchRelationKind::Angle,
                vec![handles.entity(first)?, handles.entity(second)?],
                parameter,
            ),
            GeneratedDimension::Circle(entity, parameter) => (
                "sgCircleDim",
                if radius {
                    SketchRelationKind::Radius
                } else {
                    SketchRelationKind::Diameter
                },
                vec![handles.entity(entity)?],
                parameter,
            ),
        };
        let parameter = ir
            .model
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter)
            .ok_or_else(|| format!("dimension parameter {} is missing", parameter.as_str()))?;
        let (value, angle) = match &parameter.value {
            Some(cadmpeg_ir::features::ParameterValue::Length(value)) if class != "sgAnglDim" => {
                (value.get() * METRES_PER_MILLIMETRE, false)
            }
            Some(cadmpeg_ir::features::ParameterValue::Angle(value)) if class == "sgAnglDim" => {
                (value.get(), true)
            }
            _ => {
                return Err(format!(
                    "dimension parameter {} has no value of its measure",
                    parameter.id.as_str()
                ))
            }
        };
        dimensions.push(Dimension {
            class,
            name: GENERATED_NAME.fit(&parameter.name).text.to_owned(),
            value,
            angle,
            code: code.native_code(),
            operands,
        });
    }
    Ok((relations, dimensions))
}

fn unit(vector: Vector3) -> Vector3 {
    let length = vector.norm();
    Vector3::new(vector.x / length, vector.y / length, vector.z / length)
}

fn dot(vector: Vector3, axis: [f64; 3]) -> f64 {
    vector.x * axis[0] + vector.y * axis[1] + vector.z * axis[2]
}

fn dot_point(point: Point3, axis: [f64; 3]) -> f64 {
    point.x * axis[0] + point.y * axis[1] + point.z * axis[2]
}
