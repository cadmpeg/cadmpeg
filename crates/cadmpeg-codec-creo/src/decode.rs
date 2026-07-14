// SPDX-License-Identifier: Apache-2.0
//! Conversion from a PSB container to [`CadIr`].
//!
//! Decode transfers standard datum planes as derived plane surfaces and
//! preserves each geometry section as an [`UnknownRecord`]. Source metadata
//! records the layout, namespace census, active units, and counts of decoded
//! structural rows.
//!
//! Surface and curve namespaces contain useful topology and prototype data, but
//! the placed body model is incomplete. The report therefore records blocking
//! geometry and topology losses instead of emitting a partial B-rep.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::features::{
    Angle, BooleanOp, DesignParameter, Extent, Feature, FeatureDefinition as IrFeatureDefinition,
    FeatureId as IrFeatureId, Length, ParameterId, ParameterValue, ProfileRef, RevolutionAxis,
    RevolutionConstruction,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, ProceduralSurface, ProceduralSurfaceDefinition,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ProceduralSurfaceId, RegionId,
    ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};
use serde::Serialize;

use crate::container::{self, role, ContainerScan};
use crate::topology::HalfEdgeId;

#[derive(Serialize)]
struct CreoSketchRecord {
    id: String,
    definition_id: u32,
    owner_feature_id: Option<u32>,
    variables: Vec<CreoSketchVariable>,
    segments: Vec<CreoSketchSegment>,
    dimensions: Vec<CreoSketchDimension>,
    relations: Vec<CreoSketchRelation>,
    skamps: Vec<CreoSketchSkamp>,
    relation_triples: Vec<CreoSketchRelationTriple>,
}

#[derive(Serialize)]
struct CreoSketchVariable {
    variable_type: u32,
    key: u32,
    value: Option<f64>,
    guess: Option<f64>,
    uvar_id: Option<u32>,
    dimension_driven: bool,
}

#[derive(Serialize)]
struct CreoSketchSegment {
    external_id: u32,
    kind: &'static str,
    point_ids: [u32; 2],
    center_id: Option<u32>,
    directions: [Option<u32>; 3],
    arc_orientation: Option<u32>,
    vertical_horizontal_constraint: Option<u32>,
    radius_dimension_id: Option<u32>,
    secondary_radius_dimension_id: Option<u32>,
}

#[derive(Serialize)]
struct CreoSketchDimension {
    external_id: u32,
    dimension_type: u32,
    value: Option<f64>,
    unit: &'static str,
    direction_byte: u8,
    auxiliary_value: Option<f64>,
}

#[derive(Serialize)]
struct CreoSketchRelation {
    relation_id: u32,
    used: u32,
    operands: Vec<u8>,
    operand_vectors: Option<[[Option<u32>; 4]; 3]>,
    sign: u32,
    dimension_id: u32,
    relation_type: u32,
    body: Vec<u8>,
}

#[derive(Serialize)]
struct CreoSketchSkamp {
    id: u32,
    kind: u32,
    flags: u32,
    status: u32,
    items: Vec<CreoSketchSkampItem>,
}

#[derive(Serialize)]
struct CreoSketchSkampItem {
    entity_id: u32,
    sense: u32,
}

#[derive(Serialize)]
struct CreoSketchRelationTriple {
    #[serde(rename = "relation_id")]
    relation: Option<u32>,
    #[serde(rename = "equation_id")]
    equation: Option<u32>,
    #[serde(rename = "skamp_id")]
    skamp: Option<u32>,
}

#[derive(Serialize)]
struct CreoCurveExpressionRecord {
    id: String,
    entity_id: u32,
    backup: bool,
    local_system: Option<CreoCurveExpressionLocalSystem>,
    lines: Vec<CreoCurveExpressionLine>,
    assignments: Vec<CreoCurveExpressionAssignment>,
}

#[derive(Serialize)]
struct CreoCurveExpressionLocalSystem {
    dimensions: u8,
    count: u8,
    body: Vec<u8>,
    offset: usize,
}

#[derive(Serialize)]
struct CreoCurveExpressionLine {
    text: String,
    offset: usize,
}

#[derive(Serialize)]
struct CreoCurveExpressionAssignment {
    name: String,
    expression: String,
    dependencies: Vec<String>,
    value: Option<f64>,
    offset: usize,
}

fn curve_expression_records(scan: &ContainerScan) -> Vec<CreoCurveExpressionRecord> {
    scan.curve_expressions
        .iter()
        .map(|record| CreoCurveExpressionRecord {
            id: curve_expression_record_id(record),
            entity_id: record.entity_id,
            backup: record.backup,
            local_system: record.local_system.as_ref().map(|frame| {
                CreoCurveExpressionLocalSystem {
                    dimensions: frame.dimensions,
                    count: frame.count,
                    body: frame.body.clone(),
                    offset: frame.offset,
                }
            }),
            lines: record
                .lines
                .iter()
                .map(|line| CreoCurveExpressionLine {
                    text: line.text.clone(),
                    offset: line.offset,
                })
                .collect(),
            assignments: record
                .assignments
                .iter()
                .map(|assignment| CreoCurveExpressionAssignment {
                    name: assignment.name.clone(),
                    expression: assignment.expression.clone(),
                    dependencies: assignment.dependencies.clone(),
                    value: assignment.value,
                    offset: assignment.offset,
                })
                .collect(),
        })
        .collect()
}

fn curve_expression_record_id(record: &crate::curve::CurveExpressionRecord) -> String {
    format!(
        "creo:depdb:curve_expression#{}-{}-{}",
        if record.backup { "backup" } else { "active" },
        record.entity_id,
        record.offset
    )
}

fn transfer_curve_expression_features(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let ordinal_base = ir
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .map_or(0, |value| value + 1);
    for (expression_ordinal, record) in scan
        .curve_expressions
        .iter()
        .filter(|record| !record.backup)
        .enumerate()
    {
        let ordinal = ordinal_base + expression_ordinal as u64;
        let feature_id = IrFeatureId(format!(
            "creo:depdb:curve_expression_feature#{}-{}",
            record.entity_id, record.offset
        ));
        let mut parameters_by_name = BTreeMap::<String, ParameterId>::new();
        for (assignment_ordinal, assignment) in record.assignments.iter().enumerate() {
            let parameter_id = ParameterId(format!(
                "creo:depdb:curve_expression_parameter#{}-{}-{}",
                record.entity_id, record.offset, assignment_ordinal
            ));
            let dependencies = assignment
                .dependencies
                .iter()
                .filter_map(|name| parameters_by_name.get(name).cloned())
                .collect::<Vec<_>>();
            let external_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| !parameters_by_name.contains_key(*name))
                .cloned()
                .collect::<Vec<_>>();
            let mut properties = BTreeMap::new();
            if !external_dependencies.is_empty() {
                properties.insert(
                    "external_dependencies".to_string(),
                    external_dependencies.join(","),
                );
            }
            annotate(
                annotations,
                &parameter_id.0,
                "DEPDB_DATA",
                assignment.offset as u64,
                "curve_expression_assignment",
                Exactness::Derived,
            );
            ir.model.parameters.push(DesignParameter {
                id: parameter_id.clone(),
                owner: feature_id.clone(),
                ordinal: assignment_ordinal as u32,
                name: assignment.name.clone(),
                expression: assignment.expression.clone(),
                display: None,
                value: assignment.value.map(ParameterValue::Real),
                dependencies,
                properties,
                pmi: None,
                native_ref: Some(curve_expression_record_id(record)),
            });
            parameters_by_name.insert(assignment.name.clone(), parameter_id);
        }
        annotate(
            annotations,
            &feature_id.0,
            "DEPDB_DATA",
            record.expression_offset as u64,
            "curve_expression_feature",
            Exactness::Derived,
        );
        let definition = crate::curve::expression_helix(record).map_or_else(
            || IrFeatureDefinition::Native {
                kind: "CurveFromEquation".to_string(),
                parameters: BTreeMap::from([
                    ("entity_id".to_string(), record.entity_id.to_string()),
                    (
                        "assignment_count".to_string(),
                        record.assignments.len().to_string(),
                    ),
                ]),
                properties: BTreeMap::new(),
            },
            |helix| IrFeatureDefinition::HelixNativeAxis {
                axis_native_ref: curve_expression_record_id(record),
                radius: Length(helix.radius),
                height: Length(helix.height),
                revolutions: helix.revolutions,
                start_angle: Angle(helix.start_angle),
                clockwise: helix.clockwise,
            },
        );
        ir.model.features.push(Feature {
            id: feature_id,
            ordinal,
            name: Some(format!("Curve Equation {}", record.entity_id)),
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("crv_fr_eqn".to_string()),
            source_text: Some(
                record
                    .lines
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: Some(curve_expression_record_id(record)),
        });
    }
}

fn sketch_records(scan: &ContainerScan) -> Vec<CreoSketchRecord> {
    scan.feature_definitions
        .iter()
        .filter(|definition| {
            definition.variables.is_some()
                || definition.segments.is_some()
                || definition.dimensions.is_some()
                || definition.relations.is_some()
        })
        .map(|definition| CreoSketchRecord {
            id: format!("creo:featdefs:sketch#{}", definition.id),
            definition_id: definition.id,
            owner_feature_id: definition.owner_feature_id,
            variables: definition
                .variables
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| CreoSketchVariable {
                    variable_type: row.variable_type,
                    key: row.key,
                    value: row.value,
                    guess: row.guess,
                    uvar_id: row.uvar_id,
                    dimension_driven: row.dimension_driven,
                })
                .collect(),
            segments: definition
                .segments
                .iter()
                .flat_map(|table| &table.rows)
                .map(|segment| CreoSketchSegment {
                    external_id: segment.external_id,
                    kind: match segment.kind {
                        crate::feature::FeatureSegmentKind::Line => "line",
                        crate::feature::FeatureSegmentKind::Arc => "arc",
                        crate::feature::FeatureSegmentKind::Point => "point",
                    },
                    point_ids: segment.point_ids,
                    center_id: segment.center_id,
                    directions: segment.directions,
                    arc_orientation: segment.arc_orientation,
                    vertical_horizontal_constraint: segment.vertical_horizontal,
                    radius_dimension_id: segment.radius_ref,
                    secondary_radius_dimension_id: segment.radius2_ref,
                })
                .collect(),
            dimensions: definition
                .dimensions
                .iter()
                .flat_map(|table| &table.rows)
                .map(|dimension| CreoSketchDimension {
                    external_id: dimension.external_id,
                    dimension_type: dimension.dimension_type,
                    value: dimension.value,
                    unit: match dimension.value_unit {
                        crate::feature::DimensionUnit::Radians => "radians",
                        crate::feature::DimensionUnit::Millimeters => "millimeters",
                        crate::feature::DimensionUnit::SchemaDefined => "schema_defined",
                    },
                    direction_byte: dimension.direction_byte,
                    auxiliary_value: dimension.auxiliary_value,
                })
                .collect(),
            relations: definition
                .relations
                .iter()
                .flat_map(|table| &table.rows)
                .map(|relation| CreoSketchRelation {
                    relation_id: relation.relation_id,
                    used: relation.used,
                    operands: relation.operands.clone(),
                    operand_vectors: relation.operand_vectors,
                    sign: relation.sign,
                    dimension_id: relation.dimension_id,
                    relation_type: relation.relation_type,
                    body: relation.body.clone(),
                })
                .collect(),
            skamps: definition
                .relations
                .iter()
                .flat_map(|table| &table.skamps)
                .map(|skamp| CreoSketchSkamp {
                    id: skamp.id,
                    kind: skamp.kind,
                    flags: skamp.flags,
                    status: skamp.status,
                    items: skamp
                        .items
                        .iter()
                        .map(|item| CreoSketchSkampItem {
                            entity_id: item.entity_id,
                            sense: item.sense,
                        })
                        .collect(),
                })
                .collect(),
            relation_triples: definition
                .relations
                .iter()
                .flat_map(|table| &table.triples)
                .map(|triple| CreoSketchRelationTriple {
                    relation: triple.relation_id,
                    equation: triple.equation_id,
                    skamp: triple.skamp_id,
                })
                .collect(),
        })
        .collect()
}

fn section_line_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let start = points.get(&segment.point_ids[0])?;
    let end = points.get(&segment.point_ids[1])?;
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
}

fn section_point_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Point).then_some(())?;
    let position = points.get(&segment.point_ids[0])?;
    Some(SketchGeometry::Point {
        position: cadmpeg_ir::math::Point2::new(position[0], position[1]),
    })
}

fn section_arc_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Arc && segment.arc_orientation == Some(0))
        .then_some(())?;
    let center = points.get(&segment.center_id?)?;
    let first = points.get(&segment.point_ids[0])?;
    let second = points.get(&segment.point_ids[1])?;
    let offset = |point: &[f64; 2]| [point[0] - center[0], point[1] - center[1]];
    let first_offset = offset(first);
    let second_offset = offset(second);
    let first_radius = first_offset[0].hypot(first_offset[1]);
    let second_radius = second_offset[0].hypot(second_offset[1]);
    let scale = first_radius.max(second_radius).max(1.0);
    if first_radius <= 1e-12 || (first_radius - second_radius).abs() > 1e-9 * scale {
        return None;
    }
    let start = second_offset[1].atan2(second_offset[0]);
    let mut end = first_offset[1].atan2(first_offset[0]);
    while end <= start {
        end += std::f64::consts::TAU;
    }
    Some(SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(center[0], center[1]),
        radius: Length(first_radius),
        start_angle: Angle(start),
        end_angle: Angle(end),
    })
}

fn section_segment_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    section_line_geometry(points, segment)
        .or_else(|| section_arc_geometry(points, segment))
        .or_else(|| section_point_geometry(points, segment))
}

fn saved_section_line_geometry(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let order_table = definition.order_table.as_ref()?;
    let saved_section = definition.saved_section.as_ref()?;
    let internal_id = order_table
        .rows
        .iter()
        .find(|row| row.external_id == segment.external_id)
        .map(|row| row.internal_id)
        .or_else(|| {
            let segments = &definition.segments.as_ref()?.rows;
            let position = segments
                .iter()
                .position(|candidate| candidate.external_id == segment.external_id)?;
            let previous = segments[..position].iter().rev().find_map(|candidate| {
                order_table
                    .rows
                    .iter()
                    .find(|row| row.external_id == candidate.external_id)
                    .map(|row| row.internal_id)
            })?;
            let next = segments[position + 1..].iter().find_map(|candidate| {
                order_table
                    .rows
                    .iter()
                    .find(|row| row.external_id == candidate.external_id)
                    .map(|row| row.internal_id)
            })?;
            let internal_id = previous.checked_add(1)?;
            (next == internal_id + 1
                && saved_section.entities.iter().any(|entity| {
                    matches!(entity, crate::feature::FeatureSavedEntity::Line(line) if line.entity_id == internal_id)
                }))
            .then_some(internal_id)
        })
        .or_else(|| {
            let trimmed = definition.trim_entities.as_ref()?;
            let segment_ids = definition
                .segments
                .as_ref()?
                .rows
                .iter()
                .filter(|candidate| {
                    candidate.kind == crate::feature::FeatureSegmentKind::Line
                        && trimmed
                            .rows
                            .iter()
                            .any(|row| row.external_id == candidate.external_id)
                        && !order_table
                            .rows
                            .iter()
                            .any(|row| row.external_id == candidate.external_id)
                })
                .map(|candidate| candidate.external_id)
                .collect::<Vec<_>>();
            let saved_ids = saved_section
                .entities
                .iter()
                .filter_map(|entity| match entity {
                    crate::feature::FeatureSavedEntity::Line(line)
                        if !order_table
                            .rows
                            .iter()
                            .any(|row| row.internal_id == line.entity_id) =>
                    {
                        Some(line.entity_id)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            match (segment_ids.as_slice(), saved_ids.as_slice()) {
                ([external_id], [internal_id]) if *external_id == segment.external_id => {
                    Some(*internal_id)
                }
                _ => None,
            }
        })?;
    let line = saved_section
        .entities
        .iter()
        .find_map(|entity| match entity {
            crate::feature::FeatureSavedEntity::Line(line) if line.entity_id == internal_id => {
                Some(line)
            }
            _ => None,
        })?;
    let [[Some(start_u), Some(start_v), _], [Some(end_u), Some(end_v), _]] = line.endpoints else {
        return None;
    };
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start_u, start_v),
        end: cadmpeg_ir::math::Point2::new(end_u, end_v),
    })
}

fn saved_section_arc_record<'a>(
    definition: &'a crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<&'a crate::feature::FeatureSavedArc> {
    (segment.kind == crate::feature::FeatureSegmentKind::Arc && segment.arc_orientation == Some(0))
        .then_some(())?;
    let internal_id = definition
        .order_table
        .as_ref()?
        .rows
        .iter()
        .find(|row| row.external_id == segment.external_id)?
        .internal_id;
    definition
        .saved_section
        .as_ref()?
        .entities
        .iter()
        .find_map(|entity| match entity {
            crate::feature::FeatureSavedEntity::Arc(arc) if arc.entity_id == internal_id => {
                Some(arc)
            }
            _ => None,
        })
}

fn saved_section_arc_carrier(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<([f64; 2], f64)> {
    let arc = saved_section_arc_record(definition, segment)?;
    let [center_u, center_v, _] = arc.center;
    if let ([Some(center_u), Some(center_v)], Some(radius)) = (
        [center_u, center_v],
        arc.radius.filter(|radius| *radius > 1e-12),
    ) {
        return Some(([center_u, center_v], radius));
    }
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return None;
    };
    let scale = [first_u, first_v, second_u, second_v]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
    let [center_u, center_v] = match [center_u, center_v] {
        [Some(u), Some(v)] => [u, v],
        [Some(u), None] => {
            let denominator = 2.0 * (second_v - first_v);
            if denominator.abs() <= 1e-12 * scale {
                return None;
            }
            let v = ((second_u - u).mul_add(
                second_u - u,
                second_v * second_v - (first_u - u) * (first_u - u) - first_v * first_v,
            )) / denominator;
            [u, v]
        }
        [None, Some(v)] => {
            let denominator = 2.0 * (second_u - first_u);
            if denominator.abs() <= 1e-12 * scale {
                return None;
            }
            let u = ((second_v - v).mul_add(
                second_v - v,
                second_u * second_u - (first_v - v) * (first_v - v) - first_u * first_u,
            )) / denominator;
            [u, v]
        }
        [None, None] => return None,
    };
    let first_radius = (first_u - center_u).hypot(first_v - center_v);
    let second_radius = (second_u - center_u).hypot(second_v - center_v);
    let radial_scale = first_radius.max(second_radius).max(1.0);
    if first_radius <= 1e-12
        || (first_radius - second_radius).abs() > 1e-9 * radial_scale
        || arc.radius.is_some_and(|stored| {
            (stored - first_radius).abs() > 1e-9 * stored.max(first_radius).max(1.0)
        })
    {
        return None;
    }
    let radius = arc.radius.unwrap_or(first_radius);
    Some(([center_u, center_v], radius))
}

