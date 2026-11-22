// SPDX-License-Identifier: Apache-2.0
//! The user features of a generated part as the native tree writes them,
//! from the CADIR model and the history summary's feature records.

use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch as IrSketch, SketchConstraint, SketchEntityId, SketchGeometryDefinition, SketchLocus,
};

use super::extrude::Extrusion;
use super::sketch::{Curve, Dimension, DimensionClass, Operand, Point, Relation, Sketch};
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

/// A source feature and the bodies its evaluation makes or changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::writer) struct SourceFeature {
    pub(in crate::writer) id: String,
    pub(in crate::writer) outputs: Vec<String>,
}

/// The user features of a generated part, or why the tree writes none of
/// them.
#[derive(Debug)]
pub(in crate::writer) enum Features {
    /// Every sketch and, when the part's body comes from its one extrusion,
    /// that extrusion, in source order; `unwritten_extrusion` names why the
    /// tree writes no extrusion of a part that holds one, and `driven` the
    /// driven (reference) dimensions the sketches leave out.
    Native {
        features: Vec<Feature>,
        unwritten_extrusion: Option<String>,
        driven: Vec<String>,
    },
    /// The sketches stay in the generated lane; the text names the first
    /// sketch the tree cannot write and why.
    Lane(String),
}

/// The user features of `ir`: every planar sketch of the first configuration
/// as a native sketch, when each lies on a default plane and holds only
/// points, lines, arcs and circles that are not construction geometry, and
/// the part holds no spatial sketch; and the extrusion that makes the part's
/// body (see [`extrusion`]). `left_out` holds the source features the write
/// leaves out.
pub(in crate::writer) fn features(
    ir: &cadmpeg_ir::CadIr,
    native: Option<&crate::native::SldprtNative>,
    left_out: &[SourceFeature],
) -> Result<Features, CodecError> {
    if let Some(spatial) = ir.model.spatial_sketches.first() {
        return Ok(Features::Lane(format!(
            "spatial sketch {} has no native sketch form here",
            spatial.id.as_str()
        )));
    }
    if ir.model.sketches.is_empty() {
        return Ok(Features::Native {
            features: Vec::new(),
            unwritten_extrusion: None,
            driven: Vec::new(),
        });
    }
    let Some(native) = native else {
        return Ok(Features::Lane("the write holds no history summary".into()));
    };
    let mut out = Vec::new();
    let mut written = Vec::new();
    let mut driven = Vec::new();
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
                let chains = chains(sketch, &handles, &curves);
                Ok((
                    plane, points, curves, relations, dimensions, chains, flipped,
                ))
            });
        let (plane, points, curves, relations, dimensions, chains, flipped) = match geometry {
            Ok(geometry) => geometry,
            Err(reason) => {
                return Ok(Features::Lane(format!(
                    "sketch {}: {reason}",
                    sketch.id.as_str()
                )))
            }
        };
        driven.extend(
            ir.model
                .sketch_constraints
                .iter()
                .filter(|constraint| constraint.sketch == sketch.id && is_driven(ir, constraint))
                .map(|constraint| constraint.id.as_str().to_owned()),
        );
        written.push(WrittenSketch {
            id: sketch.id.clone(),
            ordinal: owner.ordinal,
            feature_id,
            flipped,
        });
        out.push((
            owner.ordinal,
            Feature::Sketch(Sketch {
                name,
                feature_id,
                plane,
                points,
                curves,
                relations,
                dimensions,
                chains,
                body: generated_profile_stream(ir, sketch)?,
            }),
        ));
    }
    let unwritten_extrusion = match extrusion(ir, native, &written, left_out) {
        Ok(extrusion) => {
            out.extend(extrusion);
            None
        }
        Err(reason) => Some(reason),
    };
    out.sort_by_key(|(ordinal, _)| *ordinal);
    Ok(Features::Native {
        features: out.into_iter().map(|(_, feature)| feature).collect(),
        unwritten_extrusion,
        driven,
    })
}

/// A sketch the tree writes: its id, its owner's ordinal, its object
/// identifier and whether its normal opposes its plane's.
struct WrittenSketch {
    id: cadmpeg_ir::sketches::SketchId,
    ordinal: u64,
    feature_id: u32,
    flipped: bool,
}

