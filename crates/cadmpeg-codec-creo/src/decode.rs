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
    Angle, DesignParameter, Feature, FeatureDefinition as IrFeatureDefinition,
    FeatureId as IrFeatureId, Length, ParameterId, ParameterValue,
};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    UnknownId, VertexId,
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
    sign: u32,
    dimension_id: u32,
    relation_type: u32,
    body: Vec<u8>,
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
                    sign: relation.sign,
                    dimension_id: relation.dimension_id,
                    relation_type: relation.relation_type,
                    body: relation.body.clone(),
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
    section_line_geometry(points, segment).or_else(|| section_arc_geometry(points, segment))
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

fn normalized(vector: [f64; 3]) -> Option<[f64; 3]> {
    let magnitude = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    (magnitude > 1e-12).then(|| vector.map(|value| value / magnitude))
}

fn extruded_segment_surface(
    transform: &crate::placement::FeatureSectionTransform,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
) -> Option<SurfaceGeometry> {
    match section_segment_geometry(points, segment)? {
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
        let (Some(variables), Some(segments), Some(order_table), Some(trim_entities)) = (
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
        let points = variables
            .points
            .iter()
            .filter_map(|point| Some((point.point_id, [point.u?, point.v?])))
            .collect::<BTreeMap<_, _>>();
        let solved = trim_entities
            .solved_external_ids
            .iter()
            .copied()
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
            };
            let Some(surface_id) = surface_id else {
                continue;
            };
            let id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            let Some(geometry) = extruded_segment_surface(transform, &points, segment) else {
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
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, row) in table.rows.iter().enumerate() {
        for vertex in row.vertices {
            incident.entry(vertex).or_default().push(index);
        }
    }
    if incident.values().any(|rows| rows.len() > 2) {
        return Vec::new();
    }
    let mut remaining = (0..table.rows.len()).collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut frontier = vec![seed];
        while let Some(index) = frontier.pop() {
            for vertex in table.rows[index].vertices {
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
            .any(|index| !emitted.contains(&table.rows[*index].external_id))
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
            .min_by_key(|index| table.rows[**index].external_id)
            .copied()
            .expect("component contains seed");
        let mut vertex = endpoints
            .iter()
            .min()
            .copied()
            .unwrap_or(table.rows[first_row].vertices[0]);
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
            let row = &table.rows[index];
            let reversed = row.vertices[1] == vertex;
            if !reversed && row.vertices[0] != vertex {
                break;
            }
            profile.push(SketchEntityUse {
                entity: SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, row.external_id
                )),
                reversed,
            });
            vertex = if reversed {
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
        let (Some(variables), Some(segments)) = (&definition.variables, &definition.segments)
        else {
            continue;
        };
        let points = variables
            .points
            .iter()
            .filter_map(|point| Some((point.point_id, [point.u?, point.v?])))
            .collect::<BTreeMap<_, _>>();
        let solved = definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.solved_external_ids)
            .copied()
            .collect::<BTreeSet<_>>();
        let sketch_id = SketchId(format!("creo:model:sketch#{}", definition.id));
        let entities = segments
            .rows
            .iter()
            .filter_map(|segment| {
                let geometry = section_segment_geometry(&points, segment)?;
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
                    },
                    Exactness::Derived,
                );
                Some(SketchEntity {
                    id,
                    sketch: sketch_id.clone(),
                    construction: !solved.contains(&segment.external_id),
                    native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                    geometry_ref: None,
                    endpoint_refs: if segment.kind == crate::feature::FeatureSegmentKind::Arc {
                        [segment.point_ids[1], segment.point_ids[0]]
                    } else {
                        segment.point_ids
                    }
                    .iter()
                    .map(|point| format!("creo:featdefs:sketch#{}:point#{point}", definition.id))
                    .collect(),
                    geometry,
                })
            })
            .collect::<Vec<_>>();
        if entities.is_empty() {
            continue;
        }
        let emitted = segments
            .rows
            .iter()
            .filter(|segment| section_segment_geometry(&points, segment).is_some())
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

#[cfg(test)]
mod resolved_sketch_tests {
    use super::*;

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
        let (origin, axis, ref_direction) = if axis_index == 0 {
            (
                [axis_origin, second, first],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            )
        } else {
            (
                [first, axis_origin, second],
                [0.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
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

fn transfer_plane_intersection_vertices(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let planes = placed_planes(scan);
    let half_edges = scan
        .half_edges
        .iter()
        .map(|half_edge| (half_edge.id, half_edge))
        .collect::<BTreeMap<_, _>>();
    for vertex in &scan.topological_vertices {
        let incident = vertex
            .half_edges
            .iter()
            .filter_map(|half_edge| half_edges.get(half_edge))
            .filter_map(|half_edge| planes.get(&half_edge.face_id))
            .copied()
            .collect::<Vec<_>>();
        let Some(position) = solve_planes(&incident) else {
            continue;
        };
        let point_id = PointId(format!("creo:visibgeom:point#{}", vertex.id));
        let vertex_id = VertexId(format!("creo:visibgeom:vertex#{}", vertex.id));
        if ir.model.vertices.iter().any(|item| item.id == vertex_id) {
            continue;
        }
        annotate(
            annotations,
            &point_id,
            "VisibGeom",
            0,
            "placed_plane_intersection_point",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &vertex_id,
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
            id: vertex_id,
            point: point_id,
            tolerance: None,
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
    transfer_cap_pair_cylinders(scan, &mut ir, &mut annotations);
    transfer_plane_intersection_curves(scan, &mut ir, &mut annotations);
    transfer_plane_intersection_vertices(scan, &mut ir, &mut annotations);
    transfer_plane_brep(scan, &mut ir, &mut annotations);
    transfer_resolved_sketches(scan, &mut ir, &mut annotations);
    let feature_extrusion_surface_count =
        transfer_feature_extrusion_surfaces(scan, &mut ir, &mut annotations);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "transferred_feature_extrusion_surface_count".to_string(),
            feature_extrusion_surface_count.to_string(),
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
            definition: IrFeatureDefinition::Native {
                kind: operation.kind.clone(),
                parameters,
                properties: BTreeMap::new(),
            },
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
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: Some(format!("{kind} id {}", row.feature_id)),
            suppressed: false,
            parent: None,
            dependencies: feature_dependencies(scan, &ir, row.feature_id),
            source_properties: feature_source_properties(scan, row.feature_id),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: feature_output_bodies(scan, &ir, row.feature_id),
            definition: IrFeatureDefinition::Native {
                kind: kind.to_string(),
                parameters: feature_parameters(scan, row.feature_id),
                properties: BTreeMap::new(),
            },
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
             cylinders with a placed cap transfer as carriers; other parameter bodies remain \
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
             cylinders transfer when an exact `fc 05` cap pair and placed cap outline establish the \
             complete equation. VisibGeom \
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