fn saved_section_arc_geometry(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let arc = saved_section_arc_record(definition, segment)?;
    let ([center_u, center_v], radius) = saved_section_arc_carrier(definition, segment)?;
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return None;
    };
    let first = [first_u - center_u, first_v - center_v];
    let second = [second_u - center_u, second_v - center_v];
    let first_radius = first[0].hypot(first[1]);
    let second_radius = second[0].hypot(second[1]);
    let scale = radius.max(first_radius).max(second_radius).max(1.0);
    if (first_radius - radius).abs() > 1e-9 * scale || (second_radius - radius).abs() > 1e-9 * scale
    {
        return None;
    }
    let start = second[1].atan2(second[0]);
    let mut end = first[1].atan2(first[0]);
    while end <= start {
        end += std::f64::consts::TAU;
    }
    Some(SketchGeometry::Arc {
        center: cadmpeg_ir::math::Point2::new(center_u, center_v),
        radius: Length(radius),
        start_angle: Angle(start),
        end_angle: Angle(end),
    })
}

fn resolved_section_segment_geometry(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    section_segment_geometry(points, segment)
        .or_else(|| saved_section_line_geometry(definition, segment))
        .or_else(|| saved_section_arc_geometry(definition, segment))
}

pub(crate) fn resolved_section_points(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, [f64; 2]> {
    let Some(variables) = &definition.variables else {
        return BTreeMap::new();
    };
    let mut points = variables
        .points
        .iter()
        .map(|point| (point.point_id, [point.u, point.v]))
        .collect::<BTreeMap<_, _>>();
    let segments = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .collect::<Vec<_>>();
    let signed_dimensions = definition
        .relations
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|relation| {
            if relation.relation_type != 0 {
                return None;
            }
            let vectors = relation.operand_vectors?;
            if vectors[0][2..] != [None, Some(1)]
                || vectors[1] != [Some(1), Some(1), Some(0), Some(1)]
                || vectors[2] != [Some(15), Some(16), Some(15), Some(1)]
            {
                return None;
            }
            let [Some(first), Some(second), _, _] = vectors[0] else {
                return None;
            };
            let segment = segments.iter().find(|segment| {
                segment.point_ids == [first, second] || segment.point_ids == [second, first]
            })?;
            let coordinate = match segment.vertical_horizontal {
                Some(0) => 1,
                Some(1) => 0,
                _ => return None,
            };
            let magnitude = definition
                .dimensions
                .as_ref()?
                .rows
                .get(usize::try_from(relation.dimension_id).ok()?)?
                .value
                .filter(|value| value.is_finite() && *value >= 0.0)?;
            let delta = match relation.sign {
                1 => magnitude,
                0xf6 => -magnitude,
                _ => return None,
            };
            Some((first, second, coordinate, delta))
        })
        .collect::<Vec<_>>();
    loop {
        let mut changed = false;
        for segment in &segments {
            let coordinate = match segment.vertical_horizontal {
                Some(0) => 0,
                Some(1) => 1,
                _ => continue,
            };
            let [first_id, second_id] = segment.point_ids;
            let [first, second] =
                [first_id, second_id].map(|id| points.get(&id).copied().unwrap_or([None, None]));
            match [first[coordinate], second[coordinate]] {
                [Some(value), None] => {
                    points.entry(second_id).or_insert([None, None])[coordinate] = Some(value);
                    changed = true;
                }
                [None, Some(value)] => {
                    points.entry(first_id).or_insert([None, None])[coordinate] = Some(value);
                    changed = true;
                }
                _ => {}
            }
        }
        for &(first_id, second_id, coordinate, delta) in &signed_dimensions {
            let [first, second] =
                [first_id, second_id].map(|id| points.get(&id).copied().unwrap_or([None, None]));
            match [first[coordinate], second[coordinate]] {
                [Some(value), None] => {
                    points.entry(second_id).or_insert([None, None])[coordinate] =
                        Some(value + delta);
                    changed = true;
                }
                [None, Some(value)] => {
                    points.entry(first_id).or_insert([None, None])[coordinate] =
                        Some(value - delta);
                    changed = true;
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }
    points
        .into_iter()
        .filter_map(|(id, [u, v])| Some((id, [u?, v?])))
        .collect()
}

pub(crate) fn resolved_section_radii(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, f64> {
    let mut radii = definition
        .variables
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|row| {
            (row.variable_type == 3).then_some((row.key, row.value.filter(|value| *value > 0.0)?))
        })
        .collect::<BTreeMap<_, _>>();
    for relation in definition.relations.iter().flat_map(|table| &table.rows) {
        if relation.relation_type != 14 || relation.sign != 1 {
            continue;
        }
        let Some(vectors) = relation.operand_vectors else {
            continue;
        };
        let [Some(radius_id), Some(0), Some(0), Some(0)] = vectors[0] else {
            continue;
        };
        if vectors[1] != [Some(0); 4] || vectors[2] != [Some(15), Some(0), Some(0), Some(0)] {
            continue;
        }
        let Some(value) = definition
            .dimensions
            .as_ref()
            .and_then(|table| table.rows.get(usize::try_from(relation.dimension_id).ok()?))
            .and_then(|dimension| dimension.value)
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            continue;
        };
        radii.entry(radius_id).or_insert(value);
    }
    radii
}

fn section_arc_carrier(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<([f64; 2], f64)> {
    (segment.kind == crate::feature::FeatureSegmentKind::Arc).then_some(())?;
    let center = *points.get(&segment.center_id?)?;
    let radius = *resolved_section_radii(definition).get(&segment.radius_ref?)?;
    Some((center, radius))
}

struct SectionIntersectionCarrier {
    geometry: SketchGeometry,
    line_is_bounded: bool,
}

fn section_axis_line_carrier(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let points = &definition.variables.as_ref()?.points;
    let endpoint = |id| points.iter().find(|point| point.point_id == id);
    let [first, second] = segment.point_ids.map(endpoint);
    let (Some(first), Some(second)) = (first, second) else {
        return None;
    };
    let scale = first
        .u
        .into_iter()
        .chain(first.v)
        .chain(second.u)
        .chain(second.v)
        .map(f64::abs)
        .fold(1.0, f64::max);
    if segment.directions[0] == Some(0) {
        let (Some(first_u), Some(second_u)) = (first.u, second.u) else {
            return None;
        };
        ((first_u - second_u).abs() <= 1e-9 * scale).then(|| SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(first_u, -scale),
            end: cadmpeg_ir::math::Point2::new(first_u, scale),
        })
    } else if segment.directions[1] == Some(0) {
        let (Some(first_v), Some(second_v)) = (first.v, second.v) else {
            return None;
        };
        ((first_v - second_v).abs() <= 1e-9 * scale).then(|| SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-scale, first_v),
            end: cadmpeg_ir::math::Point2::new(scale, first_v),
        })
    } else {
        None
    }
}

fn section_segment_intersection_carrier(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SectionIntersectionCarrier> {
    if let Some(geometry) = resolved_section_segment_geometry(definition, points, segment) {
        return Some(SectionIntersectionCarrier {
            line_is_bounded: matches!(geometry, SketchGeometry::Line { .. }),
            geometry,
        });
    }
    if let Some(geometry) = section_axis_line_carrier(definition, segment) {
        return Some(SectionIntersectionCarrier {
            geometry,
            line_is_bounded: false,
        });
    }
    let ([center_u, center_v], radius) = section_arc_carrier(definition, points, segment)
        .or_else(|| saved_section_arc_carrier(definition, segment))?;
    Some(SectionIntersectionCarrier {
        geometry: SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(center_u, center_v),
            radius: Length(radius),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::TAU),
        },
        line_is_bounded: false,
    })
}

fn trim_segment_id(
    definition: &crate::feature::FeatureDefinition,
    row: &crate::feature::FeatureTrimEntity,
) -> Option<u32> {
    let Some(segment_table) = &definition.segments else {
        return Some(row.external_id);
    };
    let segments = &segment_table.rows;
    let trim_rows = &definition.trim_entities.as_ref()?.rows;
    if segments
        .iter()
        .any(|segment| segment.external_id == row.external_id)
    {
        return Some(row.external_id);
    }
    let unmatched_segments = segments
        .iter()
        .filter(|segment| {
            !trim_rows
                .iter()
                .any(|trim| trim.external_id == segment.external_id)
        })
        .map(|segment| segment.external_id)
        .collect::<Vec<_>>();
    let unmatched_rows = trim_rows
        .iter()
        .filter(|trim| {
            !segments
                .iter()
                .any(|segment| segment.external_id == trim.external_id)
        })
        .collect::<Vec<_>>();
    match (unmatched_segments.as_slice(), unmatched_rows.as_slice()) {
        ([segment_id], [unmatched]) if std::ptr::eq(*unmatched, row) => Some(*segment_id),
        _ => None,
    }
}

fn intersect_section_lines(first: &SketchGeometry, second: &SketchGeometry) -> Option<[f64; 2]> {
    let (
        SketchGeometry::Line {
            start: first_start,
            end: first_end,
        },
        SketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    ) = (first, second)
    else {
        return None;
    };
    let denominator = (first_start.u - first_end.u).mul_add(
        second_start.v - second_end.v,
        -(first_start.v - first_end.v) * (second_start.u - second_end.u),
    );
    let scale = (first_start.u - first_end.u)
        .abs()
        .max((first_start.v - first_end.v).abs())
        .max((second_start.u - second_end.u).abs())
        .max((second_start.v - second_end.v).abs())
        .max(1.0);
    if denominator.abs() <= 1e-12 * scale * scale {
        return None;
    }
    let first_cross = first_start
        .u
        .mul_add(first_end.v, -(first_start.v * first_end.u));
    let second_cross = second_start
        .u
        .mul_add(second_end.v, -(second_start.v * second_end.u));
    Some([
        first_cross.mul_add(
            second_start.u - second_end.u,
            -(first_start.u - first_end.u) * second_cross,
        ) / denominator,
        first_cross.mul_add(
            second_start.v - second_end.v,
            -(first_start.v - first_end.v) * second_cross,
        ) / denominator,
    ])
}

fn intersect_section_line_arc(first: &SketchGeometry, second: &SketchGeometry) -> Option<[f64; 2]> {
    let (
        (line @ SketchGeometry::Line { .. }, arc @ SketchGeometry::Arc { .. })
        | (arc @ SketchGeometry::Arc { .. }, line @ SketchGeometry::Line { .. }),
    ) = ((first, second),)
    else {
        return None;
    };
    let SketchGeometry::Line { start, end } = line else {
        return None;
    };
    let SketchGeometry::Arc { center, radius, .. } = arc else {
        return None;
    };
    let direction = [end.u - start.u, end.v - start.v];
    let length = direction[0].hypot(direction[1]);
    if length <= 1e-12 || radius.0 <= 1e-12 {
        return None;
    }
    let direction = direction.map(|value| value / length);
    let relative = [start.u - center.u, start.v - center.v];
    let projection = -(relative[0] * direction[0] + relative[1] * direction[1]);
    let closest = [
        start.u + projection * direction[0],
        start.v + projection * direction[1],
    ];
    let distance_squared = (closest[0] - center.u).mul_add(
        closest[0] - center.u,
        (closest[1] - center.v) * (closest[1] - center.v),
    );
    let radial_squared = radius.0 * radius.0;
    let scale = radial_squared.max(1.0);
    if distance_squared > radial_squared + 1e-10 * scale {
        return None;
    }
    let travel = (radial_squared - distance_squared).max(0.0).sqrt();
    let candidates = [
        [
            closest[0] + travel * direction[0],
            closest[1] + travel * direction[1],
        ],
        [
            closest[0] - travel * direction[0],
            closest[1] - travel * direction[1],
        ],
    ];
    if travel <= 1e-10 * radius.0.max(1.0) {
        return Some(candidates[0]);
    }
    let endpoint_distance = |candidate: [f64; 2]| {
        [start, end]
            .iter()
            .map(|endpoint| (candidate[0] - endpoint.u).hypot(candidate[1] - endpoint.v))
            .fold(f64::INFINITY, f64::min)
    };
    let distances = candidates.map(endpoint_distance);
    let distance_scale = distances[0].max(distances[1]).max(1.0);
    if (distances[0] - distances[1]).abs() <= 1e-9 * distance_scale {
        return None;
    }
    Some(candidates[usize::from(distances[1] < distances[0])])
}

fn resolved_trim_vertex_coordinates(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
) -> BTreeMap<u32, [f64; 2]> {
    let Some(segments) = &definition.segments else {
        return BTreeMap::new();
    };
    let mut coordinates = definition
        .trim_vertices
        .iter()
        .flat_map(|table| &table.rows)
        .filter_map(|vertex| Some((vertex.vertex_id, vertex.section_coordinates?)))
        .collect::<BTreeMap<_, _>>();
    for trim in definition
        .trim_entities
        .iter()
        .flat_map(|table| &table.rows)
    {
        let Some(external_id) = trim_segment_id(definition, trim) else {
            continue;
        };
        let Some(segment) = segments
            .rows
            .iter()
            .find(|segment| segment.external_id == external_id)
        else {
            continue;
        };
        let Some(([center_u, center_v], radius)) = saved_section_arc_carrier(definition, segment)
        else {
            continue;
        };
        let Some(arc) = saved_section_arc_record(definition, segment) else {
            continue;
        };
        for (vertex, endpoint) in trim.vertices.into_iter().zip(arc.endpoints) {
            let [Some(u), Some(v), _] = endpoint else {
                continue;
            };
            let candidate = [u, v];
            let candidate_radius = (u - center_u).hypot(v - center_v);
            let radial_scale = radius.max(candidate_radius).max(1.0);
            if (candidate_radius - radius).abs() > 1e-9 * radial_scale {
                continue;
            }
            let compatible = coordinates.get(&vertex).is_none_or(|stored| {
                let scale = stored
                    .iter()
                    .chain(&candidate)
                    .map(|value| value.abs())
                    .fold(1.0, f64::max);
                (stored[0] - candidate[0]).hypot(stored[1] - candidate[1]) <= 1e-9 * scale
            });
            if compatible {
                coordinates.entry(vertex).or_insert(candidate);
            }
        }
    }
    let mut incident = BTreeMap::<u32, Vec<u32>>::new();
    for entity in definition
        .trim_entities
        .iter()
        .flat_map(|table| &table.rows)
    {
        let Some(external_id) = trim_segment_id(definition, entity) else {
            continue;
        };
        for vertex in entity.vertices {
            incident.entry(vertex).or_default().push(external_id);
        }
    }
    for (vertex, entities) in incident {
        if coordinates.contains_key(&vertex) {
            continue;
        }
        let [first_id, second_id] = entities.as_slice() else {
            continue;
        };
        let geometry = |external_id| {
            let segment = segments
                .rows
                .iter()
                .find(|segment| segment.external_id == external_id)?;
            section_segment_intersection_carrier(definition, points, segment)
        };
        let (Some(first), Some(second)) = (geometry(*first_id), geometry(*second_id)) else {
            continue;
        };
        let line_arc_is_bounded = match (&first.geometry, &second.geometry) {
            (SketchGeometry::Line { .. }, SketchGeometry::Arc { .. }) => first.line_is_bounded,
            (SketchGeometry::Arc { .. }, SketchGeometry::Line { .. }) => second.line_is_bounded,
            _ => false,
        };
        if let Some(coordinate) = intersect_section_lines(&first.geometry, &second.geometry)
            .or_else(|| {
                line_arc_is_bounded
                    .then(|| intersect_section_line_arc(&first.geometry, &second.geometry))
                    .flatten()
            })
        {
            coordinates.insert(vertex, coordinate);
        }
    }
    loop {
        let mut additions = Vec::new();
        for trim in definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
        {
            let Some(external_id) = trim_segment_id(definition, trim) else {
                continue;
            };
            let Some(segment) = segments
                .rows
                .iter()
                .find(|segment| segment.external_id == external_id)
            else {
                continue;
            };
            let Some(SketchGeometry::Line { start, end }) =
                resolved_section_segment_geometry(definition, points, segment)
            else {
                continue;
            };
            let stored = [[start.u, start.v], [end.u, end.v]];
            let known = trim
                .vertices
                .map(|vertex| coordinates.get(&vertex).copied());
            let (known_point, missing_index) = match known {
                [Some(point), None] => (point, 1),
                [None, Some(point)] => (point, 0),
                _ => continue,
            };
            let distances =
                stored.map(|point| (point[0] - known_point[0]).hypot(point[1] - known_point[1]));
            let scale = stored
                .iter()
                .flatten()
                .map(|value| value.abs())
                .fold(1.0, f64::max);
            let matched = if distances[0] <= 1e-9 * scale && distances[1] > 1e-9 * scale {
                0
            } else if distances[1] <= 1e-9 * scale && distances[0] > 1e-9 * scale {
                1
            } else {
                continue;
            };
            additions.push((trim.vertices[missing_index], stored[1 - matched]));
        }
        let mut changed = false;
        for (vertex, coordinate) in additions {
            if let std::collections::btree_map::Entry::Vacant(entry) = coordinates.entry(vertex) {
                entry.insert(coordinate);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    coordinates
}

fn trimmed_section_segment_geometry(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    trim_vertices: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SketchGeometry> {
    let trim = definition
        .trim_entities
        .as_ref()?
        .rows
        .iter()
        .find(|row| trim_segment_id(definition, row) == Some(segment.external_id))?;
    let start = trim_vertices.get(&trim.vertices[0])?;
    let end = trim_vertices.get(&trim.vertices[1])?;
    if let Some(geometry) = resolved_section_segment_geometry(definition, points, segment) {
        if !matches!(geometry, SketchGeometry::Line { .. }) {
            return Some(geometry);
        }
    } else if let Some(([center_u, center_v], radius)) =
        section_arc_carrier(definition, points, segment)
            .or_else(|| saved_section_arc_carrier(definition, segment))
    {
        let first = [start[0] - center_u, start[1] - center_v];
        let second = [end[0] - center_u, end[1] - center_v];
        let first_radius = first[0].hypot(first[1]);
        let second_radius = second[0].hypot(second[1]);
        let scale = radius.max(first_radius).max(second_radius).max(1.0);
        if (first_radius - radius).abs() > 1e-9 * scale
            || (second_radius - radius).abs() > 1e-9 * scale
        {
            return None;
        }
        let start_angle = second[1].atan2(second[0]);
        let mut end_angle = first[1].atan2(first[0]);
        while end_angle <= start_angle {
            end_angle += std::f64::consts::TAU;
        }
        return Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(center_u, center_v),
            radius: Length(radius),
            start_angle: Angle(start_angle),
            end_angle: Angle(end_angle),
        });
    } else {
        let scale = start
            .iter()
            .chain(end)
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        let orientation_matches = match segment.vertical_horizontal {
            Some(0) => (start[0] - end[0]).abs() <= 1e-9 * scale,
            Some(1) => (start[1] - end[1]).abs() <= 1e-9 * scale,
            _ => false,
        };
        orientation_matches.then_some(())?;
    }
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
}

fn section_point_in_model(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        transform.origin[axis]
            + point[0] * transform.u_axis[axis]
            + point[1] * transform.v_axis[axis]
    })
}