/// The extrusion of `ir` with its ordinal and the sketch it extrudes, none
/// when `ir` holds no extrusion, or why the tree writes none: the tree writes
/// an extrusion when every source feature the write leaves out made only
/// bodies the write omits (see [`left_out_reason`]), the part holds one body,
/// and its features are written sketches and one active extrusion that makes
/// that body, one-sided and blind, from the plane of every profile of one
/// written sketch that precedes it, along its normal, without draft. A second
/// extrusion would take the part's one `moAtom_c` as its input too, which no
/// native part does.
fn extrusion(
    ir: &cadmpeg_ir::CadIr,
    native: &crate::native::SldprtNative,
    sketches: &[WrittenSketch],
    left_out: &[SourceFeature],
) -> Result<Option<(u64, Feature)>, String> {
    use cadmpeg_ir::features::{
        extents::{ExtrudeDirection, ExtrudeExtent, ExtrudeStart, LinearTermination},
        operation_settings::BooleanOp,
        profiles::{PlanarProfileRef, ProfileRef},
        FeatureDefinition, FeatureOperation,
    };
    let mut extrusions =
        ir.model
            .features
            .iter()
            .filter_map(|feature| match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Extrude {
                    profile,
                    direction,
                    start,
                    extent,
                    op,
                    solid,
                    ..
                }) => Some((feature, (profile, direction, start, extent, op, solid))),
                _ => None,
            });
    let Some((feature, (profile, direction, start, extent, op, solid))) = extrusions.next() else {
        return Ok(None);
    };
    let id = feature.id.as_str();
    let made = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .chain(
            feature
                .evaluation
                .outputs()
                .iter()
                .map(cadmpeg_ir::ids::BodyId::as_str),
        )
        .collect::<Vec<_>>();
    if let Some(reason) = left_out_reason(left_out, &made) {
        return Err(reason);
    }
    if feature.suppressed == Some(true) {
        return Err(format!("extrusion {id} is suppressed"));
    }
    if let Some((second, _)) = extrusions.next() {
        return Err(format!(
            "extrusion {} follows extrusion {id}, and the part's one moAtom_c can be the input \
             of one extrusion",
            second.id.as_str()
        ));
    }
    if ir.geometry.bodies.len() != 1 {
        return Err(format!(
            "the part holds {} bodies",
            ir.geometry.bodies.len()
        ));
    }
    if let Some(other) = ir.model.features.iter().find(|candidate| {
        !matches!(
            candidate.evaluation.definition(),
            FeatureDefinition::Operation(
                FeatureOperation::Sketch { .. } | FeatureOperation::Extrude { .. }
            )
        )
    }) {
        return Err(format!(
            "feature {} is neither a sketch nor an extrusion",
            other.id.as_str()
        ));
    }
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
                return Err(format!(
                    "extrusion {id} extrudes some of its sketch's profiles"
                ));
            }
            sketch
        }
        _ => return Err(format!("extrusion {id} extrudes no planar sketch")),
    };
    let sketch = sketches
        .iter()
        .find(|candidate| candidate.id == *sketch)
        .ok_or_else(|| format!("extrusion {id} extrudes a sketch the tree does not write"))?;
    // The tree writes features in source order, and an extrusion names its
    // sketch as an earlier object.
    if sketch.ordinal >= feature.ordinal {
        return Err(format!(
            "extrusion {id} does not follow its sketch {} in the source order",
            sketch.id.as_str()
        ));
    }
    let ExtrudeExtent::OneSided { side } = extent else {
        return Err(format!("extrusion {id} is not one-sided"));
    };
    let LinearTermination::Blind { length } = side.termination else {
        return Err(format!("extrusion {id} is not blind"));
    };
    let against = match direction {
        ExtrudeDirection::ProfileNormal {} => false,
        ExtrudeDirection::ReversedProfileNormal {} => true,
        _ => return Err(format!("extrusion {id} runs along no profile normal")),
    };
    if !matches!(op, BooleanOp::NewBody) {
        return Err(format!("extrusion {id} makes no new body"));
    }
    if side.draft.is_some()
        || !matches!(start, ExtrudeStart::ProfilePlane {})
        || *solid == Some(false)
    {
        return Err(format!(
            "extrusion {id} has a draft, a start off the sketch plane or a surface result"
        ));
    }
    let record_id = feature.native_ref.clone().unwrap_or_else(|| {
        crate::history::write::features::generated_feature_record_id(&feature.id)
    });
    let record = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|record| record.id == record_id)
        .ok_or_else(|| format!("extrusion {id} has no history summary record"))?;
    let feature_id = record
        .source_value()
        .ok_or_else(|| format!("extrusion {id} has no history summary source id"))?;
    let name = if record.name.is_empty() {
        feature.name.as_deref().unwrap_or(id)
    } else {
        record.name.as_str()
    };
    let depth = length.get();
    Ok(Some((
        feature.ordinal,
        Feature::Extrusion(Extrusion {
            name: GENERATED_NAME.fit(name).text.to_owned(),
            feature_id,
            sketch_id: sketch.feature_id,
            depth: depth.abs() * METRES_PER_MILLIMETRE,
            reversed: against ^ sketch.flipped ^ (depth < 0.0),
        }),
    )))
}