fn section_xyz_in_model(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 3],
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        transform.origin[axis]
            + point[0] * transform.u_axis[axis]
            + point[1] * transform.v_axis[axis]
            + point[2] * transform.normal[axis]
    })
}

fn normalized(vector: [f64; 3]) -> Option<[f64; 3]> {
    let magnitude = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    (magnitude > 1e-12).then(|| vector.map(|value| value / magnitude))
}

#[cfg(test)]
fn extruded_segment_surface(
    transform: &crate::placement::FeatureSectionTransform,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SurfaceGeometry> {
    extruded_geometry_surface(transform, &section_segment_geometry(points, segment)?)
}

fn extruded_geometry_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
) -> Option<SurfaceGeometry> {
    match geometry {
        SketchGeometry::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let line = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            let normal = normalized(cross(line, transform.normal))?;
            Some(SurfaceGeometry::Plane {
                origin: Point3::new(start[0], start[1], start[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(line[0], line[1], line[2]),
            })
        }
        SketchGeometry::Arc { center, radius, .. } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            Some(SurfaceGeometry::Cylinder {
                origin: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(
                    transform.normal[0],
                    transform.normal[1],
                    transform.normal[2],
                ),
                ref_direction: Vector3::new(
                    transform.u_axis[0],
                    transform.u_axis[1],
                    transform.u_axis[2],
                ),
                radius: radius.0,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
fn placed_section_curve_geometry(
    transform: &crate::placement::FeatureSectionTransform,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<CurveGeometry> {
    placed_section_geometry_curve(transform, &section_segment_geometry(points, segment)?)
}

fn placed_section_geometry_curve(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
) -> Option<CurveGeometry> {
    match geometry {
        SketchGeometry::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let direction = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            Some(CurveGeometry::Line {
                origin: Point3::new(start[0], start[1], start[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SketchGeometry::Arc { center, radius, .. } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            Some(CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(
                    transform.normal[0],
                    transform.normal[1],
                    transform.normal[2],
                ),
                ref_direction: Vector3::new(
                    transform.u_axis[0],
                    transform.u_axis[1],
                    transform.u_axis[2],
                ),
                radius: radius.0,
            })
        }
        _ => None,
    }
}

fn bspline_basis(index: usize, degree: usize, parameter: f64, knots: &[f64], count: usize) -> f64 {
    if parameter == *knots.last().expect("nonempty knots") {
        return if index + 1 == count { 1.0 } else { 0.0 };
    }
    if degree == 0 {
        return if knots[index] <= parameter && parameter < knots[index + 1] {
            1.0
        } else {
            0.0
        };
    }
    let left_denominator = knots[index + degree] - knots[index];
    let right_denominator = knots[index + degree + 1] - knots[index + 1];
    let left = if left_denominator > 0.0 {
        (parameter - knots[index]) / left_denominator
            * bspline_basis(index, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        (knots[index + degree + 1] - parameter) / right_denominator
            * bspline_basis(index + 1, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    left + right
}

fn bspline_basis_derivative(
    index: usize,
    degree: usize,
    parameter: f64,
    knots: &[f64],
    count: usize,
) -> f64 {
    let left_denominator = knots[index + degree] - knots[index];
    let right_denominator = knots[index + degree + 1] - knots[index + 1];
    let left = if left_denominator > 0.0 {
        degree as f64 / left_denominator * bspline_basis(index, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        degree as f64 / right_denominator
            * bspline_basis(index + 1, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    left - right
}

fn solve_vector_system(
    mut matrix: Vec<Vec<f64>>,
    mut values: Vec<[f64; 3]>,
) -> Option<Vec<[f64; 3]>> {
    let count = matrix.len();
    (values.len() == count && matrix.iter().all(|row| row.len() == count)).then_some(())?;
    for column in 0..count {
        let pivot = (column..count).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        (matrix[pivot][column].abs() > 1e-14).then_some(())?;
        matrix.swap(column, pivot);
        values.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= scale;
        }
        values[column] = values[column].map(|value| value / scale);
        let pivot_row = matrix[column].clone();
        let pivot_value = values[column];
        for row in 0..count {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            if factor == 0.0 {
                continue;
            }
            for (entry, pivot_entry) in matrix[row][column..].iter_mut().zip(&pivot_row[column..]) {
                *entry -= factor * pivot_entry;
            }
            for (value, pivot) in values[row].iter_mut().zip(pivot_value) {
                *value -= factor * pivot;
            }
        }
    }
    Some(values)
}

fn saved_spline_nurbs(spline: &crate::feature::FeatureSavedSpline) -> Option<NurbsCurve> {
    const DEGREE: usize = 3;
    let parameters = spline.parameters.as_ref()?;
    let tangents = spline.endpoint_tangents?;
    let point_count = spline.interpolation_points.len();
    (point_count >= 2 && parameters.len() == point_count).then_some(())?;
    parameters
        .windows(2)
        .all(|pair| pair[0].is_finite() && pair[0] < pair[1])
        .then_some(())?;
    parameters.last()?.is_finite().then_some(())?;
    let control_count = point_count + 2;
    let mut knots = vec![parameters[0]; DEGREE + 1];
    knots.extend_from_slice(&parameters[1..point_count - 1]);
    knots.extend(std::iter::repeat_n(parameters[point_count - 1], DEGREE + 1));
    let mut matrix = Vec::with_capacity(control_count);
    for parameter in parameters {
        matrix.push(
            (0..control_count)
                .map(|index| bspline_basis(index, DEGREE, *parameter, &knots, control_count))
                .collect(),
        );
    }
    for parameter in [parameters[0], parameters[point_count - 1]] {
        matrix.push(
            (0..control_count)
                .map(|index| {
                    bspline_basis_derivative(index, DEGREE, parameter, &knots, control_count)
                })
                .collect(),
        );
    }
    let mut values = spline.interpolation_points.clone();
    values.extend(tangents);
    let control_points = solve_vector_system(matrix, values)?
        .into_iter()
        .map(|point| Point3::new(point[0], point[1], point[2]))
        .collect();
    Some(NurbsCurve {
        degree: DEGREE as u32,
        knots,
        control_points,
        weights: None,
        periodic: false,
    })
}

fn revolved_nurbs_surface(directrix: &NurbsCurve, axis: RevolutionAxis) -> Option<NurbsSurface> {
    let axis_direction = normalized([axis.direction.x, axis.direction.y, axis.direction.z])?;
    let axis_origin = [axis.origin.x, axis.origin.y, axis.origin.z];
    let angular_poles = [
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [-1.0, 1.0],
        [-1.0, 0.0],
        [-1.0, -1.0],
        [0.0, -1.0],
        [1.0, -1.0],
        [1.0, 0.0],
    ];
    let diagonal_weight = std::f64::consts::FRAC_1_SQRT_2;
    let angular_weights = [
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
    ];
    let mut control_points = Vec::with_capacity(directrix.control_points.len() * 9);
    let mut weights = Vec::with_capacity(directrix.control_points.len() * 9);
    for (index, point) in directrix.control_points.iter().enumerate() {
        let relative = [
            point.x - axis_origin[0],
            point.y - axis_origin[1],
            point.z - axis_origin[2],
        ];
        let axial_distance = dot(relative, axis_direction);
        let center: [f64; 3] = std::array::from_fn(|component| {
            axis_origin[component] + axial_distance * axis_direction[component]
        });
        let radial = [
            point.x - center[0],
            point.y - center[1],
            point.z - center[2],
        ];
        let tangent = cross(axis_direction, radial);
        let directrix_weight = directrix
            .weights
            .as_ref()
            .map_or(1.0, |curve_weights| curve_weights[index]);
        for ([radial_scale, tangent_scale], angular_weight) in
            angular_poles.into_iter().zip(angular_weights)
        {
            control_points.push(Point3::new(
                center[0] + radial_scale * radial[0] + tangent_scale * tangent[0],
                center[1] + radial_scale * radial[1] + tangent_scale * tangent[1],
                center[2] + radial_scale * radial[2] + tangent_scale * tangent[2],
            ));
            weights.push(directrix_weight * angular_weight);
        }
    }
    Some(NurbsSurface {
        u_degree: directrix.degree,
        v_degree: 2,
        u_knots: directrix.knots.clone(),
        v_knots: vec![
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2,
            std::f64::consts::TAU,
            std::f64::consts::TAU,
            std::f64::consts::TAU,
        ],
        u_count: u32::try_from(directrix.control_points.len()).ok()?,
        v_count: 9,
        control_points,
        weights: Some(weights),
        u_periodic: false,
        v_periodic: false,
    })
}

fn transfer_feature_extrusion_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.feature_section_transforms {
        let Some(definition) = scan
            .feature_definitions
            .iter()
            .find(|definition| definition.id == transform.definition_id)
        else {
            continue;
        };
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Extrude) {
            continue;
        }
        let (Some(_), Some(segments), Some(order_table), Some(trim_entities)) = (
            &definition.variables,
            &definition.segments,
            &definition.order_table,
            &definition.trim_entities,
        ) else {
            continue;
        };
        let tables = scan
            .feature_entity_tables
            .iter()
            .filter(|table| table.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        let [table] = tables.as_slice() else {
            continue;
        };
        let points = resolved_section_points(definition);
        let solved = trim_entities
            .rows
            .iter()
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        let cylinder_entries = table
            .entry_ids
            .iter()
            .filter(|id| {
                scan.surface_rows
                    .iter()
                    .any(|row| row.id == **id && row.kind == crate::surface::SurfaceKind::Cylinder)
            })
            .copied()
            .collect::<Vec<_>>();
        let mut arcs = segments
            .rows
            .iter()
            .filter(|segment| {
                solved.contains(&segment.external_id)
                    && segment.kind == crate::feature::FeatureSegmentKind::Arc
            })
            .filter_map(|segment| Some((order_table.internal_id(segment.external_id)?, segment)))
            .collect::<Vec<_>>();
        arcs.sort_by_key(|(internal_id, _)| *internal_id);
        let arc_bindings = if arcs.len() == cylinder_entries.len() {
            arcs.iter()
                .zip(&cylinder_entries)
                .map(|((_, segment), surface_id)| (segment.external_id, *surface_id))
                .collect::<BTreeMap<_, _>>()
        } else {
            BTreeMap::new()
        };

        for segment in segments
            .rows
            .iter()
            .filter(|segment| solved.contains(&segment.external_id))
        {
            let surface_id = match segment.kind {
                crate::feature::FeatureSegmentKind::Line => order_table
                    .internal_id(segment.external_id)
                    .and_then(|internal_id| internal_id.checked_sub(1))
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| table.entry_ids.get(index))
                    .copied()
                    .filter(|id| table.surface_ids.contains(id)),
                crate::feature::FeatureSegmentKind::Arc => {
                    arc_bindings.get(&segment.external_id).copied()
                }
                crate::feature::FeatureSegmentKind::Point => None,
            };
            let Some(surface_id) = surface_id else {
                continue;
            };
            let id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            let Some(section_geometry) =
                resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
            annotate(
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "protextrude_section_carrier",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{surface_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

fn sketch_geometry_endpoints(geometry: &SketchGeometry) -> Option<([f64; 2], [f64; 2])> {
    match geometry {
        SketchGeometry::Line { start, end } => Some(([start.u, start.v], [end.u, end.v])),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some((
            [
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ],
            [
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ],
        )),
        _ => None,
    }
}

fn oriented_arc_parameterization(reversed: bool, start: f64, end: f64) -> (f64, [f64; 2]) {
    let (axis_sign, raw_start, raw_end) = if reversed {
        (-1.0, -end, -start)
    } else {
        (1.0, start, end)
    };
    let start = raw_start.rem_euclid(std::f64::consts::TAU);
    let mut end = raw_end.rem_euclid(std::f64::consts::TAU);
    if end < start {
        end += std::f64::consts::TAU;
    }
    (axis_sign, [start, end])
}

fn transfer_resolved_extrusion_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.feature_section_transforms {
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Extrude) {
            continue;
        }
        let Some(operation_index) = scan
            .feature_operations
            .iter()
            .position(|operation| operation.feature_id == feature_id)
        else {
            continue;
        };
        if scan.feature_operations[..operation_index]
            .iter()
            .any(|operation| operation.recipe.is_some())
        {
            continue;
        }
        let sketch_id = SketchId(format!("creo:model:sketch#{}", transform.definition_id));
        let Some(sketch) = ir
            .model
            .sketches
            .iter()
            .find(|sketch| sketch.id == sketch_id)
        else {
            continue;
        };
        if sketch.profiles.is_empty() {
            continue;
        }
        let cap_origins = scan
            .surface_rows
            .iter()
            .filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
            })
            .filter_map(|row| {
                let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
                ir.model.surfaces.iter().find(|surface| surface.id == id)
            })
            .filter_map(|surface| match surface.geometry {
                SurfaceGeometry::Plane { origin, normal, .. } => Some((
                    [origin.x, origin.y, origin.z],
                    [normal.x, normal.y, normal.z],
                )),
                _ => None,
            });
        let Some(span) = extrusion_span(transform.origin, transform.normal, cap_origins) else {
            continue;
        };
        let length = span.upper - span.lower;
        let entities = ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch_id)
            .map(|entity| (entity.id.clone(), entity))
            .collect::<BTreeMap<_, _>>();
        let mut profiles = Vec::new();
        let mut supported = true;
        for profile in &sketch.profiles {
            let mut geometries = Vec::new();
            for entity_use in profile {
                let Some(entity) = entities.get(&entity_use.entity) else {
                    supported = false;
                    break;
                };
                let Some((mut start, mut end)) = sketch_geometry_endpoints(&entity.geometry) else {
                    supported = false;
                    break;
                };
                if entity_use.reversed {
                    std::mem::swap(&mut start, &mut end);
                }
                geometries.push((entity.geometry.clone(), entity_use.reversed, start, end));
            }
            if !supported || geometries.len() < 2 {
                break;
            }
            let scale = geometries
                .iter()
                .flat_map(|(_, _, start, end)| start.iter().chain(end))
                .map(|value| value.abs())
                .fold(1.0, f64::max);
            if geometries
                .iter()
                .enumerate()
                .any(|(index, (_, _, _, end))| {
                    let next = geometries[(index + 1) % geometries.len()].2;
                    (end[0] - next[0]).hypot(end[1] - next[1]) > 1e-9 * scale
                })
            {
                supported = false;
                break;
            }
            profiles.push(geometries);
        }
        if !supported {
            continue;
        }

        let prefix = format!("creo:feature:extrusion#{feature_id}");
        let body_id = BodyId(format!("{prefix}:body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId(format!("{prefix}:region"));
        let shell_id = ShellId(format!("{prefix}:shell"));
        let bottom_surface = SurfaceId(format!("{prefix}:surface:bottom"));
        let top_surface = SurfaceId(format!("{prefix}:surface:top"));
        for (id, offset) in [(&bottom_surface, span.lower), (&top_surface, span.upper)] {
            annotate(
                annotations,
                id,
                "FeatDefs",
                transform.offset as u64,
                "extrusion_cap_plane",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(
                        transform.origin[0] + offset * transform.normal[0],
                        transform.origin[1] + offset * transform.normal[1],
                        transform.origin[2] + offset * transform.normal[2],
                    ),
                    normal: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    u_axis: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                },
                source_object: None,
            });
        }

        let bottom_face = FaceId(format!("{prefix}:face:bottom"));
        let top_face = FaceId(format!("{prefix}:face:top"));
        let mut shell_faces = vec![bottom_face.clone(), top_face.clone()];
        let mut bottom_loops = Vec::new();
        let mut top_loops = Vec::new();
        for (profile_index, profile) in profiles.iter().enumerate() {
            let count = profile.len();
            let mut bottom_vertices = Vec::new();
            let mut top_vertices = Vec::new();
            for (index, (_, _, start, _)) in profile.iter().enumerate() {
                for (side, offset, arena) in [
                    ("bottom", span.lower, &mut bottom_vertices),
                    ("top", span.upper, &mut top_vertices),
                ] {
                    let position = section_point_in_model(transform, *start);
                    let point_id =
                        PointId(format!("{prefix}:point:{profile_index}:{index}:{side}"));
                    let vertex_id =
                        VertexId(format!("{prefix}:vertex:{profile_index}:{index}:{side}"));
                    ir.model.points.push(Point {
                        id: point_id.clone(),
                        position: Point3::new(
                            position[0] + offset * transform.normal[0],
                            position[1] + offset * transform.normal[1],
                            position[2] + offset * transform.normal[2],
                        ),
                    });
                    ir.model.vertices.push(Vertex {
                        id: vertex_id.clone(),
                        point: point_id,
                        tolerance: None,
                    });
                    arena.push(vertex_id);
                }
            }

            let mut bottom_edges = Vec::new();
            let mut top_edges = Vec::new();
            let mut vertical_edges = Vec::new();
            for (index, (geometry, reversed, start, end)) in profile.iter().enumerate() {
                let next = (index + 1) % count;
                for (side, offset, vertices, arena) in [
                    ("bottom", span.lower, &bottom_vertices, &mut bottom_edges),
                    ("top", span.upper, &top_vertices, &mut top_edges),
                ] {
                    let curve_id =
                        CurveId(format!("{prefix}:curve:{profile_index}:{index}:{side}"));
                    let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{index}:{side}"));
                    let curve = match geometry {
                        SketchGeometry::Line { .. } => {
                            let placed_start = section_point_in_model(transform, *start);
                            let placed_end = section_point_in_model(transform, *end);
                            let direction = normalized(std::array::from_fn(|axis| {
                                placed_end[axis] - placed_start[axis]
                            }))
                            .expect("closed line segment is nondegenerate");
                            CurveGeometry::Line {
                                origin: Point3::new(
                                    placed_start[0] + offset * transform.normal[0],
                                    placed_start[1] + offset * transform.normal[1],
                                    placed_start[2] + offset * transform.normal[2],
                                ),
                                direction: Vector3::new(direction[0], direction[1], direction[2]),
                            }
                        }
                        SketchGeometry::Arc { center, radius, .. } => {
                            let center = section_point_in_model(transform, [center.u, center.v]);
                            let (axis_sign, _) = oriented_arc_parameterization(*reversed, 0.0, 0.0);
                            CurveGeometry::Circle {
                                center: Point3::new(
                                    center[0] + offset * transform.normal[0],
                                    center[1] + offset * transform.normal[1],
                                    center[2] + offset * transform.normal[2],
                                ),
                                axis: Vector3::new(
                                    axis_sign * transform.normal[0],
                                    axis_sign * transform.normal[1],
                                    axis_sign * transform.normal[2],
                                ),
                                ref_direction: Vector3::new(
                                    transform.u_axis[0],
                                    transform.u_axis[1],
                                    transform.u_axis[2],
                                ),
                                radius: radius.0,
                            }
                        }
                        _ => unreachable!("profile family checked above"),
                    };
                    ir.model.curves.push(Curve {
                        id: curve_id.clone(),
                        geometry: curve,
                        source_object: None,
                    });
                    let param_range = match geometry {
                        SketchGeometry::Line { .. } => {
                            Some([0.0, (end[0] - start[0]).hypot(end[1] - start[1])])
                        }
                        SketchGeometry::Arc {
                            start_angle,
                            end_angle,
                            ..
                        } => Some(
                            oriented_arc_parameterization(*reversed, start_angle.0, end_angle.0).1,
                        ),
                        _ => None,
                    };
                    ir.model.edges.push(Edge {
                        id: edge_id.clone(),
                        curve: Some(curve_id),
                        start: vertices[index].clone(),
                        end: vertices[next].clone(),
                        param_range,
                        tolerance: None,
                    });
                    arena.push(edge_id);
                }
                let curve_id = CurveId(format!("{prefix}:curve:{profile_index}:{index}:vertical"));
                let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{index}:vertical"));
                let origin = section_point_in_model(transform, *start);
                ir.model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Line {
                        origin: Point3::new(
                            origin[0] + span.lower * transform.normal[0],
                            origin[1] + span.lower * transform.normal[1],
                            origin[2] + span.lower * transform.normal[2],
                        ),
                        direction: Vector3::new(
                            transform.normal[0],
                            transform.normal[1],
                            transform.normal[2],
                        ),
                    },
                    source_object: None,
                });
                ir.model.edges.push(Edge {
                    id: edge_id.clone(),
                    curve: Some(curve_id),
                    start: bottom_vertices[index].clone(),
                    end: top_vertices[index].clone(),
                    param_range: Some([0.0, length]),
                    tolerance: None,
                });
                vertical_edges.push(edge_id);
            }

            let bottom_loop = LoopId(format!("{prefix}:loop:{profile_index}:bottom"));
            let top_loop = LoopId(format!("{prefix}:loop:{profile_index}:top"));
            bottom_loops.push(bottom_loop.clone());
            top_loops.push(top_loop.clone());
            let bottom_coedges = (0..count)
                .rev()
                .map(|index| {
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:bottom-cap"
                    ))
                })
                .collect::<Vec<_>>();
            let top_coedges = (0..count)
                .map(|index| CoedgeId(format!("{prefix}:coedge:{profile_index}:{index}:top-cap")))
                .collect::<Vec<_>>();
            ir.model.loops.push(IrLoop {
                id: bottom_loop.clone(),
                face: bottom_face.clone(),
                coedges: bottom_coedges.clone(),
            });
            ir.model.loops.push(IrLoop {
                id: top_loop.clone(),
                face: top_face.clone(),
                coedges: top_coedges.clone(),
            });
            for ring_index in 0..count {
                let edge_index = count - 1 - ring_index;
                let id = bottom_coedges[ring_index].clone();
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: bottom_loop.clone(),
                    edge: bottom_edges[edge_index].clone(),
                    next: bottom_coedges[(ring_index + 1) % count].clone(),
                    previous: bottom_coedges[(ring_index + count - 1) % count].clone(),
                    radial_next: CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{edge_index}:side-bottom"
                    )),
                    sense: Sense::Reversed,
                    pcurve: None,
                });
                let id = top_coedges[ring_index].clone();
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: top_loop.clone(),
                    edge: top_edges[ring_index].clone(),
                    next: top_coedges[(ring_index + 1) % count].clone(),
                    previous: top_coedges[(ring_index + count - 1) % count].clone(),
                    radial_next: CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{ring_index}:side-top"
                    )),
                    sense: Sense::Forward,
                    pcurve: None,
                });
            }

            for (index, (geometry, _, start, _)) in profile.iter().enumerate() {
                let next = (index + 1) % count;
                let surface_id =
                    SurfaceId(format!("{prefix}:surface:{profile_index}:side:{index}"));
                let section_geometry = match geometry {
                    SketchGeometry::Line { .. } => SketchGeometry::Line {
                        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
                        end: cadmpeg_ir::math::Point2::new(
                            profile[index].3[0],
                            profile[index].3[1],
                        ),
                    },
                    value => value.clone(),
                };
                let Some(surface_geometry) =
                    extruded_geometry_surface(transform, &section_geometry)
                else {
                    supported = false;
                    break;
                };
                ir.model.surfaces.push(Surface {
                    id: surface_id.clone(),
                    geometry: surface_geometry,
                    source_object: None,
                });
                let face_id = FaceId(format!("{prefix}:face:{profile_index}:side:{index}"));
                let loop_id = LoopId(format!("{prefix}:loop:{profile_index}:side:{index}"));
                let coedges = [
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:side-bottom"
                    )),
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{next}:side-vertical-out"
                    )),
                    CoedgeId(format!("{prefix}:coedge:{profile_index}:{index}:side-top")),
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:side-vertical-in"
                    )),
                ];
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face_id.clone(),
                    coedges: coedges.to_vec(),
                });
                let edge_uses = [
                    (bottom_edges[index].clone(), Sense::Forward),
                    (vertical_edges[next].clone(), Sense::Forward),
                    (top_edges[index].clone(), Sense::Reversed),
                    (vertical_edges[index].clone(), Sense::Reversed),
                ];
                for use_index in 0..4 {
                    let radial_next = match use_index {
                        0 => bottom_coedges[count - 1 - index].clone(),
                        1 => CoedgeId(format!(
                            "{prefix}:coedge:{profile_index}:{next}:side-vertical-in"
                        )),
                        2 => top_coedges[index].clone(),
                        3 => CoedgeId(format!(
                            "{prefix}:coedge:{profile_index}:{index}:side-vertical-out"
                        )),
                        _ => unreachable!(),
                    };
                    ir.model.coedges.push(Coedge {
                        id: coedges[use_index].clone(),
                        owner_loop: loop_id.clone(),
                        edge: edge_uses[use_index].0.clone(),
                        next: coedges[(use_index + 1) % 4].clone(),
                        previous: coedges[(use_index + 3) % 4].clone(),
                        radial_next,
                        sense: edge_uses[use_index].1,
                        pcurve: None,
                    });
                }
                ir.model.faces.push(Face {
                    id: face_id.clone(),
                    shell: shell_id.clone(),
                    surface: surface_id,
                    sense: Sense::Forward,
                    loops: vec![loop_id],
                    name: None,
                    color: None,
                    tolerance: None,
                });
                shell_faces.push(face_id);
            }
        }
        if !supported {
            continue;
        }
        ir.model.faces.push(Face {
            id: bottom_face,
            shell: shell_id.clone(),
            surface: bottom_surface,
            sense: Sense::Reversed,
            loops: bottom_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.faces.push(Face {
            id: top_face,
            shell: shell_id.clone(),
            surface: top_surface,
            sense: Sense::Forward,
            loops: top_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: shell_faces,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Solid,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        transferred += 1;
    }
    transferred
}

fn feature_recipe(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeKind> {
    let schema_recipe = scan
        .feature_rows
        .iter()
        .find(|row| row.feature_id == feature_id)
        .and_then(|row| match row.root_schema_class {
            Some(916 | 917) => Some(crate::feature::FeatureRecipeKind::Extrude),
            _ => None,
        });
    schema_recipe.or_else(|| {
        scan.feature_operations
            .iter()
            .find(|operation| operation.feature_id == feature_id)
            .and_then(|operation| operation.recipe)
    })
}

fn line_orientation_definition(
    segment: &crate::feature::FeatureSegment,
    entity: SketchEntityId,
) -> Option<SketchConstraintDefinition> {
    if segment.kind != crate::feature::FeatureSegmentKind::Line {
        return None;
    }
    match segment.vertical_horizontal {
        Some(0) => Some(SketchConstraintDefinition::Vertical { entity }),
        Some(1) => Some(SketchConstraintDefinition::Horizontal { entity }),
        _ => None,
    }
}

fn resolved_profile_chains(
    definition: &crate::feature::FeatureDefinition,
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = &definition.trim_entities else {
        return Vec::new();
    };
    let rows = table
        .rows
        .iter()
        .filter_map(|row| Some((row, trim_segment_id(definition, row)?)))
        .collect::<Vec<_>>();
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, row) in rows.iter().enumerate() {
        for vertex in row.0.vertices {
            incident.entry(vertex).or_default().push(index);
        }
    }
    if incident.values().any(|rows| rows.len() > 2) {
        return Vec::new();
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut frontier = vec![seed];
        while let Some(index) = frontier.pop() {
            for vertex in rows[index].0.vertices {
                for adjacent in &incident[&vertex] {
                    if component.insert(*adjacent) {
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        remaining.retain(|index| !component.contains(index));
        if component
            .iter()
            .any(|index| !emitted.contains(&rows[*index].1))
        {
            continue;
        }
        let endpoints = incident
            .iter()
            .filter(|(_, rows)| rows.iter().filter(|row| component.contains(row)).count() == 1)
            .map(|(vertex, _)| *vertex)
            .collect::<Vec<_>>();
        if !matches!(endpoints.len(), 0 | 2) {
            continue;
        }
        let first_row = component
            .iter()
            .min_by_key(|index| rows[**index].1)
            .copied()
            .expect("component contains seed");
        let mut vertex = endpoints
            .iter()
            .min()
            .copied()
            .unwrap_or(rows[first_row].0.vertices[0]);
        let start_vertex = vertex;
        let mut unused = component;
        let mut profile = Vec::new();
        while !unused.is_empty() {
            let candidates = incident[&vertex]
                .iter()
                .filter(|index| unused.contains(index))
                .copied()
                .collect::<Vec<_>>();
            let index = if profile.is_empty() && endpoints.is_empty() {
                if candidates.contains(&first_row) {
                    first_row
                } else {
                    break;
                }
            } else if candidates.len() == 1 {
                candidates[0]
            } else {
                break;
            };
            let (row, external_id) = rows[index];
            let row_reversed = row.vertices[1] == vertex;
            if !row_reversed && row.vertices[0] != vertex {
                break;
            }
            let arc_orientation_reversed = definition
                .segments
                .iter()
                .flat_map(|table| &table.rows)
                .find(|segment| segment.external_id == external_id)
                .is_some_and(|segment| {
                    segment.kind == crate::feature::FeatureSegmentKind::Arc
                        && segment.arc_orientation == Some(0)
                });
            profile.push(SketchEntityUse {
                entity: SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, external_id
                )),
                reversed: row_reversed ^ arc_orientation_reversed,
            });
            vertex = if row_reversed {
                row.vertices[0]
            } else {
                row.vertices[1]
            };
            unused.remove(&index);
        }
        let terminal_ok = if endpoints.is_empty() {
            vertex == start_vertex
        } else {
            endpoints.contains(&vertex) && vertex != start_vertex
        };
        if unused.is_empty() && terminal_ok {
            profiles.push(profile);
        }
    }
    profiles
}

fn transfer_resolved_sketches(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for transform in &scan.feature_section_transforms {
        let Some(definition) = scan
            .feature_definitions
            .iter()
            .find(|definition| definition.id == transform.definition_id)
        else {
            continue;
        };
        let Some(segments) = &definition.segments else {
            continue;
        };
        let points = resolved_section_points(definition);
        let solved = definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        let trim_vertex_coordinates = resolved_trim_vertex_coordinates(definition, &points);
        let sketch_id = SketchId(format!("creo:model:sketch#{}", definition.id));
        let mut entities = segments
            .rows
            .iter()
            .filter_map(|segment| {
                let geometry = if solved.contains(&segment.external_id) {
                    trimmed_section_segment_geometry(
                        definition,
                        &points,
                        &trim_vertex_coordinates,
                        segment,
                    )?
                } else {
                    resolved_section_segment_geometry(definition, &points, segment)?
                };
                let id = SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, segment.external_id
                ));
                annotate(
                    annotations,
                    &id.0,
                    "FeatDefs",
                    segment.offset as u64,
                    match segment.kind {
                        crate::feature::FeatureSegmentKind::Line => "solved_section_line",
                        crate::feature::FeatureSegmentKind::Arc => "solved_section_arc",
                        crate::feature::FeatureSegmentKind::Point => "solved_section_point",
                    },
                    Exactness::Derived,
                );
                Some(SketchEntity {
                    id,
                    sketch: sketch_id.clone(),
                    construction: !solved.contains(&segment.external_id),
                    native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                    geometry_ref: Some(format!(
                        "creo:featdefs:section_curve#{}:{}",
                        definition.id, segment.external_id
                    )),
                    endpoint_refs: match segment.kind {
                        crate::feature::FeatureSegmentKind::Arc => {
                            vec![segment.point_ids[1], segment.point_ids[0]]
                        }
                        crate::feature::FeatureSegmentKind::Line => segment.point_ids.to_vec(),
                        crate::feature::FeatureSegmentKind::Point => vec![segment.point_ids[0]],
                    }
                    .into_iter()
                    .map(|point| format!("creo:featdefs:sketch#{}:point#{point}", definition.id))
                    .collect(),
                    geometry,
                })
            })
            .collect::<Vec<_>>();
        for spline in definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let Some(nurbs) = saved_spline_nurbs(spline) else {
                continue;
            };
            if nurbs
                .control_points
                .iter()
                .any(|point| point.z.abs() > 1e-12)
            {
                continue;
            }
            let suffix = spline.entity_id.map_or_else(
                || format!("offset{}", spline.offset),
                |entity_id| entity_id.to_string(),
            );
            let entity_id = SketchEntityId(format!(
                "creo:featdefs:saved_spline#{}:{suffix}",
                definition.id
            ));
            let curve_id = CurveId(format!(
                "creo:featdefs:saved_spline_curve#{}:{suffix}",
                definition.id
            ));
            annotate(
                annotations,
                &entity_id.0,
                "FeatDefs",
                spline.offset as u64,
                "saved_interpolation_spline",
                Exactness::Derived,
            );
            annotate(
                annotations,
                &curve_id,
                "FeatDefs",
                spline.offset as u64,
                "placed_saved_interpolation_spline",
                Exactness::Derived,
            );
            entities.push(SketchEntity {
                id: entity_id,
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: Some(format!("creo:featdefs:saved_spline#{suffix}")),
                geometry_ref: Some(curve_id.0.clone()),
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Nurbs {
                    degree: nurbs.degree,
                    knots: nurbs.knots.clone(),
                    control_points: nurbs
                        .control_points
                        .iter()
                        .map(|point| cadmpeg_ir::math::Point2::new(point.x, point.y))
                        .collect(),
                    weights: None,
                    periodic: false,
                },
            });
            ir.model.curves.push(Curve {
                id: curve_id,
                geometry: CurveGeometry::Nurbs(NurbsCurve {
                    degree: nurbs.degree,
                    knots: nurbs.knots,
                    control_points: nurbs
                        .control_points
                        .into_iter()
                        .map(|point| {
                            let placed =
                                section_xyz_in_model(transform, [point.x, point.y, point.z]);
                            Point3::new(placed[0], placed[1], placed[2])
                        })
                        .collect(),
                    weights: None,
                    periodic: false,
                }),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("FeatDefs:saved_spline#{suffix}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        if entities.is_empty() {
            continue;
        }
        for segment in &segments.rows {
            let Some(section_geometry) =
                resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry) else {
                continue;
            };
            let id = CurveId(format!(
                "creo:featdefs:section_curve#{}:{}",
                definition.id, segment.external_id
            ));
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "placed_section_curve",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!(
                        "FeatDefs:section#{}:{}",
                        definition.id, segment.external_id
                    ),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        let emitted = segments
            .rows
            .iter()
            .filter(|segment| {
                if solved.contains(&segment.external_id) {
                    trimmed_section_segment_geometry(
                        definition,
                        &points,
                        &trim_vertex_coordinates,
                        segment,
                    )
                    .is_some()
                } else {
                    resolved_section_segment_geometry(definition, &points, segment).is_some()
                }
            })
            .map(|segment| segment.external_id)
            .collect::<BTreeSet<_>>();
        let constraints = segments
            .rows
            .iter()
            .filter(|segment| emitted.contains(&segment.external_id))
            .filter_map(|segment| {
                let entity = SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, segment.external_id
                ));
                let constraint_definition = line_orientation_definition(segment, entity)?;
                let id = SketchConstraintId(format!(
                    "creo:featdefs:sketch_constraint#{}:verhor:{}",
                    definition.id, segment.external_id
                ));
                annotate(
                    annotations,
                    &id.0,
                    "FeatDefs",
                    segment.offset as u64,
                    "section_line_orientation_constraint",
                    Exactness::ByteExact,
                );
                Some(SketchConstraint {
                    id,
                    sketch: sketch_id.clone(),
                    definition: constraint_definition,
                    native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                })
            })
            .collect::<Vec<_>>();
        ir.model.sketch_entities.extend(entities);
        ir.model.sketch_constraints.extend(constraints);
        annotate(
            annotations,
            &sketch_id.0,
            "FeatDefs",
            transform.offset as u64,
            "datum_placed_section",
            Exactness::Derived,
        );
        ir.model.sketches.push(Sketch {
            id: sketch_id,
            name: None,
            configuration: None,
            origin: Point3::new(
                transform.origin[0],
                transform.origin[1],
                transform.origin[2],
            ),
            normal: Vector3::new(
                transform.normal[0],
                transform.normal[1],
                transform.normal[2],
            ),
            u_axis: Vector3::new(
                transform.u_axis[0],
                transform.u_axis[1],
                transform.u_axis[2],
            ),
            profiles: resolved_profile_chains(definition, &emitted),
            native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
        });
    }
}

fn transfer_resolved_revolution_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.feature_section_transforms {
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Revolve) {
            continue;
        }
        let full_turn = ir.model.features.iter().any(|feature| {
            feature.id == IrFeatureId(format!("creo:model:feature#{feature_id}"))
                && matches!(
                    &feature.definition,
                    IrFeatureDefinition::Revolve {
                        construction: RevolutionConstruction {
                            extent: Some(Extent::Angle { angle: Angle(value) }),
                            ..
                        },
                        ..
                    } if (*value - std::f64::consts::TAU).abs() <= 1e-12
                )
        });
        if !full_turn {
            continue;
        }
        let Some(definition) = scan
            .feature_definitions
            .iter()
            .find(|definition| definition.id == transform.definition_id)
        else {
            continue;
        };
        let Some(axis) = resolved_revolution_axis(definition, transform) else {
            continue;
        };
        for spline in definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let suffix = spline.entity_id.map_or_else(
                || format!("offset{}", spline.offset),
                |entity_id| entity_id.to_string(),
            );
            let curve_id = CurveId(format!(
                "creo:featdefs:saved_spline_curve#{}:{suffix}",
                definition.id
            ));
            let Some(CurveGeometry::Nurbs(directrix)) = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry)
            else {
                continue;
            };
            let Some(surface) = revolved_nurbs_surface(directrix, axis) else {
                continue;
            };
            let surface_id = SurfaceId(format!(
                "creo:feature:revolution_surface#{feature_id}:{suffix}"
            ));
            let procedural_id = ProceduralSurfaceId(format!(
                "creo:feature:revolution_construction#{feature_id}:{suffix}"
            ));
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                spline.offset as u64,
                "evaluated_revolution_surface",
                Exactness::Derived,
            );
            annotate(
                annotations,
                &procedural_id,
                "FeatDefs",
                spline.offset as u64,
                "revolution_surface_construction",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Nurbs(surface),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("FeatDefs:revolution#{feature_id}:{suffix}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id,
                definition: ProceduralSurfaceDefinition::Revolution {
                    directrix: curve_id,
                    axis_origin: axis.origin,
                    axis_direction: axis.direction,
                    angular_interval: [0.0, std::f64::consts::TAU],
                    parameter_interval: [
                        *directrix.knots.first().expect("validated spline knots"),
                        *directrix.knots.last().expect("validated spline knots"),
                    ],
                    transposed: false,
                },
                cache_fit_tolerance: None,
            });
            transferred += 1;
        }
    }
    transferred
}

fn transfer_feature_dimensions(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let feature_ids = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    for definition in &scan.feature_definitions {
        let Some(owner_feature_id) = definition.owner_feature_id else {
            continue;
        };
        let owner = IrFeatureId(format!("creo:model:feature#{owner_feature_id}"));
        if !feature_ids.contains(&owner) {
            continue;
        }
        let Some(table) = &definition.dimensions else {
            continue;
        };
        for (ordinal, dimension) in table.rows.iter().enumerate() {
            let Some(value) = dimension.value else {
                continue;
            };
            let id = ParameterId(format!(
                "creo:featdefs:parameter#{}:{}",
                owner_feature_id, dimension.external_id
            ));
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                dimension.offset as u64,
                "section_dimension",
                Exactness::Derived,
            );
            let mut properties = BTreeMap::from([
                (
                    "dimension_type".to_string(),
                    dimension.dimension_type.to_string(),
                ),
                (
                    "direction_byte".to_string(),
                    dimension.direction_byte.to_string(),
                ),
            ]);
            if let Some(auxiliary) = dimension.auxiliary_value {
                properties.insert("auxiliary_value".to_string(), auxiliary.to_string());
            }
            let expression = value.to_string();
            let value = match dimension.value_unit {
                crate::feature::DimensionUnit::Radians => ParameterValue::Angle(Angle(value)),
                crate::feature::DimensionUnit::Millimeters => ParameterValue::Length(Length(value)),
                crate::feature::DimensionUnit::SchemaDefined => ParameterValue::Real(value),
            };
            ir.model.parameters.push(DesignParameter {
                id,
                owner: owner.clone(),
                ordinal: ordinal as u32,
                name: format!("d{}", dimension.external_id),
                expression,
                display: None,
                value: Some(value),
                dependencies: Vec::new(),
                properties,
                pmi: None,
                native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
            });
        }
    }
}