/// Why the native features would not make the part's current body when the
/// write leaves out `left_out`, or none when every body a left-out feature
/// makes or changes is one the write omits; `made` names the bodies the write
/// keeps and the bodies the extrusion makes. A left-out feature that records
/// no body may have changed the kept one.
fn left_out_reason(left_out: &[SourceFeature], made: &[&str]) -> Option<String> {
    left_out.iter().find_map(|feature| {
        if feature.outputs.is_empty() {
            return Some(format!(
                "the write leaves out source feature {}, which records no body it makes or \
                 changes, so the native features may not make the part's current body",
                feature.id
            ));
        }
        feature
            .outputs
            .iter()
            .find(|body| made.contains(&body.as_str()))
            .map(|body| {
                format!(
                    "the write leaves out source feature {}, which makes or changes body {body} \
                     that the extrusion makes, so the native extrusion would not make the \
                     part's current body",
                    feature.id
                )
            })
    })
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

/// The curve positions of each profile chain of `sketch`, or no chains when
/// a chain names an entity the sketch writes as no curve: the chain list is
/// then empty, as a native sketch's can be.
fn chains(sketch: &IrSketch, handles: &Handles, curves: &[Curve]) -> Vec<Vec<usize>> {
    let lines = curves
        .iter()
        .take_while(|curve| matches!(curve, Curve::Line { .. }))
        .count();
    sketch
        .profiles
        .as_slice()
        .iter()
        .map(|chain| {
            chain
                .iter()
                .map(|usage| match handles.entity(&usage.entity) {
                    Ok(Operand::Line(position)) => Some(position),
                    Ok(Operand::Arc(position)) => Some(lines + position),
                    Ok(Operand::Point(_)) | Err(_) => None,
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
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
        .filter(|constraint| constraint.sketch == sketch.id && !is_driven(ir, constraint))
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
                DimensionClass::PointPoint,
                SketchRelationKind::Distance,
                vec![handles.locus(first)?, handles.locus(second)?],
                parameter,
            ),
            GeneratedDimension::PointLine(point, line, parameter) => (
                DimensionClass::PointLine,
                SketchRelationKind::Distance,
                vec![handles.locus(point)?, handles.entity(line)?],
                parameter,
            ),
            GeneratedDimension::LineLine(first, second, parameter) => (
                DimensionClass::LineLine,
                SketchRelationKind::Distance,
                vec![handles.entity(first)?, handles.entity(second)?],
                parameter,
            ),
            GeneratedDimension::Horizontal(first, second, parameter) => (
                DimensionClass::Horizontal,
                SketchRelationKind::Distance,
                vec![handles.locus(first)?, handles.locus(second)?],
                parameter,
            ),
            GeneratedDimension::Vertical(..) | GeneratedDimension::Angle(..) => {
                return Err(format!(
                    "constraint {} is a vertical or angular dimension, whose bytes after the \
                     parameter sldprt.md does not state",
                    constraint.id.as_str()
                ))
            }
            GeneratedDimension::Circle(entity, parameter) => (
                DimensionClass::Circle,
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
        let Some(cadmpeg_ir::features::parameters::ParameterValue::Length(value)) =
            &parameter.value
        else {
            return Err(format!(
                "dimension parameter {} has no length value",
                parameter.id.as_str()
            ));
        };
        dimensions.push(Dimension {
            class,
            name: GENERATED_NAME.fit(&parameter.name).text.to_owned(),
            value: value.get() * METRES_PER_MILLIMETRE,
            code: code.native_code(),
            operands,
        });
    }
    Ok((relations, dimensions))
}

/// Whether `constraint` is a driven (reference) dimension: its record says it
/// is not driving, or the design states its parameter `Driven`. A native
/// sketch dimension is driving, so the sketch writes no driven one.
fn is_driven(ir: &cadmpeg_ir::CadIr, constraint: &SketchConstraint) -> bool {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;
    let (Definition::DistanceLoci { parameter, .. }
    | Definition::Distance { parameter, .. }
    | Definition::HorizontalDistance { parameter, .. }
    | Definition::VerticalDistance { parameter, .. }
    | Definition::Angle { parameter, .. }
    | Definition::Radius { parameter, .. }
    | Definition::Diameter { parameter, .. }) = constraint.definition.kind()
    else {
        return false;
    };
    constraint.driving == Some(false)
        || ir.design.parameters.iter().any(|candidate| {
            candidate.id.as_str() == parameter.as_str()
                && matches!(
                    candidate.definition,
                    cadmpeg_ir::design::parameter::ParameterDefinition::Driven
                )
        })
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

#[cfg(test)]
mod tests {
    use super::{left_out_reason, SourceFeature};

    fn left_out(id: &str, outputs: &[&str]) -> SourceFeature {
        SourceFeature {
            id: id.into(),
            outputs: outputs.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn a_left_out_feature_that_made_only_omitted_bodies_keeps_the_extrusion() {
        let coil = left_out("feature#coil", &["body#coil"]);
        assert_eq!(left_out_reason(&[coil], &["body#boss"]), None);
    }

    #[test]
    fn a_left_out_feature_that_changed_the_extruded_body_keeps_the_stored_body() {
        let draft = left_out("feature#draft", &["body#boss"]);
        let reason = left_out_reason(&[draft], &["body#boss"]).expect("a reason");
        assert!(
            reason.contains("feature#draft, which makes or changes body body#boss"),
            "{reason}"
        );
    }

    #[test]
    fn a_left_out_feature_with_no_recorded_body_keeps_the_stored_body() {
        let coil = left_out("feature#coil", &["body#coil"]);
        let insert = left_out("feature#insert", &[]);
        let reason = left_out_reason(&[coil, insert], &["body#boss"]).expect("a reason");
        assert!(
            reason.contains("feature#insert, which records no body"),
            "{reason}"
        );
    }
}