fn feature_output_bodies(scan: &ContainerScan, ir: &CadIr, feature_id: u32) -> Vec<BodyId> {
    let generated_surfaces = scan
        .feature_entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| &table.surface_ids)
        .map(|surface_id| SurfaceId(format!("creo:visibgeom:surface#{surface_id}")));
    let mut outputs = Vec::new();
    let evaluated = BodyId(format!("creo:feature:extrusion#{feature_id}:body"));
    if ir.model.bodies.iter().any(|body| body.id == evaluated) {
        outputs.push(evaluated);
    }
    for surface in generated_surfaces {
        for face in ir.model.faces.iter().filter(|face| face.surface == surface) {
            let Some(shell) = ir.model.shells.iter().find(|shell| shell.id == face.shell) else {
                continue;
            };
            let Some(region) = ir
                .model
                .regions
                .iter()
                .find(|region| region.id == shell.region)
            else {
                continue;
            };
            if !outputs.contains(&region.body) {
                outputs.push(region.body.clone());
            }
        }
    }
    outputs
}

fn feature_field_text(value: &crate::feature::FeatureFieldValue) -> Option<String> {
    match value {
        crate::feature::FeatureFieldValue::Empty => Some("empty".to_string()),
        crate::feature::FeatureFieldValue::CompactInt(value) => Some(value.to_string()),
        crate::feature::FeatureFieldValue::CompactIntArray(values) => Some(
            values
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        crate::feature::FeatureFieldValue::EntityReference {
            entity_id,
            terminated,
        } => Some(format!(
            "entity:{entity_id}{}",
            if *terminated { ":terminated" } else { "" }
        )),
        crate::feature::FeatureFieldValue::ScalarArray {
            decoded_values: Some(values),
            ..
        } => Some(
            values
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        crate::feature::FeatureFieldValue::ScalarArray {
            decoded_values: None,
            ..
        }
        | crate::feature::FeatureFieldValue::Raw(_) => None,
    }
}

fn insert_feature_parameter(parameters: &mut BTreeMap<String, String>, base: &str, value: String) {
    if let std::collections::btree_map::Entry::Vacant(entry) = parameters.entry(base.to_string()) {
        entry.insert(value);
        return;
    }
    let mut occurrence = 2;
    loop {
        let name = format!("{base}#{occurrence}");
        if let std::collections::btree_map::Entry::Vacant(entry) = parameters.entry(name) {
            entry.insert(value);
            return;
        }
        occurrence += 1;
    }
}

fn feature_parameters(scan: &ContainerScan, feature_id: u32) -> BTreeMap<String, String> {
    let mut parameters = BTreeMap::new();
    for field in scan
        .feature_choice_fields
        .iter()
        .filter(|field| field.feature_id == feature_id)
    {
        let Some(value) = feature_field_text(&field.value) else {
            continue;
        };
        insert_feature_parameter(
            &mut parameters,
            &format!("choice.{}.{}", field.choice_label, field.name),
            value,
        );
    }
    for affected in scan
        .feature_affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match affected.kind {
            crate::feature::AffectedIdKind::Geometry => "affected_geometry_ids",
            crate::feature::AffectedIdKind::Edges => "affected_edge_ids",
            crate::feature::AffectedIdKind::StrongParents => "strong_parent_feature_ids",
            crate::feature::AffectedIdKind::Parents => "parent_feature_ids",
            crate::feature::AffectedIdKind::Contours => "contour_ids",
        };
        parameters.insert(
            name.to_string(),
            affected
                .ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    for affected in scan
        .feature_replay_affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        insert_feature_parameter(
            &mut parameters,
            "replay_affected_ids",
            affected
                .ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_affected_counted",
            affected.has_count_opener.to_string(),
        );
    }
    for direction in scan
        .feature_direction_bytes
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match direction.lane {
            crate::feature::DirectionLane::Primary => "direction",
            crate::feature::DirectionLane::Secondary => "direction2",
        };
        let value = match direction.value {
            crate::feature::DirectionValue::SideFlag(value) => value.to_string(),
            crate::feature::DirectionValue::Raw(value) => value.to_string(),
        };
        parameters.insert(name.to_string(), value);
    }
    if let Some(definition) = scan
        .feature_definitions
        .iter()
        .find(|definition| definition.owner_feature_id == Some(feature_id))
    {
        parameters.insert(
            "sketch_segment_count".to_string(),
            definition
                .segments
                .as_ref()
                .map_or(0, |segments| segments.rows.len())
                .to_string(),
        );
        parameters.insert(
            "dimension_count".to_string(),
            definition
                .dimensions
                .as_ref()
                .map_or(0, |dimensions| dimensions.rows.len())
                .to_string(),
        );
    }
    for transform in scan
        .feature_section_transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
    {
        insert_feature_parameter(
            &mut parameters,
            "profile_sketch",
            format!("creo:model:sketch#{}", transform.definition_id),
        );
        if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Extrude) {
            insert_feature_parameter(
                &mut parameters,
                "sweep_direction",
                transform
                    .normal
                    .iter()
                    .map(f64::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
    }
    parameters
}

fn schema_operation_kind(schema_class: u32) -> Option<&'static str> {
    match schema_class {
        911 => Some("Hole"),
        913 => Some("Round"),
        916 | 917 => Some("Protrusion"),
        923 => Some("Datum Plane"),
        _ => None,
    }
}

fn feature_source_properties(scan: &ContainerScan, feature_id: u32) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    if let Some(recipe) = feature_recipe(scan, feature_id) {
        properties.insert(
            "recipe".to_string(),
            match recipe {
                crate::feature::FeatureRecipeKind::Extrude => "protextrude",
                crate::feature::FeatureRecipeKind::Revolve => "protrevolve",
            }
            .to_string(),
        );
    }
    if let Some(schema_class) = scan
        .feature_rows
        .iter()
        .find(|row| row.feature_id == feature_id)
        .and_then(|row| row.root_schema_class)
        .or_else(|| {
            scan.feature_operations
                .iter()
                .find(|operation| operation.feature_id == feature_id)
                .and_then(|operation| operation.root_schema_class)
        })
    {
        properties.insert(
            "featdefs_schema_class".to_string(),
            schema_class.to_string(),
        );
    }
    properties
}

fn feature_dependencies(scan: &ContainerScan, ir: &CadIr, feature_id: u32) -> Vec<IrFeatureId> {
    scan.feature_affected_ids
        .iter()
        .filter(|record| {
            record.feature_id == feature_id
                && matches!(
                    record.kind,
                    crate::feature::AffectedIdKind::StrongParents
                        | crate::feature::AffectedIdKind::Parents
                )
        })
        .flat_map(|record| &record.ids)
        .chain(
            scan.feature_operations
                .iter()
                .filter(|operation| operation.feature_id == feature_id)
                .filter_map(|operation| operation.parent_feature_id.as_ref()),
        )
        .filter_map(|dependency| {
            let id = IrFeatureId(format!("creo:model:feature#{dependency}"));
            ir.model
                .features
                .iter()
                .any(|feature| feature.id == id)
                .then_some(id)
        })
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

fn resolved_revolution_axis(
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<RevolutionAxis> {
    definition.variables.as_ref()?;
    let segments = definition.segments.as_ref()?;
    let points = resolved_section_points(definition);
    let candidates = segments
        .rows
        .iter()
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .filter_map(|segment| {
            let start = points.get(&segment.point_ids[0])?;
            let end = points.get(&segment.point_ids[1])?;
            if start[0] != 0.0 || end[0] != 0.0 || start == end {
                return None;
            }
            let start = section_point_in_model(transform, *start);
            let end = section_point_in_model(transform, *end);
            let direction = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            Some(RevolutionAxis {
                origin: Point3::new(start[0], start[1], start[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        })
        .collect::<Vec<_>>();
    let [axis] = candidates.as_slice() else {
        return None;
    };
    Some(*axis)
}

fn schema_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    schema_class: u32,
    kind: &str,
) -> IrFeatureDefinition {
    if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Revolve) {
        let transforms = scan
            .feature_section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        if let [transform] = transforms.as_slice() {
            let sketch_id = SketchId(format!("creo:model:sketch#{}", transform.definition_id));
            let profile = ir
                .model
                .sketches
                .iter()
                .find(|sketch| sketch.id == sketch_id)
                .map(|sketch| {
                    if sketch.profiles.is_empty() {
                        ProfileRef::Native(format!(
                            "creo:featdefs:sketch#{}",
                            transform.definition_id
                        ))
                    } else {
                        ProfileRef::Sketch(sketch_id)
                    }
                });
            let axis = scan
                .feature_definitions
                .iter()
                .find(|definition| definition.id == transform.definition_id)
                .and_then(|definition| resolved_revolution_axis(definition, transform));
            if profile.is_some() || axis.is_some() {
                return IrFeatureDefinition::Revolve {
                    construction: RevolutionConstruction {
                        profile,
                        axis,
                        extent: None,
                    },
                    op: BooleanOp::Unresolved,
                };
            }
        }
    }
    if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Extrude) {
        let transforms = scan
            .feature_section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        if let [transform] = transforms.as_slice() {
            let sketch_id = SketchId(format!("creo:model:sketch#{}", transform.definition_id));
            let profile = ir
                .model
                .sketches
                .iter()
                .find(|sketch| sketch.id == sketch_id)
                .map(|sketch| {
                    if sketch.profiles.is_empty() {
                        ProfileRef::Native(format!(
                            "creo:featdefs:sketch#{}",
                            transform.definition_id
                        ))
                    } else {
                        ProfileRef::Sketch(sketch_id.clone())
                    }
                });
            if let Some(profile) = profile {
                let cap_origins = scan
                    .surface_rows
                    .iter()
                    .filter(|row| {
                        row.feature_id == feature_id
                            && row.kind == crate::surface::SurfaceKind::Plane
                    })
                    .filter_map(|row| {
                        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
                        ir.model.surfaces.iter().find(|surface| surface.id == id)
                    })
                    .filter_map(|surface| match surface.geometry {
                        SurfaceGeometry::Plane { origin, normal, .. } => Some((
                            [origin.x, origin.y, origin.z],
                            [normal.x, normal.y, normal.z],
                        )),
                        _ => None,
                    });
                if let Some((extent, direction)) =
                    extrusion_extent_and_direction(transform.origin, transform.normal, cap_origins)
                {
                    return IrFeatureDefinition::Extrude {
                        profile,
                        direction: Some(Vector3::new(direction[0], direction[1], direction[2])),
                        extent,
                        op: if ir.model.bodies.iter().any(|body| {
                            body.id == BodyId(format!("creo:feature:extrusion#{feature_id}:body"))
                        }) {
                            BooleanOp::NewBody
                        } else {
                            BooleanOp::Unresolved
                        },
                        draft: None,
                    };
                }
            }
        }
    }
    if schema_class == 923 {
        let planes = scan
            .surface_rows
            .iter()
            .filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
            })
            .filter_map(|row| {
                let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
                ir.model.surfaces.iter().find(|surface| surface.id == id)
            })
            .collect::<Vec<_>>();
        if let [plane] = planes.as_slice() {
            if let SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } = &plane.geometry
            {
                return IrFeatureDefinition::DatumPlane {
                    origin: *origin,
                    normal: *normal,
                    u_axis: *u_axis,
                };
            }
        }
    }
    IrFeatureDefinition::Native {
        kind: kind.to_string(),
        parameters: feature_parameters(scan, feature_id),
        properties: BTreeMap::new(),
    }
}

fn retain_native_feature_parameters(
    source_properties: &mut BTreeMap<String, String>,
    definition: &IrFeatureDefinition,
    parameters: &BTreeMap<String, String>,
) {
    if matches!(definition, IrFeatureDefinition::Native { .. }) {
        return;
    }
    for (name, value) in parameters {
        source_properties.insert(format!("native_parameter.{name}"), value.clone());
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExtrusionSpan {
    lower: f64,
    upper: f64,
}

fn extrusion_span(
    profile_origin: [f64; 3],
    direction: [f64; 3],
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<ExtrusionSpan> {
    let direction_length = direction
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    if direction_length <= f64::EPSILON {
        return None;
    }
    let direction = direction.map(|value| value / direction_length);
    let mut offsets = Vec::<f64>::new();
    for (origin, normal) in planes {
        let normal_length = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
        if normal_length <= f64::EPSILON {
            continue;
        }
        let parallel = normal
            .iter()
            .zip(direction)
            .map(|(left, right)| left * right)
            .sum::<f64>()
            .abs();
        if (parallel / normal_length - 1.0).abs() > 1e-9 {
            continue;
        }
        let offset = origin
            .iter()
            .zip(profile_origin)
            .zip(direction)
            .map(|((coordinate, base), axis)| (coordinate - base) * axis)
            .sum::<f64>();
        let scale = offset.abs().max(1.0);
        if !offsets
            .iter()
            .any(|known| (known - offset).abs() <= 1e-9 * scale)
        {
            offsets.push(offset);
        }
    }
    match offsets.as_slice() {
        [offset] if offset.abs() > 1e-12 => Some(ExtrusionSpan {
            lower: offset.min(0.0),
            upper: offset.max(0.0),
        }),
        [first, second] if first * second < 0.0 => Some(ExtrusionSpan {
            lower: first.min(*second),
            upper: first.max(*second),
        }),
        _ => None,
    }
}

fn extrusion_extent_and_direction(
    profile_origin: [f64; 3],
    direction: [f64; 3],
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<(Extent, [f64; 3])> {
    let span = extrusion_span(profile_origin, direction, planes)?;
    let direction = normalized(direction)?;
    if span.lower == 0.0 || span.upper == 0.0 {
        let signed_length = if span.upper == 0.0 {
            span.lower
        } else {
            span.upper
        };
        return Some((
            Extent::Blind {
                length: Length(signed_length.abs()),
            },
            direction.map(|value| value * signed_length.signum()),
        ));
    }
    let first = span.upper;
    let second = -span.lower;
    let scale = first.max(second).max(1.0);
    let extent = if (first - second).abs() <= 1e-9 * scale {
        Extent::Symmetric {
            length: Length(first + second),
        }
    } else {
        Extent::TwoSided {
            first: Length(first),
            second: Length(second),
        }
    };
    Some((extent, direction))
}

#[cfg(test)]
mod resolved_sketch_tests {
    use super::*;

    #[test]
    fn reversed_arc_uses_opposite_axis_and_canonical_increasing_domain() {
        let (axis_sign, range) = oriented_arc_parameterization(
            true,
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
        );

        assert_eq!(axis_sign, -1.0);
        assert_eq!(
            range,
            [
                3.0 * std::f64::consts::FRAC_PI_2,
                5.0 * std::f64::consts::FRAC_PI_2
            ]
        );
    }

    #[test]
    fn equal_opposite_cap_planes_define_symmetric_extent() {
        let extent = extrusion_extent_and_direction(
            [0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0],
            [
                ([0.0, 4.0, 0.0], [0.0, 1.0, 0.0]),
                ([0.0, -4.0, 0.0], [0.0, 1.0, 0.0]),
                ([3.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ],
        );

        assert_eq!(
            extent,
            Some((
                Extent::Symmetric {
                    length: Length(8.0)
                },
                [0.0, -1.0, 0.0]
            ))
        );
    }

    #[test]
    fn asymmetric_cap_planes_define_two_sided_extent() {
        assert_eq!(
            extrusion_extent_and_direction(
                [0.0; 3],
                [0.0, 0.0, 1.0],
                [
                    ([0.0, 0.0, -2.0], [0.0, 0.0, 1.0]),
                    ([0.0, 0.0, 3.0], [0.0, 0.0, 1.0]),
                ],
            ),
            Some((
                Extent::TwoSided {
                    first: Length(3.0),
                    second: Length(2.0),
                },
                [0.0, 0.0, 1.0],
            ))
        );
    }

    #[test]
    fn one_negative_cap_offset_reverses_blind_direction() {
        assert_eq!(
            extrusion_extent_and_direction(
                [0.0; 3],
                [0.0, -1.0, 0.0],
                [([0.0, 48.0, 0.0], [0.0, 1.0, 0.0])],
            ),
            Some((
                Extent::Blind {
                    length: Length(48.0),
                },
                [-0.0, 1.0, -0.0],
            ))
        );
    }

    #[test]
    fn section_line_requires_two_solved_points() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [7, 9],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 12,
            offset: 40,
        };
        let mut points = BTreeMap::from([(7, [2.0, 3.0])]);
        assert!(section_line_geometry(&points, &segment).is_none());
        points.insert(9, [5.0, 8.0]);
        assert_eq!(
            section_line_geometry(&points, &segment),
            Some(SketchGeometry::Line {
                start: cadmpeg_ir::math::Point2::new(2.0, 3.0),
                end: cadmpeg_ir::math::Point2::new(5.0, 8.0),
            })
        );
    }

    #[test]
    fn section_point_uses_its_single_solved_position() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Point,
            directions: [None; 3],
            point_ids: [7, 7],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 4,
            offset: 40,
        };
        let points = BTreeMap::from([(7, [2.0, 3.0])]);

        assert_eq!(
            section_point_geometry(&points, &segment),
            Some(SketchGeometry::Point {
                position: cadmpeg_ir::math::Point2::new(2.0, 3.0),
            })
        );
    }

    #[test]
    fn section_axis_line_carrier_uses_equal_decoded_ordinates() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [Some(0), None, Some(0)],
            point_ids: [7, 9],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 12,
            offset: 40,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 5,
            owner_feature_id: Some(6),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                points: vec![
                    crate::feature::FeatureSectionPoint {
                        point_id: 7,
                        u: Some(2.0),
                        v: None,
                    },
                    crate::feature::FeatureSectionPoint {
                        point_id: 9,
                        u: Some(2.0),
                        v: Some(8.0),
                    },
                ],
                offset: 0,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        assert_eq!(
            section_axis_line_carrier(&definition, &segment),
            Some(SketchGeometry::Line {
                start: cadmpeg_ir::math::Point2::new(2.0, -8.0),
                end: cadmpeg_ir::math::Point2::new(2.0, 8.0),
            })
        );
    }

    #[test]
    fn intersects_evaluated_section_carriers() {
        let horizontal = SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(-2.0, 1.0),
            end: cadmpeg_ir::math::Point2::new(2.0, 1.0),
        };
        let vertical = SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(0.5, -3.0),
            end: cadmpeg_ir::math::Point2::new(0.5, 3.0),
        };
        assert_eq!(
            intersect_section_lines(&horizontal, &vertical),
            Some([0.5, 1.0])
        );

        let circle_half = SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        };
        let endpoint_line = SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(2.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(3.0, 1.0),
        };
        let intersection = intersect_section_line_arc(&endpoint_line, &circle_half)
            .expect("line has one endpoint on the arc");
        assert!((intersection[0] - 2.0).abs() <= 1e-12);
        assert!(intersection[1].abs() <= 1e-12);
    }

    #[test]
    fn saved_line_joins_through_order_table() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [7, 9],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            offset: 40,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 5,
            owner_feature_id: Some(6),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: Some(crate::feature::FeatureOrderTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureOrderRow {
                    external_id: 42,
                    internal_id: 3,
                    bitmask: 0,
                    offset: 10,
                }],
                offset: 8,
            }),
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(crate::feature::FeatureSavedSection {
                entities: vec![crate::feature::FeatureSavedEntity::Line(
                    crate::feature::FeatureSavedLine {
                        entity_id: 3,
                        references: Vec::new(),
                        attributes: Vec::new(),
                        endpoints: [
                            [Some(-8.0), Some(-0.85), Some(0.0)],
                            [Some(8.0), Some(-0.85), None],
                        ],
                        offset: 20,
                    },
                )],
                offset: 18,
            }),
            offset: 0,
        };

        assert_eq!(
            saved_section_line_geometry(&definition, &segment),
            Some(SketchGeometry::Line {
                start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
                end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
            })
        );

        let mut completed = definition;
        completed
            .order_table
            .as_mut()
            .expect("test definition has an order table")
            .rows
            .clear();
        completed.segments = Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![segment.clone()],
            offset: 4,
        });
        completed.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
            rows: vec![crate::feature::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                center_vertex: None,
                kind: crate::feature::TrimEntityKind::Line,
                offset: 6,
            }],
            solved_external_ids: vec![42],
            offset: 5,
        });
        assert_eq!(
            saved_section_line_geometry(&completed, &segment),
            Some(SketchGeometry::Line {
                start: cadmpeg_ir::math::Point2::new(-8.0, -0.85),
                end: cadmpeg_ir::math::Point2::new(8.0, -0.85),
            })
        );
    }

    #[test]
    fn saved_arc_joins_through_order_table() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [7, 9],
            center_id: Some(8),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            offset: 40,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 5,
            owner_feature_id: Some(6),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: Some(crate::feature::FeatureOrderTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureOrderRow {
                    external_id: 42,
                    internal_id: 3,
                    bitmask: 0,
                    offset: 10,
                }],
                offset: 8,
            }),
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(crate::feature::FeatureSavedSection {
                entities: vec![crate::feature::FeatureSavedEntity::Arc(
                    crate::feature::FeatureSavedArc {
                        entity_id: 3,
                        center: [Some(0.0), Some(0.0), Some(0.0)],
                        radius: Some(2.0),
                        endpoints: [
                            [Some(0.0), Some(-2.0), Some(0.0)],
                            [Some(-2.0), Some(0.0), Some(0.0)],
                        ],
                        parameters: [None; 2],
                        offset: 20,
                    },
                )],
                offset: 18,
            }),
            offset: 0,
        };

        assert_eq!(
            saved_section_arc_geometry(&definition, &segment),
            Some(SketchGeometry::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length(2.0),
                start_angle: Angle(std::f64::consts::PI),
                end_angle: Angle(3.0 * std::f64::consts::FRAC_PI_2),
            })
        );

        let mut trimmed = definition;
        trimmed.segments = Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![segment],
            offset: 38,
        });
        trimmed.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
            rows: vec![crate::feature::FeatureTrimEntity {
                external_id: 42,
                mode: Some(0),
                vertices: [1, 2],
                center_vertex: None,
                kind: crate::feature::TrimEntityKind::Arc,
                offset: 30,
            }],
            solved_external_ids: vec![42],
            offset: 28,
        });
        assert_eq!(
            resolved_trim_vertex_coordinates(&trimmed, &BTreeMap::new()),
            BTreeMap::from([(1, [0.0, -2.0]), (2, [-2.0, 0.0])])
        );
        if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
            .saved_section
            .as_mut()
            .expect("test definition has a saved section")
            .entities[0]
        {
            arc.center[1] = None;
            arc.radius = None;
        }
        let segment = &trimmed
            .segments
            .as_ref()
            .expect("test definition has a segment table")
            .rows[0];
        assert_eq!(
            saved_section_arc_carrier(&trimmed, segment),
            Some(([0.0, 0.0], 2.0))
        );
        if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
            .saved_section
            .as_mut()
            .expect("test definition has a saved section")
            .entities[0]
        {
            arc.center[1] = Some(0.0);
            arc.radius = Some(2.0);
        }
        if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
            .saved_section
            .as_mut()
            .expect("test definition has a saved section")
            .entities[0]
        {
            arc.endpoints[0] = [None; 3];
        } else {
            panic!("test entity is an arc");
        }
        assert_eq!(
            resolved_trim_vertex_coordinates(&trimmed, &BTreeMap::new()),
            BTreeMap::from([(2, [-2.0, 0.0])])
        );
        if let crate::feature::FeatureSavedEntity::Arc(arc) = &mut trimmed
            .saved_section
            .as_mut()
            .expect("test definition has a saved section")
            .entities[0]
        {
            arc.endpoints[1] = [None; 3];
        }
        let segment = &trimmed
            .segments
            .as_ref()
            .expect("test definition has a segment table")
            .rows[0];
        assert!(saved_section_arc_geometry(&trimmed, segment).is_none());
        assert_eq!(
            section_segment_intersection_carrier(&trimmed, &BTreeMap::new(), segment)
                .map(|carrier| carrier.geometry),
            Some(SketchGeometry::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length(2.0),
                start_angle: Angle(0.0),
                end_angle: Angle(std::f64::consts::TAU),
            })
        );
    }

    #[test]
    fn saved_arc_carrier_combines_with_trim_vertices() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [7, 9],
            center_id: Some(8),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            offset: 40,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 5,
            owner_feature_id: Some(6),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: Some(crate::feature::FeatureTrimEntityTable {
                rows: vec![crate::feature::FeatureTrimEntity {
                    external_id: 42,
                    mode: Some(0),
                    vertices: [1, 2],
                    center_vertex: None,
                    kind: crate::feature::TrimEntityKind::Arc,
                    offset: 30,
                }],
                solved_external_ids: vec![42],
                offset: 28,
            }),
            trim_vertices: None,
            order_table: Some(crate::feature::FeatureOrderTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureOrderRow {
                    external_id: 42,
                    internal_id: 3,
                    bitmask: 0,
                    offset: 10,
                }],
                offset: 8,
            }),
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(crate::feature::FeatureSavedSection {
                entities: vec![crate::feature::FeatureSavedEntity::Arc(
                    crate::feature::FeatureSavedArc {
                        entity_id: 3,
                        center: [Some(0.0), Some(0.0), Some(0.0)],
                        radius: Some(2.0),
                        endpoints: [[None; 3]; 2],
                        parameters: [None; 2],
                        offset: 20,
                    },
                )],
                offset: 18,
            }),
            offset: 0,
        };
        let trim_vertices = BTreeMap::from([(1, [-2.0, 0.0]), (2, [0.0, -2.0])]);

        assert_eq!(
            trimmed_section_segment_geometry(
                &definition,
                &BTreeMap::new(),
                &trim_vertices,
                &segment,
            ),
            Some(SketchGeometry::Arc {
                center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                radius: Length(2.0),
                start_angle: Angle(-std::f64::consts::FRAC_PI_2),
                end_angle: Angle(std::f64::consts::PI),
            })
        );
    }

    #[test]
    fn placed_extrusion_line_defines_plane() {
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 5,
            feature_id: Some(5),
            origin: [10.0, 20.0, 30.0],
            u_axis: [0.0, 1.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [1.0, 0.0, 0.0],
            offset: 7,
        };
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 3,
            offset: 9,
        };
        let points = BTreeMap::from([(1, [2.0, 3.0]), (2, [6.0, 3.0])]);
        assert_eq!(
            extruded_segment_surface(&transform, &points, &segment),
            Some(SurfaceGeometry::Plane {
                origin: Point3::new(10.0, 22.0, 33.0),
                normal: Vector3::new(0.0, 0.0, -1.0),
                u_axis: Vector3::new(0.0, 1.0, 0.0),
            })
        );
        assert_eq!(
            placed_section_curve_geometry(&transform, &points, &segment),
            Some(CurveGeometry::Line {
                origin: Point3::new(10.0, 22.0, 33.0),
                direction: Vector3::new(0.0, 1.0, 0.0),
            })
        );
    }

    #[test]
    fn placed_extrusion_arc_defines_cylinder() {
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 5,
            feature_id: Some(5),
            origin: [10.0, 20.0, 30.0],
            u_axis: [0.0, 1.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [1.0, 0.0, 0.0],
            offset: 7,
        };
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 4,
            offset: 9,
        };
        let points = BTreeMap::from([(1, [2.0, 0.0]), (2, [-2.0, 0.0]), (3, [0.0, 0.0])]);
        assert_eq!(
            extruded_segment_surface(&transform, &points, &segment),
            Some(SurfaceGeometry::Cylinder {
                origin: Point3::new(10.0, 20.0, 30.0),
                axis: Vector3::new(1.0, 0.0, 0.0),
                ref_direction: Vector3::new(0.0, 1.0, 0.0),
                radius: 2.0,
            })
        );
        assert_eq!(
            placed_section_curve_geometry(&transform, &points, &segment),
            Some(CurveGeometry::Circle {
                center: Point3::new(10.0, 20.0, 30.0),
                axis: Vector3::new(1.0, 0.0, 0.0),
                ref_direction: Vector3::new(0.0, 1.0, 0.0),
                radius: 2.0,
            })
        );
    }

    #[test]
    fn line_orientation_selectors_are_closed() {
        let mut segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [7, 9],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: Some(0),
            radius_ref: None,
            radius2_ref: None,
            external_id: 12,
            offset: 40,
        };
        let entity = SketchEntityId("entity".into());
        assert_eq!(
            line_orientation_definition(&segment, entity.clone()),
            Some(SketchConstraintDefinition::Vertical {
                entity: entity.clone()
            })
        );
        segment.vertical_horizontal = Some(1);
        assert_eq!(
            line_orientation_definition(&segment, entity.clone()),
            Some(SketchConstraintDefinition::Horizontal {
                entity: entity.clone()
            })
        );
        segment.vertical_horizontal = Some(2);
        assert_eq!(line_orientation_definition(&segment, entity.clone()), None);
        segment.kind = crate::feature::FeatureSegmentKind::Arc;
        segment.vertical_horizontal = Some(0);
        assert_eq!(line_orientation_definition(&segment, entity), None);
    }

    #[test]
    fn zero_orientation_arc_runs_clockwise_from_first_endpoint() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: Some(4),
            radius2_ref: None,
            external_id: 12,
            offset: 40,
        };
        let points = BTreeMap::from([(1, [0.0, -2.0]), (2, [0.0, 2.0]), (3, [0.0, 0.0])]);
        let Some(SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        }) = section_arc_geometry(&points, &segment)
        else {
            panic!("complete arc");
        };
        assert_eq!(center, cadmpeg_ir::math::Point2::new(0.0, 0.0));
        assert_eq!(radius, Length(2.0));
        assert!((start_angle.0 - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((end_angle.0 - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn profile_chain_follows_trim_vertex_incidence() {
        let definition = crate::feature::FeatureDefinition {
            id: 40,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: Some(crate::feature::FeatureTrimEntityTable {
                rows: [(10, [1, 2]), (11, [3, 2]), (12, [3, 4]), (13, [4, 1])]
                    .into_iter()
                    .map(
                        |(external_id, vertices)| crate::feature::FeatureTrimEntity {
                            external_id,
                            mode: None,
                            vertices,
                            center_vertex: None,
                            kind: crate::feature::TrimEntityKind::Line,
                            offset: external_id as usize,
                        },
                    )
                    .collect(),
                solved_external_ids: vec![10, 11, 12, 13],
                offset: 5,
            }),
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 1,
        };
        let profiles = resolved_profile_chains(
            &definition,
            &BTreeSet::from([10_u32, 11_u32, 12_u32, 13_u32]),
        );
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].len(), 4);
        assert_eq!(profiles[0][0].entity.0, "creo:featdefs:sketch_entity#40:10");
        assert!(!profiles[0][0].reversed);
        assert!(profiles[0][1].reversed);

        assert!(
            resolved_profile_chains(&definition, &BTreeSet::from([10_u32, 11_u32, 12_u32]))
                .is_empty()
        );

        let mut arcs = definition.clone();
        arcs.trim_entities = Some(crate::feature::FeatureTrimEntityTable {
            rows: [(10, [1, 2]), (11, [2, 1])]
                .into_iter()
                .map(
                    |(external_id, vertices)| crate::feature::FeatureTrimEntity {
                        external_id,
                        mode: None,
                        vertices,
                        center_vertex: Some(3),
                        kind: crate::feature::TrimEntityKind::Arc,
                        offset: external_id as usize,
                    },
                )
                .collect(),
            solved_external_ids: vec![10, 11],
            offset: 5,
        });
        arcs.segments = Some(crate::feature::FeatureSegmentTable {
            declared_count: 2,
            entity_ref: None,
            rows: [10, 11]
                .into_iter()
                .map(|external_id| crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Arc,
                    directions: [None; 3],
                    point_ids: [1, 2],
                    center_id: Some(3),
                    arc_orientation: Some(0),
                    vertical_horizontal: None,
                    radius_ref: None,
                    radius2_ref: None,
                    external_id,
                    offset: external_id as usize,
                })
                .collect(),
            offset: 4,
        });
        let arc_profile = resolved_profile_chains(&arcs, &BTreeSet::from([10, 11]));
        assert_eq!(arc_profile.len(), 1);
        assert!(arc_profile[0].iter().all(|entity| entity.reversed));
    }

    #[test]
    fn revolution_axis_uses_the_unique_complete_section_centerline() {
        let definition = crate::feature::FeatureDefinition {
            id: 40,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(crate::feature::FeatureVariableTable {
                declared_count: 0,
                entity_ref: None,
                rows: Vec::new(),
                points: vec![
                    crate::feature::FeatureSectionPoint {
                        point_id: 1,
                        u: Some(0.0),
                        v: Some(-2.0),
                    },
                    crate::feature::FeatureSectionPoint {
                        point_id: 2,
                        u: Some(0.0),
                        v: Some(3.0),
                    },
                ],
                offset: 1,
            }),
            segments: Some(crate::feature::FeatureSegmentTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureSegment {
                    kind: crate::feature::FeatureSegmentKind::Line,
                    directions: [None; 3],
                    point_ids: [1, 2],
                    center_id: None,
                    arc_orientation: None,
                    vertical_horizontal: Some(0),
                    radius_ref: None,
                    radius2_ref: None,
                    external_id: 1,
                    offset: 2,
                }],
                offset: 2,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 1,
        };
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 40,
            feature_id: Some(40),
            origin: [5.0, 7.0, 11.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [0.0, -1.0, 0.0],
            offset: 3,
        };

        let axis = resolved_revolution_axis(&definition, &transform).expect("axis");
        assert_eq!(axis.origin, Point3::new(5.0, 7.0, 9.0));
        assert_eq!(axis.direction, Vector3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn saved_spline_collocation_interpolates_points_and_endpoint_derivatives() {
        let spline = crate::feature::FeatureSavedSpline {
            entity_id: Some(7),
            interpolation_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            endpoint_tangents: Some([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
            parameters: Some(vec![0.0, 1.0, 2.0]),
            offset: 10,
        };
        let nurbs = saved_spline_nurbs(&spline).expect("clamped interpolation spline");
        for (parameter, expected) in [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)] {
            let point = nurbs.control_points.iter().enumerate().fold(
                [0.0; 3],
                |mut point, (index, control)| {
                    let basis = bspline_basis(
                        index,
                        nurbs.degree as usize,
                        parameter,
                        &nurbs.knots,
                        nurbs.control_points.len(),
                    );
                    point[0] += basis * control.x;
                    point[1] += basis * control.y;
                    point[2] += basis * control.z;
                    point
                },
            );
            assert!((point[0] - expected).abs() < 1e-12);
            assert!(point[1].abs() < 1e-12 && point[2].abs() < 1e-12);
        }
        for parameter in [0.0, 2.0] {
            let derivative = nurbs.control_points.iter().enumerate().fold(
                [0.0; 3],
                |mut derivative, (index, control)| {
                    let basis = bspline_basis_derivative(
                        index,
                        nurbs.degree as usize,
                        parameter,
                        &nurbs.knots,
                        nurbs.control_points.len(),
                    );
                    derivative[0] += basis * control.x;
                    derivative[1] += basis * control.y;
                    derivative[2] += basis * control.z;
                    derivative
                },
            );
            assert!((derivative[0] - 1.0).abs() < 1e-12);
            assert!(derivative[1].abs() < 1e-12 && derivative[2].abs() < 1e-12);
        }
    }

    #[test]
    fn full_revolution_uses_exact_quadratic_circle_poles() {
        let directrix = NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
            weights: None,
            periodic: false,
        };
        let surface = revolved_nurbs_surface(
            &directrix,
            RevolutionAxis {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 0.0, 1.0),
            },
        )
        .expect("revolution surface");

        assert_eq!((surface.u_count, surface.v_count), (2, 9));
        assert_eq!(surface.control_points[0], Point3::new(2.0, 0.0, 0.0));
        assert_eq!(surface.control_points[1], Point3::new(2.0, 2.0, 0.0));
        assert_eq!(surface.control_points[2], Point3::new(0.0, 2.0, 0.0));
        assert_eq!(surface.control_points[8], surface.control_points[0]);
        assert_eq!(
            surface.weights.as_ref().expect("rational weights")[1],
            std::f64::consts::FRAC_1_SQRT_2
        );
    }
}

#[derive(Clone, Copy)]
struct PlaneEquation {
    origin: [f64; 3],
    normal: [f64; 3],
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1].mul_add(right[2], -(left[2] * right[1])),
        left[2].mul_add(right[0], -(left[0] * right[2])),
        left[0].mul_add(right[1], -(left[1] * right[0])),
    ]
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0].mul_add(right[0], left[1].mul_add(right[1], left[2] * right[2]))
}

fn solve_planes(planes: &[PlaneEquation]) -> Option<[f64; 3]> {
    for first in 0..planes.len() {
        for second in first + 1..planes.len() {
            for third in second + 1..planes.len() {
                let a = planes[first];
                let b = planes[second];
                let c = planes[third];
                let b_cross_c = cross(b.normal, c.normal);
                let determinant = dot(a.normal, b_cross_c);
                if determinant.abs() <= 1e-9 {
                    continue;
                }
                let distances = [
                    dot(a.normal, a.origin),
                    dot(b.normal, b.origin),
                    dot(c.normal, c.origin),
                ];
                let c_cross_a = cross(c.normal, a.normal);
                let a_cross_b = cross(a.normal, b.normal);
                let point = [0, 1, 2].map(|axis| {
                    (distances[0] * b_cross_c[axis]
                        + distances[1] * c_cross_a[axis]
                        + distances[2] * a_cross_b[axis])
                        / determinant
                });
                if point.iter().all(|value| value.is_finite())
                    && planes.iter().all(|plane| {
                        (dot(plane.normal, point) - dot(plane.normal, plane.origin)).abs() <= 1e-6
                    })
                {
                    return Some(point);
                }
            }
        }
    }
    None
}

fn is_axis_aligned(vector: [f64; 3]) -> bool {
    vector.iter().filter(|value| value.abs() > 1e-9).count() == 1
}

fn placed_planes(scan: &ContainerScan) -> BTreeMap<u32, PlaneEquation> {
    let mut planes = scan
        .plane_local_systems
        .iter()
        .filter_map(|frame| {
            let origin = frame.origin?;
            let normal = frame.normal?;
            (!is_axis_aligned(normal))
                .then_some((frame.surface_id, PlaneEquation { origin, normal }))
        })
        .collect::<BTreeMap<_, _>>();
    for plane in &scan.outline_planes {
        planes.insert(
            plane.surface_id,
            PlaneEquation {
                origin: plane.origin,
                normal: plane.normal,
            },
        );
    }
    planes
}

fn transfer_plane_brep(scan: &ContainerScan, ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let planes = placed_planes(scan);
    let half_edges = scan
        .half_edges
        .iter()
        .map(|half_edge| (half_edge.id, half_edge))
        .collect::<BTreeMap<_, _>>();
    let incidence = scan
        .half_edge_vertex_incidence
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertices = scan
        .topological_vertices
        .iter()
        .filter_map(|vertex| {
            let incident_planes = vertex
                .half_edges
                .iter()
                .filter_map(|half_edge| half_edges.get(half_edge))
                .filter_map(|half_edge| planes.get(&half_edge.face_id))
                .copied()
                .collect::<Vec<_>>();
            solve_planes(&incident_planes).map(|point| (vertex.id, point))
        })
        .collect::<BTreeMap<_, _>>();
    let edge_vertices = scan
        .curve_topology_rows
        .iter()
        .filter_map(|row| {
            let forward = incidence.get(&HalfEdgeId {
                curve_id: row.id,
                side: 0,
            })?;
            let reverse = incidence.get(&HalfEdgeId {
                curve_id: row.id,
                side: 1,
            })?;
            let end = forward.end_vertex_id?;
            (reverse.start_vertex_id == end
                && reverse.end_vertex_id == Some(forward.start_vertex_id)
                && solved_vertices.contains_key(&forward.start_vertex_id)
                && solved_vertices.contains_key(&end))
            .then_some((row.id, [forward.start_vertex_id, end]))
        })
        .collect::<BTreeMap<_, _>>();
    let loops_per_face = scan
        .loops
        .iter()
        .fold(BTreeMap::<u32, usize>::new(), |mut map, lp| {
            *map.entry(lp.face_id).or_default() += 1;
            map
        });
    let eligible_loops = scan
        .loops
        .iter()
        .filter(|lp| planes.contains_key(&lp.face_id))
        .filter(|lp| loops_per_face.get(&lp.face_id) == Some(&1))
        .filter(|lp| {
            lp.half_edges
                .iter()
                .all(|half_edge| edge_vertices.contains_key(&half_edge.curve_id))
        })
        .collect::<Vec<_>>();
    if eligible_loops.is_empty() {
        return;
    }

    let emitted_half_edges = eligible_loops
        .iter()
        .flat_map(|lp| lp.half_edges.iter().copied())
        .collect::<BTreeSet<_>>();
    let emitted_curves = emitted_half_edges
        .iter()
        .map(|half_edge| half_edge.curve_id)
        .collect::<BTreeSet<_>>();
    let used_vertices = emitted_curves
        .iter()
        .filter_map(|curve| edge_vertices.get(curve))
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let row_offsets = scan
        .curve_topology_rows
        .iter()
        .map(|row| (row.id, row.offset))
        .collect::<BTreeMap<_, _>>();

    for vertex_id in used_vertices {
        let point_id = PointId(format!("creo:visibgeom:point#{vertex_id}"));
        let vertex = VertexId(format!("creo:visibgeom:vertex#{vertex_id}"));
        if ir.model.vertices.iter().any(|item| item.id == vertex) {
            continue;
        }
        let position = solved_vertices[&vertex_id];
        annotate(
            annotations,
            &point_id,
            "VisibGeom",
            0,
            "plane_intersection_point",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &vertex,
            "VisibGeom",
            0,
            "topological_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: Point3::new(position[0], position[1], position[2]),
        });
        ir.model.vertices.push(Vertex {
            id: vertex,
            point: point_id,
            tolerance: None,
        });
    }
    for curve_id in &emitted_curves {
        let [start, end] = edge_vertices[curve_id];
        let id = EdgeId(format!("creo:visibgeom:edge#{curve_id}"));
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row_offsets.get(curve_id).copied().unwrap_or(0) as u64,
            "curve_topology_edge",
            Exactness::Derived,
        );
        ir.model.edges.push(Edge {
            id,
            curve: Some(CurveId(format!("creo:visibgeom:curve#{curve_id}"))),
            start: VertexId(format!("creo:visibgeom:vertex#{start}")),
            end: VertexId(format!("creo:visibgeom:vertex#{end}")),
            param_range: None,
            tolerance: None,
        });
    }

    let mut face_adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    for lp in &eligible_loops {
        face_adjacency.entry(lp.face_id).or_default();
    }
    for curve_id in &emitted_curves {
        let faces = emitted_half_edges
            .iter()
            .filter(|half_edge| half_edge.curve_id == *curve_id)
            .filter_map(|half_edge| half_edges.get(half_edge))
            .map(|half_edge| half_edge.face_id)
            .collect::<Vec<_>>();
        if let [first, second] = faces.as_slice() {
            face_adjacency.entry(*first).or_default().insert(*second);
            face_adjacency.entry(*second).or_default().insert(*first);
        }
    }
    let mut remaining = face_adjacency.keys().copied().collect::<BTreeSet<_>>();
    let mut components = Vec::new();
    while let Some(start) = remaining.pop_first() {
        let mut component = BTreeSet::from([start]);
        let mut pending = vec![start];
        while let Some(face) = pending.pop() {
            for neighbour in face_adjacency.get(&face).into_iter().flatten() {
                if remaining.remove(neighbour) {
                    component.insert(*neighbour);
                    pending.push(*neighbour);
                }
            }
        }
        components.push(component);
    }

    for (component_index, faces) in components.iter().enumerate() {
        let body_id = BodyId(format!("creo:visibgeom:body#{}", component_index + 1));
        let region_id = RegionId(format!("creo:visibgeom:region#{}", component_index + 1));
        let shell_id = ShellId(format!("creo:visibgeom:shell#{}", component_index + 1));
        for (id, tag) in [
            (body_id.to_string(), "plane_sheet_body"),
            (region_id.to_string(), "plane_sheet_region"),
            (shell_id.to_string(), "plane_sheet_shell"),
        ] {
            annotate(annotations, id, "VisibGeom", 0, tag, Exactness::Derived);
        }
        let component_curves = eligible_loops
            .iter()
            .filter(|lp| faces.contains(&lp.face_id))
            .flat_map(|lp| lp.half_edges.iter().map(|half_edge| half_edge.curve_id))
            .collect::<BTreeSet<_>>();
        let closed = component_curves.iter().all(|curve_id| {
            let adjacent = emitted_half_edges
                .iter()
                .filter(|half_edge| half_edge.curve_id == *curve_id)
                .filter_map(|half_edge| half_edges.get(half_edge))
                .map(|half_edge| half_edge.face_id)
                .collect::<BTreeSet<_>>();
            adjacent.len() == 2 && adjacent.iter().all(|face| faces.contains(face))
        });
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: if closed {
                BodyKind::Solid
            } else {
                BodyKind::Sheet
            },
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id,
            faces: faces
                .iter()
                .map(|face| FaceId(format!("creo:visibgeom:face#{face}")))
                .collect(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        for face_id in faces {
            let native_loop = eligible_loops
                .iter()
                .find(|lp| lp.face_id == *face_id)
                .expect("eligible face has one loop");
            let face = FaceId(format!("creo:visibgeom:face#{face_id}"));
            let loop_id = LoopId(format!("creo:visibgeom:loop#{face_id}"));
            let face_offset = scan
                .surface_rows
                .iter()
                .find(|row| row.id == *face_id)
                .map_or(0, |row| row.offset);
            annotate(
                annotations,
                &face,
                "VisibGeom",
                face_offset as u64,
                "plane_face",
                Exactness::Derived,
            );
            annotate(
                annotations,
                &loop_id,
                "VisibGeom",
                face_offset as u64,
                "native_face_loop",
                Exactness::Derived,
            );
            ir.model.faces.push(Face {
                id: face.clone(),
                shell: shell_id.clone(),
                surface: SurfaceId(format!("creo:visibgeom:surface#{face_id}")),
                sense: Sense::Forward,
                loops: vec![loop_id.clone()],
                name: None,
                color: None,
                tolerance: None,
            });
            let coedge_ids = native_loop
                .half_edges
                .iter()
                .map(|half_edge| {
                    CoedgeId(format!(
                        "creo:visibgeom:coedge#{}:{}",
                        half_edge.curve_id, half_edge.side
                    ))
                })
                .collect::<Vec<_>>();
            ir.model.loops.push(IrLoop {
                id: loop_id.clone(),
                face,
                coedges: coedge_ids.clone(),
            });
            for (index, half_edge) in native_loop.half_edges.iter().enumerate() {
                let id = coedge_ids[index].clone();
                let twin = HalfEdgeId {
                    curve_id: half_edge.curve_id,
                    side: 1 - half_edge.side,
                };
                let radial_next = if emitted_half_edges.contains(&twin) {
                    CoedgeId(format!(
                        "creo:visibgeom:coedge#{}:{}",
                        twin.curve_id, twin.side
                    ))
                } else {
                    id.clone()
                };
                annotate(
                    annotations,
                    &id,
                    "VisibGeom",
                    row_offsets.get(&half_edge.curve_id).copied().unwrap_or(0) as u64,
                    "native_half_edge",
                    Exactness::Derived,
                );
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: loop_id.clone(),
                    edge: EdgeId(format!("creo:visibgeom:edge#{}", half_edge.curve_id)),
                    next: coedge_ids[(index + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()].clone(),
                    radial_next,
                    sense: if half_edge.side == 0 {
                        Sense::Forward
                    } else {
                        Sense::Reversed
                    },
                    pcurve: None,
                });
            }
        }
    }
}

fn transfer_cap_pair_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let planes = scan
        .outline_planes
        .iter()
        .map(|plane| (plane.surface_id, plane))
        .collect::<BTreeMap<_, _>>();
    let circle_offsets = scan
        .fc05_circles
        .iter()
        .map(|circle| (circle.curve_id, circle.offset))
        .collect::<BTreeMap<_, _>>();
    for pair in &scan.fc05_cylinder_cap_pairs {
        let placed_caps = pair
            .cap_plane_ids
            .iter()
            .zip(&pair.curve_cap_ordinates_row_frame)
            .filter_map(|(id, ordinate)| planes.get(id).map(|plane| (*plane, *ordinate)))
            .collect::<Vec<_>>();
        let Some((first_cap, first_ordinate)) = placed_caps.first().copied() else {
            continue;
        };
        let Some(axis_index) = (0..3).find(|axis| first_cap.normal[*axis].abs() == 1.0) else {
            continue;
        };
        if axis_index == 2
            || placed_caps
                .iter()
                .any(|(plane, _)| plane.normal != first_cap.normal)
        {
            continue;
        }
        let translations = placed_caps
            .iter()
            .map(|(plane, ordinate)| plane.origin[axis_index] - ordinate)
            .collect::<Vec<_>>();
        if translations
            .iter()
            .any(|translation| (translation - translations[0]).abs() > 1e-9)
        {
            continue;
        }
        let axis_origin = first_ordinate + translations[0];
        let [first, second] = pair.center_row_frame;
        let [reference_x, reference_z] = pair.reference_direction_row_frame;
        let axis_sign = -f64::from(pair.parameter_sign);
        let (origin, axis, ref_direction) = if axis_index == 0 {
            (
                [axis_origin, second, first],
                [axis_sign, 0.0, 0.0],
                [0.0, reference_z, reference_x],
            )
        } else {
            (
                [first, axis_origin, second],
                [0.0, axis_sign, 0.0],
                [reference_x, 0.0, reference_z],
            )
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", pair.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            pair.offset as u64,
            "fc05_cap_pair_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
                radius: pair.radius_mm,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", pair.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        for ((curve_id, ordinate), cap_plane_id) in pair
            .curve_ids
            .iter()
            .zip(&pair.curve_cap_ordinates_row_frame)
            .zip(&pair.cap_plane_ids)
        {
            let cap_offset = planes.get(cap_plane_id).map_or_else(
                || ordinate + translations[0],
                |plane| plane.origin[axis_index],
            );
            let center = if axis_index == 0 {
                [cap_offset, second, first]
            } else {
                [first, cap_offset, second]
            };
            let id = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
            if ir.model.curves.iter().any(|curve| curve.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "VisibGeom",
                circle_offsets.get(curve_id).copied().unwrap_or(pair.offset) as u64,
                "fc05_cap_circle",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(
                        ref_direction[0],
                        ref_direction[1],
                        ref_direction[2],
                    ),
                    radius: pair.radius_mm,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{curve_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
    }
}

fn transfer_fc05_cap_circles(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let planes = scan
        .outline_planes
        .iter()
        .map(|plane| (plane.surface_id, plane))
        .collect::<BTreeMap<_, _>>();
    let kinds = scan
        .surface_rows
        .iter()
        .map(|surface| (surface.id, surface.kind))
        .collect::<BTreeMap<_, _>>();
    for circle in &scan.fc05_circles {
        let topology = scan
            .curve_topology_rows
            .iter()
            .filter(|row| row.id == circle.curve_id)
            .collect::<Vec<_>>();
        let [topology] = topology.as_slice() else {
            continue;
        };
        let cap_planes = topology
            .faces
            .iter()
            .filter(|face| kinds.get(face) == Some(&crate::surface::SurfaceKind::Plane))
            .filter_map(|face| planes.get(face).copied())
            .collect::<Vec<_>>();
        let cylinders = topology
            .faces
            .iter()
            .filter(|face| kinds.get(face) == Some(&crate::surface::SurfaceKind::Cylinder))
            .copied()
            .collect::<Vec<_>>();
        let ([cap], [cylinder_id], Some(reference), Some(parameter_sign), Some(axis_ordinate)) = (
            cap_planes.as_slice(),
            cylinders.as_slice(),
            circle.reference_direction_row_frame,
            circle.parameter_sign,
            circle.cap_ordinate_row_frame,
        ) else {
            continue;
        };
        let Some(axis_index) = (0..3).find(|axis| cap.normal[*axis].abs() == 1.0) else {
            continue;
        };
        if axis_index == 2 {
            continue;
        }
        let [first, second] = circle.center_row_frame;
        let axis_sign = -f64::from(parameter_sign);
        let (center, axis, ref_direction) = if axis_index == 0 {
            (
                [cap.origin[0], second, first],
                [axis_sign, 0.0, 0.0],
                [0.0, reference[1], reference[0]],
            )
        } else {
            (
                [first, cap.origin[1], second],
                [0.0, axis_sign, 0.0],
                [reference[0], 0.0, reference[1]],
            )
        };
        let id = CurveId(format!("creo:visibgeom:curve#{}", circle.curve_id));
        if !ir.model.curves.iter().any(|curve| curve.id == id) {
            annotate(
                annotations,
                &id,
                "VisibGeom",
                circle.offset as u64,
                "fc05_cap_circle",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(
                        ref_direction[0],
                        ref_direction[1],
                        ref_direction[2],
                    ),
                    radius: circle.radius_mm,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{}", circle.curve_id),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        let surface_id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        let axis_origin = if axis_index == 0 {
            [axis_ordinate, second, first]
        } else {
            [first, axis_ordinate, second]
        };
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            circle.offset as u64,
            "fc05_axis_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id: surface_id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(axis_origin[0], axis_origin[1], axis_origin[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
                radius: circle.radius_mm,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{cylinder_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
}

fn transfer_plane_intersection_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let planes = placed_planes(scan);
    for row in &scan.curve_topology_rows {
        let (Some(first), Some(second)) = (planes.get(&row.faces[0]), planes.get(&row.faces[1]))
        else {
            continue;
        };
        let direction = cross(first.normal, second.normal);
        let denominator = dot(direction, direction);
        if denominator <= 1e-18 {
            continue;
        }
        let first_distance = dot(first.normal, first.origin);
        let second_distance = dot(second.normal, second.origin);
        let weighted = [0, 1, 2].map(|axis| {
            first_distance * second.normal[axis] - second_distance * first.normal[axis]
        });
        let point_numerator = cross(weighted, direction);
        let origin = point_numerator.map(|value| value / denominator);
        let direction_norm = denominator.sqrt();
        let direction = direction.map(|value| value / direction_norm);
        let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            "plane_intersection_line",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Line {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
}

/// Decode a `.prt` stream into an IR document and loss report.
///
/// The stream is read from its beginning. `options.container_only` is reflected
/// in the report, but the current decoder always performs the same structural
/// scan.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    let ir = build_ir(&scan)?;
    let report = build_report(&scan, options.container_only);
    Ok(DecodeResult::new(ir, report))
}

/// Build source metadata, preserved geometry records, and datum-plane surfaces.
fn build_ir(scan: &ContainerScan) -> Result<CadIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));

    for section in scan.sections.iter().filter(|s| s.role == role::GEOMETRY) {
        let end = (section.offset + section.length).min(scan.data.len());
        let bytes = &scan.data[section.offset..end];
        let id = UnknownId(format!("creo:{}:section#{}", section.name, section.offset));
        annotate(
            &mut annotations,
            &id,
            &section.name,
            section.offset as u64,
            "psb_geometry_section",
            Exactness::Unknown,
        );
        ir.push_native_unknown(
            "creo",
            UnknownRecord {
                id,
                offset: section.offset as u64,
                byte_len: bytes.len() as u64,
                sha256: sha256_hex(bytes),
                data: Some(bytes.to_vec()),
                links: Vec::new(),
            },
        )?;
    }
    for plane in &scan.datum_planes {
        let id = SurfaceId(format!("creo:actdatums:surface#{}", plane.id));
        annotate(
            &mut annotations,
            &id,
            "ActDatums",
            plane.offset_in_payload as u64,
            "datum_plane_outline",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(
                    plane.normal[0] * plane.offset,
                    plane.normal[1] * plane.offset,
                    plane.normal[2] * plane.offset,
                ),
                normal: Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                u_axis: cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                    plane.normal[0],
                    plane.normal[1],
                    plane.normal[2],
                )),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("ActDatums:{}", plane.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    for frame in &scan.plane_local_systems {
        let (Some(origin), Some(normal), Some(u_axis)) = (frame.origin, frame.normal, frame.u_axis)
        else {
            continue;
        };
        if is_axis_aligned(normal) {
            continue;
        }
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", frame.surface_id));
        annotate(
            &mut annotations,
            &id,
            "VisibGeom",
            frame.offset as u64,
            "plane_local_system",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", frame.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    for plane in &scan.outline_planes {
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", plane.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            &mut annotations,
            &id,
            "VisibGeom",
            plane.offset as u64,
            "plane_outline_held_coordinate",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                normal: Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                u_axis: Vector3::new(plane.u_axis[0], plane.u_axis[1], plane.u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", plane.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    transfer_fc05_cap_circles(scan, &mut ir, &mut annotations);
    transfer_cap_pair_cylinders(scan, &mut ir, &mut annotations);
    transfer_plane_intersection_curves(scan, &mut ir, &mut annotations);
    transfer_plane_brep(scan, &mut ir, &mut annotations);
    transfer_resolved_sketches(scan, &mut ir, &mut annotations);
    let feature_revolution_surface_count =
        transfer_resolved_revolution_surfaces(scan, &mut ir, &mut annotations);
    let feature_extrusion_surface_count =
        transfer_feature_extrusion_surfaces(scan, &mut ir, &mut annotations);
    let feature_extrusion_brep_count =
        transfer_resolved_extrusion_breps(scan, &mut ir, &mut annotations);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "transferred_feature_revolution_surface_count".to_string(),
            feature_revolution_surface_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_extrusion_surface_count".to_string(),
            feature_extrusion_surface_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_extrusion_brep_count".to_string(),
            feature_extrusion_brep_count.to_string(),
        );
    }
    for datum in &scan.datum_planes {
        let id = IrFeatureId(format!("creo:model:feature#{}", datum.feature_id));
        if ir.model.features.iter().any(|feature| feature.id == id) {
            continue;
        }
        annotate(
            &mut annotations,
            &id,
            "ActDatums",
            datum.offset_in_payload as u64,
            "datum_plane_feature",
            Exactness::Derived,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: IrFeatureDefinition::DatumPlane {
                origin: Point3::new(
                    datum.normal[0] * datum.offset,
                    datum.normal[1] * datum.offset,
                    datum.normal[2] * datum.offset,
                ),
                normal: Vector3::new(datum.normal[0], datum.normal[1], datum.normal[2]),
                u_axis: cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                    datum.normal[0],
                    datum.normal[1],
                    datum.normal[2],
                )),
            },
            native_ref: None,
        });
    }
    let operation_ordinal_base = ir.model.features.len();
    for (operation_index, operation) in scan.feature_operations.iter().enumerate() {
        let id = IrFeatureId(format!("creo:model:feature#{}", operation.feature_id));
        let outputs = feature_output_bodies(scan, &ir, operation.feature_id);
        let mut source_properties = feature_source_properties(scan, operation.feature_id);
        if let Some(prefix) = operation.status_prefix {
            source_properties.insert(
                "mdl_status_prefix".to_string(),
                char::from(prefix).to_string(),
            );
        }
        let parameters = feature_parameters(scan, operation.feature_id);
        let schema_class = scan
            .feature_rows
            .iter()
            .find(|row| row.feature_id == operation.feature_id)
            .and_then(|row| row.root_schema_class)
            .or(operation.root_schema_class);
        let definition = schema_class.map_or_else(
            || IrFeatureDefinition::Native {
                kind: operation.kind.clone(),
                parameters: parameters.clone(),
                properties: BTreeMap::new(),
            },
            |schema_class| {
                schema_feature_definition(
                    scan,
                    &ir,
                    operation.feature_id,
                    schema_class,
                    &operation.kind,
                )
            },
        );
        retain_native_feature_parameters(&mut source_properties, &definition, &parameters);
        let dependencies = feature_dependencies(scan, &ir, operation.feature_id);
        annotate(
            &mut annotations,
            &id,
            "MdlStatus",
            operation.offset as u64,
            "feature_operation_name",
            Exactness::ByteExact,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: (operation_ordinal_base + operation_index) as u64,
            name: Some(format!("{} id {}", operation.kind, operation.feature_id)),
            suppressed: false,
            parent: None,
            dependencies,
            source_properties,
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition,
            native_ref: None,
        });
    }
    for row in &scan.feature_rows {
        let id = IrFeatureId(format!("creo:model:feature#{}", row.feature_id));
        if ir.model.features.iter().any(|feature| feature.id == id)
            || !scan
                .surface_rows
                .iter()
                .any(|surface| surface.feature_id == row.feature_id)
        {
            continue;
        }
        let Some(schema_class) = row.root_schema_class else {
            continue;
        };
        let Some(kind) = schema_operation_kind(schema_class) else {
            continue;
        };
        annotate(
            &mut annotations,
            &id,
            "AllFeatur",
            row.offset as u64,
            "schema_feature_operation",
            Exactness::ByteExact,
        );
        let definition = schema_feature_definition(scan, &ir, row.feature_id, schema_class, kind);
        let parameters = feature_parameters(scan, row.feature_id);
        let mut source_properties = feature_source_properties(scan, row.feature_id);
        retain_native_feature_parameters(&mut source_properties, &definition, &parameters);
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: Some(format!("{kind} id {}", row.feature_id)),
            suppressed: false,
            parent: None,
            dependencies: feature_dependencies(scan, &ir, row.feature_id),
            source_properties,
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: feature_output_bodies(scan, &ir, row.feature_id),
            definition,
            native_ref: None,
        });
    }
    transfer_curve_expression_features(scan, &mut ir, &mut annotations);
    transfer_feature_dimensions(scan, &mut ir, &mut annotations);
    let sketches = sketch_records(scan);
    if !sketches.is_empty() {
        for sketch in &sketches {
            let offset = scan
                .feature_definitions
                .iter()
                .find(|definition| definition.id == sketch.definition_id)
                .map_or(0, |definition| definition.offset);
            annotate(
                &mut annotations,
                &sketch.id,
                "FeatDefs",
                offset as u64,
                "feature_sketch",
                Exactness::Derived,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("sketches", &sketches)?;
    }
    let curve_expressions = curve_expression_records(scan);
    if !curve_expressions.is_empty() {
        for (expression, source) in curve_expressions.iter().zip(&scan.curve_expressions) {
            annotate(
                &mut annotations,
                &expression.id,
                "DEPDB_DATA",
                source.expression_offset as u64,
                "curve_expression_program",
                Exactness::ByteExact,
            );
        }
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("curve_expressions", &curve_expressions)?;
    }
    ir.annotations = annotations.build();
    Ok(ir)
}

fn annotate(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    source_stream: &str,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) {
    let stream = annotations.stream(format!("creo:{source_stream}"));
    annotations.note(id.to_string(), stream, offset).tag(tag);
    annotations.exactness(id, exactness);
}

fn source_meta(scan: &ContainerScan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert("version_line".to_string(), scan.version_line.clone());
    attributes.insert("layout".to_string(), scan.layout.token().to_string());
    attributes.insert("file_size".to_string(), scan.data.len().to_string());
    attributes.insert("section_count".to_string(), scan.sections.len().to_string());
    if let Some(c) = scan.census.srf_array_count {
        attributes.insert("srf_array_count".to_string(), c.to_string());
    }
    if let Some(c) = scan.census.crv_array_count {
        attributes.insert("crv_array_count".to_string(), c.to_string());
    }
    if let Some(unit) = &scan.principal_unit {
        attributes.insert("principal_unit".to_string(), unit.clone());
    }
    attributes.insert(
        "decoded_surface_row_count".to_string(),
        scan.surface_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_surface_parameter_record_count".to_string(),
        scan.surface_parameters.len().to_string(),
    );
    attributes.insert(
        "decoded_plane_local_system_count".to_string(),
        scan.plane_local_systems.len().to_string(),
    );
    attributes.insert(
        "decoded_plane_envelope_count".to_string(),
        scan.plane_envelopes.len().to_string(),
    );
    attributes.insert(
        "decoded_outline_plane_count".to_string(),
        scan.outline_planes.len().to_string(),
    );
    attributes.insert(
        "decoded_surface_prototype_count".to_string(),
        scan.surface_prototypes.len().to_string(),
    );
    attributes.insert(
        "decoded_named_surface_prototype_count".to_string(),
        scan.surface_prototype_records.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_prototype_count".to_string(),
        scan.curve_prototypes.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_parameter_record_count".to_string(),
        scan.curve_parameters.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_expression_record_count".to_string(),
        scan.curve_expressions.len().to_string(),
    );
    if let Some(family_table) = scan.family_table {
        attributes.insert(
            "family_table_pointer".to_string(),
            match family_table.pointer {
                crate::container::FamilyTablePointer::Null => "null".to_string(),
                crate::container::FamilyTablePointer::Entity(id) => format!("entity:{id}"),
            },
        );
    }
    attributes.insert(
        "decoded_pcurve_count".to_string(),
        scan.pcurves.len().to_string(),
    );
    attributes.insert(
        "decoded_fc_curve_control_point_record_count".to_string(),
        scan.fc_curve_control_points.len().to_string(),
    );
    attributes.insert(
        "decoded_fc05_circle_count".to_string(),
        scan.fc05_circles.len().to_string(),
    );
    attributes.insert(
        "decoded_fc05_cylinder_cap_pair_count".to_string(),
        scan.fc05_cylinder_cap_pairs.len().to_string(),
    );
    attributes.insert(
        "decoded_prototype_pcurve_count".to_string(),
        scan.prototype_pcurves.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_prototype_topology_count".to_string(),
        scan.curve_prototype_topology.len().to_string(),
    );
    attributes.insert(
        "decoded_bound_prototype_pcurve_count".to_string(),
        scan.bound_prototype_pcurves.len().to_string(),
    );
    attributes.insert(
        "decoded_curve_topology_row_count".to_string(),
        scan.curve_topology_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_half_edge_count".to_string(),
        scan.half_edges.len().to_string(),
    );
    attributes.insert(
        "decoded_topological_vertex_count".to_string(),
        scan.topological_vertices.len().to_string(),
    );
    attributes.insert(
        "decoded_loop_count".to_string(),
        scan.loops.len().to_string(),
    );
    attributes.insert(
        "decoded_face_component_count".to_string(),
        scan.face_components.len().to_string(),
    );
    attributes.insert(
        "decoded_datum_plane_count".to_string(),
        scan.datum_planes.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_count".to_string(),
        scan.feature_ids.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_row_count".to_string(),
        scan.feature_rows.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_choice_count".to_string(),
        scan.feature_choices.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_choice_field_count".to_string(),
        scan.feature_choice_fields.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_geometry_table_count".to_string(),
        scan.feature_geometry_tables.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_affected_id_array_count".to_string(),
        scan.feature_affected_ids.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_replay_affected_id_count".to_string(),
        scan.feature_replay_affected_ids.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_direction_byte_count".to_string(),
        scan.feature_direction_bytes.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_definition_count".to_string(),
        scan.feature_definitions.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_section_transform_count".to_string(),
        scan.feature_section_transforms.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_operation_count".to_string(),
        scan.feature_operations.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_outline_count".to_string(),
        scan.feature_definitions
            .iter()
            .map(|definition| definition.outlines.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_section_point_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.variables.as_ref())
            .map(|variables| variables.points.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_segment_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.segments.as_ref())
            .map(|segments| segments.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_trim_entity_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.trim_entities.as_ref())
            .map(|entities| entities.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_trim_vertex_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.trim_vertices.as_ref())
            .map(|vertices| vertices.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_order_entry_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.order_table.as_ref())
            .map(|order| order.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_dimension_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.dimensions.as_ref())
            .map(|dimensions| dimensions.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_relation_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.relations.as_ref())
            .map(|relations| relations.rows.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_saved_entity_count".to_string(),
        scan.feature_definitions
            .iter()
            .filter_map(|definition| definition.saved_section.as_ref())
            .map(|saved| saved.entities.len())
            .sum::<usize>()
            .to_string(),
    );
    attributes.insert(
        "decoded_feature_entity_count".to_string(),
        scan.feature_entities.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_entity_reference_count".to_string(),
        scan.feature_entity_references.len().to_string(),
    );
    attributes.insert(
        "decoded_feature_entity_table_count".to_string(),
        scan.feature_entity_tables.len().to_string(),
    );
    if let Some(count) = scan.declared_body_count {
        attributes.insert("declared_body_count".to_string(), count.to_string());
    }
    if let Some(value) = scan.first_quilt_ptr {
        attributes.insert("first_quilt_ptr".to_string(), value.to_string());
    }
    SourceMeta {
        format: "creo".to_string(),
        attributes,
    }
}

/// Build diagnostics for data that cannot be represented in the emitted IR.
fn build_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let geom_sections = scan
        .sections
        .iter()
        .filter(|s| s.role == role::GEOMETRY)
        .count();
    let mut placed_plane_ids = scan
        .plane_local_systems
        .iter()
        .filter(|frame| {
            frame.origin.is_some()
                && frame.u_axis.is_some()
                && frame.normal.is_some_and(|normal| !is_axis_aligned(normal))
        })
        .map(|frame| frame.surface_id)
        .collect::<BTreeSet<_>>();
    placed_plane_ids.extend(scan.outline_planes.iter().map(|plane| plane.surface_id));
    let placed_plane_count = placed_plane_ids.len();

    let mut losses = Vec::new();

    if container_only {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: "Container-only decode requested; only the container layer was read."
                .to_string(),
            provenance: None,
        });
    }

    // The namespace census: what is byte-backed and readable.
    let srf = scan
        .census
        .srf_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    let crv = scan
        .census
        .crv_array_count
        .map_or_else(|| "n/a".to_string(), |c| c.to_string());
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "PSB container decoded structurally: {} section(s), {} layout, VisibGeom namespace \
             census srf_array={srf} / crv_array={crv}; {} typed surface rows, {} labeled curve \
             prototypes, {} canonical curve-topology rows, and {} closed native loops were decoded. \
             Outline-backed planes, guarded non-axis support frames, and topology-bound `fc 05` \
             cylinders with a resolved model-X or model-Y cap plane transfer as carriers; other parameter bodies remain \
             structural records.",
            scan.sections.len(),
            scan.layout.token(),
            scan.surface_rows.len(),
            scan.curve_prototypes.len(),
            scan.curve_topology_rows.len(),
            scan.loops.len(),
        ),
        provenance: None,
    });

    // The core prototype-vs-instance limitation.
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: format!(
            "General model B-rep transfer remains incomplete. Exact single-loop plane components \
             transfer when their vertices are determined by placed-plane intersections. Selected \
             cylinders transfer when an exact `fc 05` record and placed cap outline establish the \
             complete axis placement, parameterization, and radius. VisibGeom \
             stores one surface prototype per family (a first-instance template), not per-instance \
             located geometry, so other prototype scalars cannot be emitted as model surfaces \
             without mislabeling most instances \
             ([spec §4.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#32-surface-prototypes)). {geom_sections} PSB geometry section(s) were preserved verbatim as unknown \
             records."
        ),
        provenance: None,
    });

    if placed_plane_count != 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {placed_plane_count} model-space plane carrier(s) from complete \
                 VisibGeom local-system support frames."
            ),
            provenance: None,
        });
    }

    if !scan.datum_planes.is_empty() {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "Transferred {} exact model-space construction datum plane carrier(s) from ActDatums; \
                 these are unbounded reference planes, not model B-rep faces.",
                scan.datum_planes.len()
            ),
            provenance: None,
        });
    }

    // The specific undecoded PSB layers that gate per-instance geometry.
    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: "Additional model-space carriers are gated by unresolved lane-specific scalar \
                  prefixes, feature-local transform bindings, the `0x26` per-instance torus/sphere \
                  override region, and the round/fillet feature evaluator. These gaps prevent \
                  transfer of the remaining non-plane per-instance surfaces, curves, and vertices."
            .to_string(),
        provenance: None,
    });

    // Topology.
    losses.push(LossNote {
        category: LossCategory::Topology,
        severity: Severity::Blocking,
        message: "Native curve half-edges and closed loops were decoded. Exact plane-intersection \
                  components transfer as body/region/shell/face/loop/coedge/edge/vertex graphs; \
                  remaining components require face-instance partitioning, surface parameter \
                  bindings, curve geometry, or vertex coordinates."
            .to_string(),
        provenance: None,
    });

    // Features, history, materials.
    losses.push(LossNote {
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: "Named feature operations and their decoded dependency/input tables transfer as \
                  native design records. Curve-equation assignments transfer with their source, \
                  dependencies, and closed arithmetic values. Full neutral operation semantics, \
                  configurations, remaining expression families, materials, and display data \
                  remain untransferred."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "creo".to_string(),
        container_only,
        geometry_transferred: !scan.datum_planes.is_empty() || placed_plane_count != 0,
        losses,
        notes: summary.notes,
    }
}
