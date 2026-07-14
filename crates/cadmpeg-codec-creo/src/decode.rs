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
    Angle, BooleanOp, ChamferSpec, DesignParameter, DimensionDisplay, EdgeSelection, Extent,
    FaceSelection, Feature, FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId,
    FeatureSourceContent, FeatureTreeNodeRole, HoleKind, Length, ParameterId, ParameterValue,
    PatternForm, PatternKind, ProfileRef, RadiusSpec, RevolutionAxis, RevolutionConstruction,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus, SketchNativeOperand,
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
    trim_entities: Vec<CreoSketchTrimEntity>,
    trim_vertices: Vec<CreoSketchTrimVertex>,
    order_rows: Vec<CreoSketchOrderRow>,
    saved_entities: Vec<CreoSketchSavedEntity>,
    dimensions: Vec<CreoSketchDimension>,
    relations: Vec<CreoSketchRelation>,
    skamps: Vec<CreoSketchSkamp>,
    relation_triples: Vec<CreoSketchRelationTriple>,
}

#[derive(Serialize)]
struct CreoSketchTrimEntity {
    external_id: u32,
    mode: Option<u32>,
    vertices: [u32; 2],
    center_vertex: Option<u32>,
    kind: &'static str,
}

#[derive(Serialize)]
struct CreoSketchTrimVertex {
    vertex_id: u32,
    entities: [u32; 2],
    section_coordinates: Option<[f64; 2]>,
}

#[derive(Serialize)]
struct CreoSketchOrderRow {
    external_id: u32,
    internal_id: u32,
    bitmask: u32,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CreoSketchSavedEntity {
    Line {
        entity_id: u32,
        references: Vec<u32>,
        attributes: Vec<[u8; 5]>,
        endpoints: [[Option<f64>; 3]; 2],
    },
    Arc {
        entity_id: u32,
        center: [Option<f64>; 3],
        radius: Option<f64>,
        endpoints: [[Option<f64>; 3]; 2],
        parameters: [Option<f64>; 2],
    },
    Circle {
        entity_id: u32,
        center: [Option<f64>; 3],
        radius: Option<f64>,
    },
    Spline {
        entity_id: Option<u32>,
        interpolation_points: Vec<[f64; 3]>,
        endpoint_tangents: Option<[[f64; 3]; 2]>,
        parameters: Option<Vec<f64>>,
    },
    Dummy {
        entity_id: Option<u32>,
    },
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
    explicit_slots: Option<[f64; 12]>,
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

#[derive(Serialize)]
struct CreoPcurveEndpointRecord {
    id: String,
    curve_id: u32,
    faces: [u32; 2],
    face_0_endpoints: [[f64; 2]; 2],
    face_1_endpoints: [[f64; 2]; 2],
    source_form: &'static str,
}

fn pcurve_endpoint_records(scan: &ContainerScan) -> Vec<(CreoPcurveEndpointRecord, usize)> {
    let mut records = scan
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                CreoPcurveEndpointRecord {
                    id: format!("creo:visibgeom:pcurve_endpoints#{}", pcurve.curve_id),
                    curve_id: pcurve.curve_id,
                    faces: pcurve.faces,
                    face_0_endpoints: pcurve.face_0_endpoints,
                    face_1_endpoints: pcurve.face_1_endpoints,
                    source_form: "positional",
                },
                pcurve.offset,
            )
        })
        .collect::<Vec<_>>();
    records.extend(scan.bound_prototype_pcurves.iter().map(|pcurve| {
        (
            CreoPcurveEndpointRecord {
                id: format!(
                    "creo:visibgeom:prototype_pcurve_endpoints#{}",
                    pcurve.curve_id
                ),
                curve_id: pcurve.curve_id,
                faces: pcurve.faces,
                face_0_endpoints: pcurve.face_0_endpoints,
                face_1_endpoints: pcurve.face_1_endpoints,
                source_form: "prototype",
            },
            pcurve.offset,
        )
    }));
    records.sort_by_key(|(_, offset)| *offset);
    records
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
                    explicit_slots: frame.explicit_slots,
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

fn curve_expression_helix_definition(
    record: &crate::curve::CurveExpressionRecord,
) -> Option<ProceduralCurveDefinition> {
    let helix = crate::curve::expression_helix(record)?;
    let slots = record.local_system.as_ref()?.explicit_slots?;
    let u = Vector3::new(slots[0], slots[1], slots[2]);
    let v = Vector3::new(slots[6], slots[7], slots[8]);
    let u_norm = u.norm();
    let v_norm = v.norm();
    let scale = u_norm.max(v_norm).max(1.0);
    if !u_norm.is_finite()
        || !v_norm.is_finite()
        || u_norm <= 1e-12
        || v_norm <= 1e-12
        || (u_norm - v_norm).abs() > 1e-9 * scale
        || (u.x * v.x + u.y * v.y + u.z * v.z).abs() > 1e-9 * u_norm * v_norm
        || slots[3..6].iter().any(|value| value.abs() > 1e-12)
    {
        return None;
    }
    let u = Vector3::new(u.x / u_norm, u.y / u_norm, u.z / u_norm);
    let v = Vector3::new(v.x / v_norm, v.y / v_norm, v.z / v_norm);
    let axis = Vector3::new(
        u.y * v.z - u.z * v.y,
        u.z * v.x - u.x * v.z,
        u.x * v.y - u.y * v.x,
    );
    let origin = Point3::new(slots[9], slots[10], slots[11]);
    let (sin, cos) = helix.start_angle.sin_cos();
    let major_direction = Vector3::new(
        u.x * cos + v.x * sin,
        u.y * cos + v.y * sin,
        u.z * cos + v.z * sin,
    );
    let tangent_direction = Vector3::new(
        -u.x * sin + v.x * cos,
        -u.y * sin + v.y * cos,
        -u.z * sin + v.z * cos,
    );
    let minor_direction = if helix.clockwise {
        Vector3::new(
            -tangent_direction.x,
            -tangent_direction.y,
            -tangent_direction.z,
        )
    } else {
        tangent_direction
    };
    Some(ProceduralCurveDefinition::Helix {
        angle_range: [0.0, helix.revolutions * std::f64::consts::TAU],
        center: Point3::new(
            origin.x + axis.x * helix.z_start,
            origin.y + axis.y * helix.z_start,
            origin.z + axis.z * helix.z_start,
        ),
        major: Vector3::new(
            major_direction.x * helix.radius,
            major_direction.y * helix.radius,
            major_direction.z * helix.radius,
        ),
        minor: Vector3::new(
            minor_direction.x * helix.radius,
            minor_direction.y * helix.radius,
            minor_direction.z * helix.radius,
        ),
        pitch: Vector3::new(
            axis.x * helix.height / helix.revolutions,
            axis.y * helix.height / helix.revolutions,
            axis.z * helix.height / helix.revolutions,
        ),
        apex_factor: 0.0,
        axis,
    })
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
        let mut source_content = Vec::with_capacity(record.assignments.len());
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
                .filter(|name| name.as_str() != "t" && !parameters_by_name.contains_key(*name))
                .cloned()
                .collect::<Vec<_>>();
            let intrinsic_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| name.as_str() == "t")
                .cloned()
                .collect::<Vec<_>>();
            let mut properties = BTreeMap::new();
            if !external_dependencies.is_empty() {
                properties.insert(
                    "external_dependencies".to_string(),
                    external_dependencies.join(","),
                );
            }
            if !intrinsic_dependencies.is_empty() {
                properties.insert(
                    "independent_variables".to_string(),
                    intrinsic_dependencies.join(","),
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
            source_content.push(FeatureSourceContent::Parameter(parameter_id.clone()));
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
        let helix = crate::curve::expression_helix(record);
        let placed_helix = curve_expression_helix_definition(record);
        if let Some(procedural_definition) = placed_helix {
            let curve_id = CurveId(format!(
                "creo:depdb:curve_expression_curve#{}-{}",
                record.entity_id, record.offset
            ));
            let procedural_id = ProceduralCurveId(format!(
                "creo:depdb:curve_expression_helix#{}-{}",
                record.entity_id, record.offset
            ));
            annotate(
                annotations,
                &curve_id.0,
                "DEPDB_DATA",
                record.offset as u64,
                "curve_expression_carrier",
                Exactness::Unknown,
            );
            annotate(
                annotations,
                &procedural_id.0,
                "DEPDB_DATA",
                record.offset as u64,
                "curve_expression_helix",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Unknown { record: None },
                source_object: None,
            });
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: curve_id,
                definition: procedural_definition,
                cache_fit_tolerance: None,
            });
        }
        let definition = helix.map_or_else(
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
            source_content,
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
                || definition.trim_entities.is_some()
                || definition.trim_vertices.is_some()
                || definition.order_table.is_some()
                || definition.saved_section.is_some()
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
            trim_entities: definition
                .trim_entities
                .iter()
                .flat_map(|table| &table.rows)
                .map(|entity| CreoSketchTrimEntity {
                    external_id: entity.external_id,
                    mode: entity.mode,
                    vertices: entity.vertices,
                    center_vertex: entity.center_vertex,
                    kind: match entity.kind {
                        crate::feature::TrimEntityKind::Line => "line",
                        crate::feature::TrimEntityKind::Arc => "arc",
                    },
                })
                .collect(),
            trim_vertices: definition
                .trim_vertices
                .iter()
                .flat_map(|table| &table.rows)
                .map(|vertex| CreoSketchTrimVertex {
                    vertex_id: vertex.vertex_id,
                    entities: vertex.entities,
                    section_coordinates: vertex.section_coordinates,
                })
                .collect(),
            order_rows: definition
                .order_table
                .iter()
                .flat_map(|table| &table.rows)
                .map(|row| CreoSketchOrderRow {
                    external_id: row.external_id,
                    internal_id: row.internal_id,
                    bitmask: row.bitmask,
                })
                .collect(),
            saved_entities: definition
                .saved_section
                .iter()
                .flat_map(|section| &section.entities)
                .map(|entity| match entity {
                    crate::feature::FeatureSavedEntity::Line(line) => CreoSketchSavedEntity::Line {
                        entity_id: line.entity_id,
                        references: line.references.clone(),
                        attributes: line.attributes.clone(),
                        endpoints: line.endpoints,
                    },
                    crate::feature::FeatureSavedEntity::Arc(arc) => CreoSketchSavedEntity::Arc {
                        entity_id: arc.entity_id,
                        center: arc.center,
                        radius: arc.radius,
                        endpoints: arc.endpoints,
                        parameters: arc.parameters,
                    },
                    crate::feature::FeatureSavedEntity::Circle(circle) => {
                        CreoSketchSavedEntity::Circle {
                            entity_id: circle.entity_id,
                            center: circle.center,
                            radius: circle.radius,
                        }
                    }
                    crate::feature::FeatureSavedEntity::Spline(spline) => {
                        CreoSketchSavedEntity::Spline {
                            entity_id: spline.entity_id,
                            interpolation_points: spline.interpolation_points.clone(),
                            endpoint_tangents: spline.endpoint_tangents,
                            parameters: spline.parameters.clone(),
                        }
                    }
                    crate::feature::FeatureSavedEntity::Dummy(dummy) => {
                        CreoSketchSavedEntity::Dummy {
                            entity_id: dummy.entity_id,
                        }
                    }
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

fn saved_section_entity_geometry(
    entity: &crate::feature::FeatureSavedEntity,
) -> Option<(u32, SketchGeometry, usize)> {
    match entity {
        crate::feature::FeatureSavedEntity::Line(line) => {
            let [[Some(start_u), Some(start_v), _], [Some(end_u), Some(end_v), _]] = line.endpoints
            else {
                return None;
            };
            Some((
                line.entity_id,
                SketchGeometry::Line {
                    start: Point2::new(start_u, start_v),
                    end: Point2::new(end_u, end_v),
                },
                line.offset,
            ))
        }
        crate::feature::FeatureSavedEntity::Arc(arc) => {
            let ([Some(center_u), Some(center_v)], Some(radius)) = (
                [arc.center[0], arc.center[1]],
                arc.radius.filter(|radius| *radius > 1e-12),
            ) else {
                return None;
            };
            let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] =
                arc.endpoints
            else {
                return None;
            };
            let first = [first_u - center_u, first_v - center_v];
            let second = [second_u - center_u, second_v - center_v];
            let scale = radius
                .max(first[0].hypot(first[1]))
                .max(second[0].hypot(second[1]))
                .max(1.0);
            if (first[0].hypot(first[1]) - radius).abs() > 1e-9 * scale
                || (second[0].hypot(second[1]) - radius).abs() > 1e-9 * scale
            {
                return None;
            }
            let start_angle = second[1].atan2(second[0]);
            let mut end_angle = first[1].atan2(first[0]);
            while end_angle <= start_angle {
                end_angle += std::f64::consts::TAU;
            }
            Some((
                arc.entity_id,
                SketchGeometry::Arc {
                    center: Point2::new(center_u, center_v),
                    radius: Length(radius),
                    start_angle: Angle(start_angle),
                    end_angle: Angle(end_angle),
                },
                arc.offset,
            ))
        }
        crate::feature::FeatureSavedEntity::Circle(circle) => {
            let ([Some(center_u), Some(center_v)], Some(radius)) = (
                [circle.center[0], circle.center[1]],
                circle.radius.filter(|radius| *radius > 1e-12),
            ) else {
                return None;
            };
            Some((
                circle.entity_id,
                SketchGeometry::Arc {
                    center: Point2::new(center_u, center_v),
                    radius: Length(radius),
                    start_angle: Angle(0.0),
                    end_angle: Angle(std::f64::consts::TAU),
                },
                circle.offset,
            ))
        }
        crate::feature::FeatureSavedEntity::Spline(_)
        | crate::feature::FeatureSavedEntity::Dummy(_) => None,
    }
}

fn is_full_circle_geometry(geometry: &SketchGeometry) -> bool {
    matches!(
        geometry,
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } if (end_angle.0 - start_angle.0 - std::f64::consts::TAU).abs() <= 1e-12
    )
}

fn saved_geometry_endpoints(geometry: &SketchGeometry) -> Option<[[f64; 2]; 2]> {
    match geometry {
        SketchGeometry::Line { start, end } => Some([[start.u, start.v], [end.u, end.v]]),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } if !is_full_circle_geometry(geometry) => Some([
            [
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ],
            [
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ],
        ]),
        _ => None,
    }
}

fn saved_points_coincide(first: [f64; 2], second: [f64; 2]) -> bool {
    let scale = first
        .into_iter()
        .chain(second)
        .map(f64::abs)
        .fold(1.0, f64::max);
    first
        .into_iter()
        .zip(second)
        .all(|(left, right)| (left - right).abs() <= 1e-9 * scale)
}

fn saved_profile_chains(
    definition_id: u32,
    geometries: &[(u32, SketchGeometry)],
) -> Vec<Vec<SketchEntityUse>> {
    let mut profiles = geometries
        .iter()
        .filter(|(_, geometry)| is_full_circle_geometry(geometry))
        .map(|(external_id, _)| {
            vec![SketchEntityUse {
                entity: SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{definition_id}:{external_id}"
                )),
                reversed: false,
            }]
        })
        .collect::<Vec<_>>();
    let rows = geometries
        .iter()
        .filter_map(|(external_id, geometry)| {
            Some((*external_id, saved_geometry_endpoints(geometry)?))
        })
        .collect::<Vec<_>>();
    let mut mates = vec![[None; 2]; rows.len()];
    for (row_index, (_, endpoints)) in rows.iter().enumerate() {
        for endpoint_index in 0..2 {
            let matches = rows
                .iter()
                .enumerate()
                .flat_map(|(candidate_row, (_, candidate_endpoints))| {
                    (0..2).map(move |candidate_endpoint| {
                        (candidate_row, candidate_endpoint, candidate_endpoints)
                    })
                })
                .filter(|(candidate_row, candidate_endpoint, candidate_endpoints)| {
                    (*candidate_row != row_index || *candidate_endpoint != endpoint_index)
                        && saved_points_coincide(
                            endpoints[endpoint_index],
                            candidate_endpoints[*candidate_endpoint],
                        )
                })
                .map(|(candidate_row, candidate_endpoint, _)| (candidate_row, candidate_endpoint))
                .collect::<Vec<_>>();
            if let [mate] = matches.as_slice() {
                mates[row_index][endpoint_index] = Some(*mate);
            }
        }
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    while let Some(seed) = remaining
        .iter()
        .min_by_key(|index| rows[**index].0)
        .copied()
    {
        if mates[seed].iter().any(Option::is_none) {
            remaining.remove(&seed);
            continue;
        }
        let mut uses = Vec::new();
        let mut used = BTreeSet::new();
        let mut row = seed;
        let mut reversed = false;
        loop {
            if !used.insert(row) {
                break;
            }
            uses.push(SketchEntityUse {
                entity: SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{definition_id}:{}",
                    rows[row].0
                )),
                reversed,
            });
            let outgoing = usize::from(!reversed);
            let Some((next_row, next_endpoint)) = mates[row][outgoing] else {
                break;
            };
            row = next_row;
            reversed = next_endpoint == 1;
            if row == seed {
                if !reversed {
                    profiles.push(uses);
                }
                break;
            }
        }
        remaining.retain(|index| !used.contains(index));
    }
    profiles
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
    let coincident_point_pairs = definition
        .relations
        .iter()
        .flat_map(|table| &table.skamps)
        .filter_map(|skamp| {
            let [first, second] = skamp.items.as_slice() else {
                return None;
            };
            match skamp.kind {
                0 => Some([
                    section_skamp_endpoint_point_id(definition, first)?,
                    section_skamp_endpoint_point_id(definition, second)?,
                ]),
                3 => {
                    let first_point = section_skamp_point_entity_id(definition, first);
                    let second_point = section_skamp_point_entity_id(definition, second);
                    match (first_point, second_point) {
                        (Some(point), None) => {
                            Some([point, section_skamp_selected_point_id(definition, second)?])
                        }
                        (None, Some(point)) => {
                            Some([section_skamp_selected_point_id(definition, first)?, point])
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    let point_on_line_coordinates = definition
        .relations
        .iter()
        .flat_map(|table| &table.skamps)
        .filter_map(|skamp| section_skamp_point_on_line(definition, skamp))
        .collect::<Vec<_>>();
    let symmetric_point_constraints = definition
        .relations
        .iter()
        .flat_map(|table| &table.skamps)
        .filter_map(|skamp| section_skamp_axis_symmetry(definition, skamp))
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
            if !section_linear_distance_vectors(vectors) {
                return None;
            }
            let [Some(first), Some(second), _, _] = vectors[0] else {
                return None;
            };
            let segment = segments.iter().find(|segment| {
                segment.point_ids == [first, second] || segment.point_ids == [second, first]
            })?;
            let coordinate =
                1usize.checked_sub(section_line_fixed_coordinate(definition, segment)?)?;
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
                0 => match segment.directions[0] {
                    Some(1) => magnitude,
                    None => -magnitude,
                    _ => return None,
                },
                _ => return None,
            };
            Some((first, second, coordinate, delta))
        })
        .collect::<Vec<_>>();
    loop {
        let mut changed = false;
        for segment in &segments {
            let Some(coordinate) = section_line_fixed_coordinate(definition, segment) else {
                continue;
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
        for &[first_id, second_id] in &coincident_point_pairs {
            let [first, second] =
                [first_id, second_id].map(|id| points.get(&id).copied().unwrap_or([None, None]));
            for coordinate in 0..2 {
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
        }
        for &(line_point_id, point_id, coordinate) in &point_on_line_coordinates {
            let [line_point, point] = [line_point_id, point_id]
                .map(|id| points.get(&id).copied().unwrap_or([None, None]));
            match [line_point[coordinate], point[coordinate]] {
                [Some(value), None] => {
                    points.entry(point_id).or_insert([None, None])[coordinate] = Some(value);
                    changed = true;
                }
                [None, Some(value)] => {
                    points.entry(line_point_id).or_insert([None, None])[coordinate] = Some(value);
                    changed = true;
                }
                _ => {}
            }
        }
        for &(axis_point_id, first_id, second_id, fixed_coordinate) in &symmetric_point_constraints
        {
            let [axis, first, second] = [axis_point_id, first_id, second_id]
                .map(|id| points.get(&id).copied().unwrap_or([None, None]));
            let parallel_coordinate = 1usize.saturating_sub(fixed_coordinate);
            match [first[parallel_coordinate], second[parallel_coordinate]] {
                [Some(value), None] => {
                    points.entry(second_id).or_insert([None, None])[parallel_coordinate] =
                        Some(value);
                    changed = true;
                }
                [None, Some(value)] => {
                    points.entry(first_id).or_insert([None, None])[parallel_coordinate] =
                        Some(value);
                    changed = true;
                }
                _ => {}
            }
            let Some(axis_value) = axis[fixed_coordinate] else {
                continue;
            };
            match [first[fixed_coordinate], second[fixed_coordinate]] {
                [Some(value), None] => {
                    points.entry(second_id).or_insert([None, None])[fixed_coordinate] =
                        Some(2.0 * axis_value - value);
                    changed = true;
                }
                [None, Some(value)] => {
                    points.entry(first_id).or_insert([None, None])[fixed_coordinate] =
                        Some(2.0 * axis_value - value);
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

fn section_line_fixed_coordinate(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> Option<usize> {
    let segment = unique_section_skamp_segment(definition, segment.external_id)?;
    (segment.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    let mut adjacency = BTreeMap::<u32, Vec<(u32, usize)>>::new();
    for skamp in definition.relations.iter().flat_map(|table| &table.skamps) {
        let (parity, first, second) = match (skamp.kind, skamp.items.as_slice()) {
            (5 | 7, [first, second]) if first.sense == 0 && second.sense == 0 => {
                ((skamp.kind == 5) as usize, first, second)
            }
            _ => continue,
        };
        let Some(first_segment) = unique_section_skamp_segment(definition, first.entity_id) else {
            continue;
        };
        let Some(second_segment) = unique_section_skamp_segment(definition, second.entity_id)
        else {
            continue;
        };
        if first_segment.kind != crate::feature::FeatureSegmentKind::Line
            || second_segment.kind != crate::feature::FeatureSegmentKind::Line
        {
            continue;
        }
        adjacency
            .entry(first.entity_id)
            .or_default()
            .push((second.entity_id, parity));
        adjacency
            .entry(second.entity_id)
            .or_default()
            .push((first.entity_id, parity));
    }
    let mut parities = BTreeMap::from([(segment.external_id, 0usize)]);
    let mut pending = std::collections::VecDeque::from([segment.external_id]);
    while let Some(entity_id) = pending.pop_front() {
        let parity = parities[&entity_id];
        for &(neighbor, edge_parity) in adjacency.get(&entity_id).into_iter().flatten() {
            let neighbor_parity = parity ^ edge_parity;
            match parities.get(&neighbor) {
                Some(stored) if *stored != neighbor_parity => return None,
                Some(_) => {}
                None => {
                    parities.insert(neighbor, neighbor_parity);
                    pending.push_back(neighbor);
                }
            }
        }
    }
    let mut coordinates = BTreeSet::new();
    for (entity_id, parity) in parities {
        let related = unique_section_skamp_segment(definition, entity_id)?;
        coordinates.extend(
            section_line_direct_fixed_coordinates(definition, related)
                .into_iter()
                .map(|coordinate| coordinate ^ parity),
        );
    }
    coordinates
        .first()
        .copied()
        .filter(|_| coordinates.len() == 1)
}

fn section_line_direct_fixed_coordinates(
    definition: &crate::feature::FeatureDefinition,
    segment: &crate::feature::FeatureSegment,
) -> BTreeSet<usize> {
    let mut coordinates = segment
        .vertical_horizontal
        .and_then(|selector| match selector {
            0 => Some(0),
            1 => Some(1),
            _ => None,
        })
        .into_iter()
        .collect::<BTreeSet<_>>();
    coordinates.extend(
        definition
            .relations
            .iter()
            .flat_map(|table| &table.skamps)
            .filter_map(|skamp| match (skamp.kind, skamp.items.as_slice()) {
                (1, [item]) if item.sense == 0 && item.entity_id == segment.external_id => Some(1),
                (2, [item]) if item.sense == 0 && item.entity_id == segment.external_id => Some(0),
                _ => None,
            }),
    );
    coordinates
}

fn section_skamp_point_on_line(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, u32, usize)> {
    let [first, second] = skamp.items.as_slice() else {
        return None;
    };
    let pair = match skamp.kind {
        3 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                let line = unique_section_skamp_segment(definition, line_item.entity_id)?;
                (line_item.sense == 0 && line.kind == crate::feature::FeatureSegmentKind::Line)
                    .then_some((
                        line,
                        section_skamp_selected_point_id(definition, point_item)?,
                    ))
            }),
        9 => [(first, second), (second, first)]
            .into_iter()
            .find_map(|(line_item, point_item)| {
                let line = unique_section_skamp_segment(definition, line_item.entity_id)?;
                let point = unique_section_skamp_segment(definition, point_item.entity_id)?;
                (line_item.sense == 0
                    && point_item.sense == 0
                    && line.kind == crate::feature::FeatureSegmentKind::Line
                    && point.kind == crate::feature::FeatureSegmentKind::Point)
                    .then_some((line, point.point_ids[0]))
            }),
        _ => None,
    }?;
    let coordinate = section_line_fixed_coordinate(definition, pair.0)?;
    Some((pair.0.point_ids[0], pair.1, coordinate))
}

fn section_skamp_axis_symmetry(
    definition: &crate::feature::FeatureDefinition,
    skamp: &crate::feature::FeatureSkamp,
) -> Option<(u32, u32, u32, usize)> {
    let (14, [axis_item, first_item, second_item]) = (skamp.kind, skamp.items.as_slice()) else {
        return None;
    };
    let axis = unique_section_skamp_segment(definition, axis_item.entity_id)?;
    (axis_item.sense == 0 && axis.kind == crate::feature::FeatureSegmentKind::Line).then_some(())?;
    Some((
        axis.point_ids[0],
        section_skamp_selected_point_id(definition, first_item)?,
        section_skamp_selected_point_id(definition, second_item)?,
        section_line_fixed_coordinate(definition, axis)?,
    ))
}

fn section_skamp_endpoint_point_id(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<u32> {
    let segment = unique_section_skamp_segment(definition, item.entity_id)?;
    match item.sense {
        2 => Some(segment.point_ids[0]),
        3 => Some(segment.point_ids[1]),
        _ => None,
    }
}

fn unique_section_skamp_segment(
    definition: &crate::feature::FeatureDefinition,
    external_id: u32,
) -> Option<&crate::feature::FeatureSegment> {
    let segments = definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .filter(|segment| segment.external_id == external_id)
        .collect::<Vec<_>>();
    let [segment] = segments.as_slice() else {
        return None;
    };
    Some(segment)
}

fn section_skamp_point_entity_id(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<u32> {
    let segment = unique_section_skamp_segment(definition, item.entity_id)?;
    (item.sense == 0 && segment.kind == crate::feature::FeatureSegmentKind::Point)
        .then_some(segment.point_ids[0])
}

fn section_skamp_selected_point_id(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<u32> {
    let segment = unique_section_skamp_segment(definition, item.entity_id)?;
    match item.sense {
        2 => Some(segment.point_ids[0]),
        3 => Some(segment.point_ids[1]),
        4 => segment.center_id,
        _ => None,
    }
}

pub(crate) fn resolved_section_radii(
    definition: &crate::feature::FeatureDefinition,
) -> BTreeMap<u32, f64> {
    let mut candidates = BTreeMap::<u32, Vec<f64>>::new();
    for row in definition.variables.iter().flat_map(|table| &table.rows) {
        if row.variable_type == 3 {
            if let Some(value) = row.value.filter(|value| value.is_finite() && *value > 0.0) {
                candidates.entry(row.key).or_default().push(value);
            }
        }
    }
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
        candidates.entry(radius_id).or_default().push(value);
    }
    let points = resolved_section_points(definition);
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| &table.rows)
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
    {
        if unique_section_skamp_segment(definition, segment.external_id) != Some(segment) {
            continue;
        }
        let Some(radius_id) = segment.radius_ref else {
            continue;
        };
        let Some(center) = segment.center_id.and_then(|id| points.get(&id)) else {
            continue;
        };
        let endpoint_radii = segment
            .point_ids
            .iter()
            .filter_map(|id| points.get(id))
            .map(|point| (point[0] - center[0]).hypot(point[1] - center[1]))
            .filter(|radius| radius.is_finite() && *radius > 1e-12)
            .collect::<Vec<_>>();
        let Some(radius) = endpoint_radii.first().copied() else {
            continue;
        };
        let scale = endpoint_radii
            .iter()
            .copied()
            .fold(radius.max(1.0), f64::max);
        if endpoint_radii
            .iter()
            .all(|candidate| (*candidate - radius).abs() <= 1e-9 * scale)
        {
            candidates.entry(radius_id).or_default().push(radius);
        }
    }
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    for skamp in definition.relations.iter().flat_map(|table| &table.skamps) {
        let [first, second] = skamp.items.as_slice() else {
            continue;
        };
        if skamp.kind != 6 || first.sense != 0 || second.sense != 0 {
            continue;
        }
        let Some(first_radius) = unique_section_skamp_segment(definition, first.entity_id)
            .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
            .and_then(|segment| segment.radius_ref)
        else {
            continue;
        };
        let Some(second_radius) = unique_section_skamp_segment(definition, second.entity_id)
            .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
            .and_then(|segment| segment.radius_ref)
        else {
            continue;
        };
        adjacency
            .entry(first_radius)
            .or_default()
            .insert(second_radius);
        adjacency
            .entry(second_radius)
            .or_default()
            .insert(first_radius);
    }
    let mut remaining = candidates
        .keys()
        .chain(adjacency.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut radii = BTreeMap::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = std::collections::VecDeque::from([seed]);
        while let Some(radius_id) = pending.pop_front() {
            for neighbor in adjacency.get(&radius_id).into_iter().flatten() {
                if component.insert(*neighbor) {
                    pending.push_back(*neighbor);
                }
            }
        }
        let values = component
            .iter()
            .flat_map(|radius_id| candidates.get(radius_id).into_iter().flatten())
            .copied()
            .collect::<Vec<_>>();
        if let Some(value) = values.first().copied() {
            let scale = values.iter().copied().fold(value.max(1.0), f64::max);
            if !values
                .iter()
                .all(|candidate| (*candidate - value).abs() <= 1e-9 * scale)
            {
                remaining.retain(|radius_id| !component.contains(radius_id));
                continue;
            }
            radii.extend(component.iter().map(|radius_id| (*radius_id, value)));
        }
        remaining.retain(|radius_id| !component.contains(radius_id));
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

fn feature_plane_equations(scan: &ContainerScan, feature_id: u32) -> Vec<([f64; 3], [f64; 3])> {
    let ids = scan
        .surface_rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .map(|row| row.id)
        .collect::<Vec<_>>();
    let id_set = ids.iter().copied().collect::<BTreeSet<_>>();
    let outlines = scan
        .outline_planes
        .iter()
        .filter(|plane| id_set.contains(&plane.surface_id))
        .map(|plane| (plane.surface_id, (plane.origin, plane.normal)))
        .collect::<BTreeMap<_, _>>();
    ids.into_iter()
        .filter_map(|id| {
            outlines.get(&id).copied().or_else(|| {
                scan.plane_local_systems
                    .iter()
                    .find(|frame| frame.surface_id == id)
                    .and_then(|frame| Some((frame.origin?, frame.normal?)))
            })
        })
        .collect()
}

fn feature_outline_planes(scan: &ContainerScan, feature_id: u32) -> Vec<(u32, [f64; 3], [f64; 3])> {
    scan.surface_rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .filter_map(|row| {
            scan.outline_planes
                .iter()
                .find(|plane| plane.surface_id == row.id)
                .map(|plane| (row.id, plane.origin, plane.normal))
        })
        .collect()
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

fn revolved_section_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    revolution_axis: RevolutionAxis,
) -> Option<SurfaceGeometry> {
    let axis = normalized([
        revolution_axis.direction.x,
        revolution_axis.direction.y,
        revolution_axis.direction.z,
    ])?;
    let axis_origin = [
        revolution_axis.origin.x,
        revolution_axis.origin.y,
        revolution_axis.origin.z,
    ];
    let project = |point: [f64; 3]| {
        let displacement = std::array::from_fn(|index| point[index] - axis_origin[index]);
        let axial = dot(displacement, axis);
        let on_axis = std::array::from_fn(|index| axis_origin[index] + axial * axis[index]);
        let radial = std::array::from_fn(|index| point[index] - on_axis[index]);
        (on_axis, radial)
    };
    let vector = |values: [f64; 3]| Vector3::new(values[0], values[1], values[2]);
    let point = |values: [f64; 3]| Point3::new(values[0], values[1], values[2]);
    match geometry {
        SketchGeometry::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let direction = normalized(std::array::from_fn(|index| end[index] - start[index]))?;
            let (mut on_axis, mut radial) = project(start);
            let mut radius = dot(radial, radial).sqrt();
            if radius <= 1e-10 {
                (on_axis, radial) = project(end);
                radius = dot(radial, radial).sqrt();
            }
            let axial_rate = dot(direction, axis);
            let radial_rate =
                std::array::from_fn(|index| direction[index] - axial_rate * axis[index]);
            let radial_speed = dot(radial_rate, radial_rate).sqrt();
            let scale = radius.max(1.0);
            if radius > 1e-10 {
                let coplanar_residual = dot(cross(radial, radial_rate), axis).abs();
                (coplanar_residual <= 1e-9 * scale).then_some(())?;
            }
            let reference = normalized(radial).or_else(|| normalized(radial_rate))?;
            if radial_speed <= 1e-10 {
                (radius > 1e-10).then_some(())?;
                return Some(SurfaceGeometry::Cylinder {
                    origin: point(on_axis),
                    axis: vector(axis),
                    ref_direction: vector(reference),
                    radius,
                });
            }
            if axial_rate.abs() <= 1e-10 {
                return Some(SurfaceGeometry::Plane {
                    origin: point(on_axis),
                    normal: vector(axis),
                    u_axis: vector(reference),
                });
            }
            let radial_rate = dot(radial_rate, reference);
            let cone_axis = if radial_rate / axial_rate < 0.0 {
                std::array::from_fn(|index| -axis[index])
            } else {
                axis
            };
            Some(SurfaceGeometry::Cone {
                origin: point(on_axis),
                axis: vector(cone_axis),
                ref_direction: vector(reference),
                radius,
                ratio: 1.0,
                half_angle: radial_rate.abs().atan2(axial_rate.abs()),
            })
        }
        SketchGeometry::Arc { center, radius, .. } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            let (on_axis, radial) = project(center);
            let major_radius = dot(radial, radial).sqrt();
            let reference = normalized(radial).or_else(|| {
                [transform.u_axis, transform.v_axis]
                    .into_iter()
                    .find_map(|candidate| {
                        let axial = dot(candidate, axis);
                        normalized(std::array::from_fn(|index| {
                            candidate[index] - axial * axis[index]
                        }))
                    })
            })?;
            if major_radius <= 1e-10 {
                Some(SurfaceGeometry::Sphere {
                    center: point(center),
                    axis: vector(axis),
                    ref_direction: vector(reference),
                    radius: radius.0,
                })
            } else {
                Some(SurfaceGeometry::Torus {
                    center: point(on_axis),
                    axis: vector(axis),
                    ref_direction: vector(reference),
                    major_radius,
                    minor_radius: radius.0,
                })
            }
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

fn placed_section_nurbs(
    transform: &crate::placement::FeatureSectionTransform,
    nurbs: &NurbsCurve,
) -> NurbsCurve {
    NurbsCurve {
        degree: nurbs.degree,
        knots: nurbs.knots.clone(),
        control_points: nurbs
            .control_points
            .iter()
            .map(|point| {
                let placed = section_xyz_in_model(transform, [point.x, point.y, point.z]);
                Point3::new(placed[0], placed[1], placed[2])
            })
            .collect(),
        weights: nurbs.weights.clone(),
        periodic: nurbs.periodic,
    }
}

fn translated_nurbs_curve(curve: &NurbsCurve, translation: [f64; 3]) -> NurbsCurve {
    NurbsCurve {
        degree: curve.degree,
        knots: curve.knots.clone(),
        control_points: curve
            .control_points
            .iter()
            .map(|point| {
                Point3::new(
                    point.x + translation[0],
                    point.y + translation[1],
                    point.z + translation[2],
                )
            })
            .collect(),
        weights: curve.weights.clone(),
        periodic: curve.periodic,
    }
}

fn extruded_nurbs_surface(directrix: &NurbsCurve, sweep: [f64; 3]) -> Option<NurbsSurface> {
    let mut control_points = Vec::with_capacity(directrix.control_points.len() * 2);
    let mut weights = directrix
        .weights
        .as_ref()
        .map(|_| Vec::with_capacity(control_points.capacity()));
    for (index, point) in directrix.control_points.iter().enumerate() {
        control_points.push(*point);
        control_points.push(Point3::new(
            point.x + sweep[0],
            point.y + sweep[1],
            point.z + sweep[2],
        ));
        if let (Some(source), Some(target)) = (&directrix.weights, &mut weights) {
            target.extend([source[index], source[index]]);
        }
    }
    Some(NurbsSurface {
        u_degree: directrix.degree,
        v_degree: 1,
        u_knots: directrix.knots.clone(),
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: u32::try_from(directrix.control_points.len()).ok()?,
        v_count: 2,
        control_points,
        weights,
        u_periodic: directrix.periodic,
        v_periodic: false,
    })
}

fn transfer_saved_spline_curves(
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
            let suffix = spline.entity_id.map_or_else(
                || format!("offset{}", spline.offset),
                |entity_id| entity_id.to_string(),
            );
            let curve_id = CurveId(format!(
                "creo:featdefs:saved_spline_curve#{}:{suffix}",
                definition.id
            ));
            if ir.model.curves.iter().any(|curve| curve.id == curve_id) {
                continue;
            }
            annotate(
                annotations,
                &curve_id,
                "FeatDefs",
                spline.offset as u64,
                "placed_saved_interpolation_spline",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: curve_id,
                geometry: CurveGeometry::Nurbs(placed_section_nurbs(transform, &nurbs)),
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
            transferred += 1;
        }
    }
    transferred
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
        let Some(order_table) = &definition.order_table else {
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
        let solved = definition
            .trim_entities
            .iter()
            .flat_map(|trim_entities| &trim_entities.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        for segment in definition
            .segments
            .iter()
            .flat_map(|segments| &segments.rows)
            .filter(|segment| solved.contains(&segment.external_id))
        {
            let surface_id = match segment.kind {
                crate::feature::FeatureSegmentKind::Line
                | crate::feature::FeatureSegmentKind::Arc => order_table
                    .internal_id(segment.external_id)
                    .and_then(|_| generated_surface_id(table, segment.external_id)),
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

        for (internal_id, section_geometry, offset) in definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(saved_section_entity_geometry)
        {
            let Some(external_id) = order_table.external_id(internal_id) else {
                continue;
            };
            let Some(native_surface_id) = generated_surface_id(table, external_id) else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
            let Some(expected_kind) = surface_kind_for_geometry(&geometry) else {
                continue;
            };
            if !scan.surface_rows.iter().any(|row| {
                row.id == native_surface_id
                    && row.feature_id == feature_id
                    && row.kind == expected_kind
            }) {
                continue;
            }
            let id = SurfaceId(format!("creo:visibgeom:surface#{native_surface_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                offset as u64,
                "protextrude_saved_section_carrier",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{native_surface_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }

        let splines = definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
            .filter_map(|spline| {
                let internal_id = spline.entity_id?;
                let external_id = order_table.external_id(internal_id)?;
                let surface_id = generated_surface_id(table, external_id)?;
                scan.surface_rows
                    .iter()
                    .any(|row| {
                        row.id == surface_id
                            && row.feature_id == feature_id
                            && row.kind == crate::surface::SurfaceKind::Extrusion
                    })
                    .then_some((surface_id, spline))
            })
            .collect::<Vec<_>>();
        let Some(span) = extrusion_span(
            transform.origin,
            transform.normal,
            feature_plane_equations(scan, feature_id),
        ) else {
            continue;
        };
        let lower_translation = transform.normal.map(|value| value * span.lower);
        let sweep = transform
            .normal
            .map(|value| value * (span.upper - span.lower));
        for (native_surface_id, spline) in splines {
            let Some(section_curve) = saved_spline_nurbs(spline) else {
                continue;
            };
            let placed = placed_section_nurbs(transform, &section_curve);
            let directrix = translated_nurbs_curve(&placed, lower_translation);
            let Some(surface) = extruded_nurbs_surface(&directrix, sweep) else {
                continue;
            };
            let suffix = spline
                .entity_id
                .expect("ordered saved spline has an entity id")
                .to_string();
            let curve_id = CurveId(format!(
                "creo:feature:extrusion_directrix#{feature_id}:{suffix}"
            ));
            if !ir.model.curves.iter().any(|curve| curve.id == curve_id) {
                annotate(
                    annotations,
                    &curve_id,
                    "FeatDefs",
                    spline.offset as u64,
                    "protextrude_spline_directrix",
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Nurbs(directrix.clone()),
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
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{native_surface_id}"));
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            let procedural_id = ProceduralSurfaceId(format!(
                "creo:feature:extrusion_construction#{feature_id}:{suffix}"
            ));
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                spline.offset as u64,
                "protextrude_spline_surface",
                Exactness::Derived,
            );
            annotate(
                annotations,
                &procedural_id,
                "FeatDefs",
                spline.offset as u64,
                "protextrude_spline_surface_construction",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Nurbs(surface),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{native_surface_id}"),
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
                definition: ProceduralSurfaceDefinition::Extrusion {
                    directrix: curve_id,
                    parameter_interval: Some([
                        *directrix.knots.first().expect("validated spline knots"),
                        *directrix.knots.last().expect("validated spline knots"),
                    ]),
                    direction: Vector3::new(sweep[0], sweep[1], sweep[2]),
                    native_position: None,
                },
                cache_fit_tolerance: None,
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

fn line_pcurve(start: [f64; 2], end: [f64; 2]) -> PcurveGeometry {
    PcurveGeometry::Line {
        origin: Point2::new(start[0], start[1]),
        direction: Point2::new(end[0] - start[0], end[1] - start[1]),
    }
}

fn circular_pcurve(
    center: [f64; 2],
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> PcurveGeometry {
    let segment_count = ((end_angle - start_angle).abs() / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let step = (end_angle - start_angle) / segment_count as f64;
    let mut control_points = Vec::with_capacity(2 * segment_count + 1);
    let mut weights = Vec::with_capacity(2 * segment_count + 1);
    for segment in 0..segment_count {
        let first = start_angle + segment as f64 * step;
        let second = first + step;
        let middle = 0.5 * (first + second);
        let middle_weight = (0.5 * step).cos();
        if segment == 0 {
            control_points.push(Point2::new(
                center[0] + radius * first.cos(),
                center[1] + radius * first.sin(),
            ));
            weights.push(1.0);
        }
        control_points.push(Point2::new(
            center[0] + radius * middle.cos() / middle_weight,
            center[1] + radius * middle.sin() / middle_weight,
        ));
        weights.push(middle_weight);
        control_points.push(Point2::new(
            center[0] + radius * second.cos(),
            center[1] + radius * second.sin(),
        ));
        weights.push(1.0);
    }
    let mut knots = vec![0.0; 3];
    for boundary in 1..segment_count {
        knots.extend([boundary as f64 / segment_count as f64; 2]);
    }
    knots.extend([1.0; 3]);
    PcurveGeometry::Nurbs {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    }
}

fn extrusion_cap_pcurve(
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
) -> PcurveGeometry {
    match geometry {
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let [start_angle, end_angle] = if reversed {
                [end_angle.0, start_angle.0]
            } else {
                [start_angle.0, end_angle.0]
            };
            circular_pcurve([center.u, center.v], radius.0, start_angle, end_angle)
        }
        _ => line_pcurve(start, end),
    }
}

fn extrusion_side_uvs(
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
    span: ExtrusionSpan,
) -> [[[f64; 2]; 2]; 4] {
    let [first, second] = match geometry {
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } if reversed => [end_angle.0, start_angle.0],
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } => [start_angle.0, end_angle.0],
        _ => [0.0, (end[0] - start[0]).hypot(end[1] - start[1])],
    };
    [
        [[first, span.lower], [second, span.lower]],
        [[second, span.lower], [second, span.upper]],
        [[first, span.upper], [second, span.upper]],
        [[first, span.lower], [first, span.upper]],
    ]
}

fn extrusion_profile_signed_area(
    profile: &[(SketchGeometry, bool, [f64; 2], [f64; 2])],
) -> Option<f64> {
    let area_twice = profile
        .iter()
        .map(|(geometry, reversed, start, end)| {
            let chord = start[0].mul_add(end[1], -(start[1] * end[0]));
            let SketchGeometry::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } = geometry
            else {
                return chord;
            };
            let forward_delta = (end_angle.0 - start_angle.0).rem_euclid(std::f64::consts::TAU);
            let delta = if *reversed {
                -forward_delta
            } else {
                forward_delta
            };
            center.u.mul_add(
                end[1] - start[1],
                -(center.v * (end[0] - start[0])) + radius.0 * radius.0 * delta,
            )
        })
        .sum::<f64>();
    let scale = profile
        .iter()
        .flat_map(|(_, _, start, end)| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (area_twice.abs() > 1e-12 * scale * scale).then_some(0.5 * area_twice)
}

type ExtrusionProfile = Vec<(SketchGeometry, bool, [f64; 2], [f64; 2])>;

fn segments_intersect(first: [[f64; 2]; 2], second: [[f64; 2]; 2], tolerance: f64) -> bool {
    let orient = |a: [f64; 2], b: [f64; 2], point: [f64; 2]| {
        (b[0] - a[0]).mul_add(point[1] - a[1], -((b[1] - a[1]) * (point[0] - a[0])))
    };
    let on_segment = |segment: [[f64; 2]; 2], point: [f64; 2]| {
        point[0] >= segment[0][0].min(segment[1][0]) - tolerance
            && point[0] <= segment[0][0].max(segment[1][0]) + tolerance
            && point[1] >= segment[0][1].min(segment[1][1]) - tolerance
            && point[1] <= segment[0][1].max(segment[1][1]) + tolerance
    };
    let orientations = [
        orient(first[0], first[1], second[0]),
        orient(first[0], first[1], second[1]),
        orient(second[0], second[1], first[0]),
        orient(second[0], second[1], first[1]),
    ];
    let first_length = (first[1][0] - first[0][0]).hypot(first[1][1] - first[0][1]);
    let second_length = (second[1][0] - second[0][0]).hypot(second[1][1] - second[0][1]);
    let first_cross_tolerance = tolerance * first_length.max(1.0);
    let second_cross_tolerance = tolerance * second_length.max(1.0);
    let opposite = |left: f64, right: f64, cross_tolerance: f64| {
        (left > cross_tolerance && right < -cross_tolerance)
            || (left < -cross_tolerance && right > cross_tolerance)
    };
    if opposite(orientations[0], orientations[1], first_cross_tolerance)
        && opposite(orientations[2], orientations[3], second_cross_tolerance)
    {
        return true;
    }
    (orientations[0].abs() <= first_cross_tolerance && on_segment(first, second[0]))
        || (orientations[1].abs() <= first_cross_tolerance && on_segment(first, second[1]))
        || (orientations[2].abs() <= second_cross_tolerance && on_segment(second, first[0]))
        || (orientations[3].abs() <= second_cross_tolerance && on_segment(second, first[1]))
}

fn profile_arc(
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
) -> Option<([f64; 2], f64, f64, f64)> {
    let SketchGeometry::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = &segment.0
    else {
        return None;
    };
    let forward_delta = (end_angle.0 - start_angle.0).rem_euclid(std::f64::consts::TAU);
    let delta = if segment.1 {
        -forward_delta
    } else {
        forward_delta
    };
    Some((
        [center.u, center.v],
        radius.0,
        (segment.2[1] - center.v).atan2(segment.2[0] - center.u),
        delta,
    ))
}

fn point_on_profile_arc(point: [f64; 2], arc: ([f64; 2], f64, f64, f64), tolerance: f64) -> bool {
    let (center, radius, start, delta) = arc;
    let relative = [point[0] - center[0], point[1] - center[1]];
    let distance = relative[0].hypot(relative[1]);
    if (distance - radius).abs() > tolerance {
        return false;
    }
    let angle = relative[1].atan2(relative[0]);
    let travel = if delta >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU)
    };
    travel <= delta.abs() + tolerance / radius.max(1.0)
}

fn line_arc_intersect(line: [[f64; 2]; 2], arc: ([f64; 2], f64, f64, f64), tolerance: f64) -> bool {
    let direction = [line[1][0] - line[0][0], line[1][1] - line[0][1]];
    let relative = [line[0][0] - arc.0[0], line[0][1] - arc.0[1]];
    let a = direction[0].mul_add(direction[0], direction[1] * direction[1]);
    let b = 2.0 * direction[0].mul_add(relative[0], direction[1] * relative[1]);
    let c = relative[0].mul_add(relative[0], relative[1] * relative[1]) - arc.1 * arc.1;
    let discriminant = b.mul_add(b, -(4.0 * a * c));
    if a <= tolerance * tolerance || discriminant < -tolerance * tolerance {
        return false;
    }
    let root = discriminant.max(0.0).sqrt();
    [-root, root].into_iter().any(|signed_root| {
        let parameter = (-b + signed_root) / (2.0 * a);
        parameter >= -tolerance
            && parameter <= 1.0 + tolerance
            && point_on_profile_arc(
                [
                    line[0][0] + parameter * direction[0],
                    line[0][1] + parameter * direction[1],
                ],
                arc,
                tolerance,
            )
    })
}

fn arcs_intersect(
    first: ([f64; 2], f64, f64, f64),
    second: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let displacement = [second.0[0] - first.0[0], second.0[1] - first.0[1]];
    let distance = displacement[0].hypot(displacement[1]);
    if distance <= tolerance && (first.1 - second.1).abs() <= tolerance {
        let endpoints = |arc: ([f64; 2], f64, f64, f64)| {
            [
                [
                    arc.0[0] + arc.1 * arc.2.cos(),
                    arc.0[1] + arc.1 * arc.2.sin(),
                ],
                [
                    arc.0[0] + arc.1 * (arc.2 + arc.3).cos(),
                    arc.0[1] + arc.1 * (arc.2 + arc.3).sin(),
                ],
            ]
        };
        return endpoints(first)
            .into_iter()
            .any(|point| point_on_profile_arc(point, second, tolerance))
            || endpoints(second)
                .into_iter()
                .any(|point| point_on_profile_arc(point, first, tolerance));
    }
    if distance <= tolerance
        || distance > first.1 + second.1 + tolerance
        || distance < (first.1 - second.1).abs() - tolerance
    {
        return false;
    }
    let along = (first.1 * first.1 - second.1 * second.1 + distance * distance) / (2.0 * distance);
    let height_squared = first.1 * first.1 - along * along;
    if height_squared < -tolerance * tolerance {
        return false;
    }
    let base = [
        first.0[0] + along * displacement[0] / distance,
        first.0[1] + along * displacement[1] / distance,
    ];
    let height = height_squared.max(0.0).sqrt();
    let offset = [
        -height * displacement[1] / distance,
        height * displacement[0] / distance,
    ];
    [-1.0, 1.0].into_iter().any(|sign| {
        let point = [base[0] + sign * offset[0], base[1] + sign * offset[1]];
        point_on_profile_arc(point, first, tolerance)
            && point_on_profile_arc(point, second, tolerance)
    })
}

fn profile_segments_intersect(
    first: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    second: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    tolerance: f64,
) -> bool {
    match (profile_arc(first), profile_arc(second)) {
        (None, None) => segments_intersect([first.2, first.3], [second.2, second.3], tolerance),
        (None, Some(arc)) => line_arc_intersect([first.2, first.3], arc, tolerance),
        (Some(arc), None) => line_arc_intersect([second.2, second.3], arc, tolerance),
        (Some(first), Some(second)) => arcs_intersect(first, second, tolerance),
    }
}

fn profile_strictly_contains(profile: &ExtrusionProfile, point: [f64; 2]) -> bool {
    let mut winding = 0.0;
    for segment in profile {
        let mut accumulate = |first: [f64; 2], second: [f64; 2]| {
            let first = [first[0] - point[0], first[1] - point[1]];
            let second = [second[0] - point[0], second[1] - point[1]];
            winding += first[0]
                .mul_add(second[1], -(first[1] * second[0]))
                .atan2(first[0].mul_add(second[0], first[1] * second[1]));
        };
        if let Some((center, radius, start, delta)) = profile_arc(segment) {
            let pieces = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
            for piece in 0..pieces {
                let first = start + delta * piece as f64 / pieces as f64;
                let second = start + delta * (piece + 1) as f64 / pieces as f64;
                accumulate(
                    [
                        center[0] + radius * first.cos(),
                        center[1] + radius * first.sin(),
                    ],
                    [
                        center[0] + radius * second.cos(),
                        center[1] + radius * second.sin(),
                    ],
                );
            }
        } else {
            accumulate(segment.2, segment.3);
        }
    }
    winding.abs() > std::f64::consts::PI
}

fn ordered_extrusion_profiles(
    mut profiles: Vec<ExtrusionProfile>,
) -> Option<(Vec<ExtrusionProfile>, f64)> {
    let scale = profiles
        .iter()
        .flatten()
        .flat_map(|(_, _, start, end)| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let tolerance = 1e-9 * scale;
    for profile in &profiles {
        for first in 0..profile.len() {
            for second in first + 1..profile.len() {
                if second == first + 1 || (first == 0 && second + 1 == profile.len()) {
                    continue;
                }
                if profile_segments_intersect(&profile[first], &profile[second], tolerance) {
                    return None;
                }
            }
        }
    }
    for first in 0..profiles.len() {
        for second in first + 1..profiles.len() {
            for first_segment in &profiles[first] {
                for second_segment in &profiles[second] {
                    if profile_segments_intersect(first_segment, second_segment, tolerance) {
                        return None;
                    }
                }
            }
        }
    }
    let outer = profiles
        .iter()
        .enumerate()
        .filter(|(candidate, profile)| {
            profiles.iter().enumerate().all(|(index, inner)| {
                index == *candidate || profile_strictly_contains(profile, inner[0].2)
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [outer] = outer.as_slice() else {
        return None;
    };
    for first in 0..profiles.len() {
        if first == *outer {
            continue;
        }
        for second in first + 1..profiles.len() {
            if second == *outer {
                continue;
            }
            if profile_strictly_contains(&profiles[first], profiles[second][0].2)
                || profile_strictly_contains(&profiles[second], profiles[first][0].2)
            {
                return None;
            }
        }
    }
    let outer_area = extrusion_profile_signed_area(&profiles[*outer])?;
    if profiles.iter().enumerate().any(|(index, profile)| {
        index != *outer
            && extrusion_profile_signed_area(profile)
                .is_none_or(|area| area.is_sign_positive() == outer_area.is_sign_positive())
    }) {
        return None;
    }
    profiles.swap(0, *outer);
    Some((profiles, outer_area))
}

fn add_extrusion_pcurve(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    id: PcurveId,
    source_offset: usize,
    geometry: PcurveGeometry,
) -> PcurveId {
    annotate(
        annotations,
        &id,
        "FeatDefs",
        source_offset as u64,
        "extrusion_trim_pcurve",
        Exactness::Derived,
    );
    ir.model.pcurves.push(Pcurve {
        id: id.clone(),
        geometry,
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some([0.0, 1.0]),
        fit_tolerance: None,
    });
    id
}

fn transfer_resolved_extrusion_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for (transform_index, transform) in scan.feature_section_transforms.iter().enumerate() {
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Extrude) {
            continue;
        }
        if scan.feature_section_transforms[..transform_index]
            .iter()
            .filter_map(|preceding| preceding.feature_id)
            .any(|preceding| feature_recipe(scan, preceding).is_some())
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
        let cap_origins = feature_plane_equations(scan, feature_id);
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
        let (profiles, outer_area) = if profiles.len() == 1 {
            let Some(area) = extrusion_profile_signed_area(&profiles[0]) else {
                continue;
            };
            (profiles, area)
        } else {
            let Some(result) = ordered_extrusion_profiles(profiles) else {
                continue;
            };
            result
        };
        let forward_caps = outer_area > 0.0;

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
                let (geometry, reversed, start, end) = &profile[edge_index];
                let bottom_pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId(format!(
                        "{prefix}:pcurve:{profile_index}:{edge_index}:bottom-cap"
                    )),
                    transform.offset,
                    extrusion_cap_pcurve(geometry, *reversed, *start, *end),
                );
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
                    pcurve: Some(bottom_pcurve),
                });
                let id = top_coedges[ring_index].clone();
                let (geometry, reversed, start, end) = &profile[ring_index];
                let top_pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId(format!(
                        "{prefix}:pcurve:{profile_index}:{ring_index}:top-cap"
                    )),
                    transform.offset,
                    extrusion_cap_pcurve(geometry, *reversed, *start, *end),
                );
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
                    pcurve: Some(top_pcurve),
                });
            }

            let forward_sides = extrusion_profile_signed_area(profile)
                .expect("validated extrusion profile has nonzero area")
                > 0.0;
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
                let side_uvs =
                    extrusion_side_uvs(geometry, profile[index].1, *start, profile[index].3, span);
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
                    let pcurve = add_extrusion_pcurve(
                        ir,
                        annotations,
                        PcurveId(format!(
                            "{prefix}:pcurve:{profile_index}:{index}:side:{use_index}"
                        )),
                        transform.offset,
                        line_pcurve(side_uvs[use_index][0], side_uvs[use_index][1]),
                    );
                    ir.model.coedges.push(Coedge {
                        id: coedges[use_index].clone(),
                        owner_loop: loop_id.clone(),
                        edge: edge_uses[use_index].0.clone(),
                        next: coedges[(use_index + 1) % 4].clone(),
                        previous: coedges[(use_index + 3) % 4].clone(),
                        radial_next,
                        sense: edge_uses[use_index].1,
                        pcurve: Some(pcurve),
                    });
                }
                ir.model.faces.push(Face {
                    id: face_id.clone(),
                    shell: shell_id.clone(),
                    surface: surface_id,
                    sense: if forward_sides {
                        Sense::Forward
                    } else {
                        Sense::Reversed
                    },
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
            sense: if forward_caps {
                Sense::Reversed
            } else {
                Sense::Forward
            },
            loops: bottom_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.faces.push(Face {
            id: top_face,
            shell: shell_id.clone(),
            surface: top_surface,
            sense: if forward_caps {
                Sense::Forward
            } else {
                Sense::Reversed
            },
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
    let stored_recipe = scan
        .feature_operations
        .iter()
        .find(|operation| operation.feature_id == feature_id)
        .and_then(|operation| operation.recipe);
    let schema_recipe = scan
        .feature_rows
        .iter()
        .find(|row| row.feature_id == feature_id)
        .and_then(|row| match row.root_schema_class {
            Some(916 | 917) => Some(crate::feature::FeatureRecipeKind::Extrude),
            _ => None,
        });
    stored_recipe.or(schema_recipe)
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

fn section_point_locus(
    definition_id: u32,
    segments: &[crate::feature::FeatureSegment],
    point_id: u32,
) -> Option<SketchLocus> {
    let segment = segments
        .iter()
        .filter(|segment| segment.point_ids.contains(&point_id))
        .min_by_key(|segment| segment.external_id)?;
    let entity = SketchEntityId(format!(
        "creo:featdefs:sketch_entity#{definition_id}:{}",
        segment.external_id
    ));
    if segment.point_ids[0] == point_id {
        Some(SketchLocus::Start(entity))
    } else {
        Some(SketchLocus::End(entity))
    }
}

fn relation_incidence_entities(
    definition: &crate::feature::FeatureDefinition,
    relation_id: u32,
) -> Vec<SketchEntityId> {
    let Some(relations) = &definition.relations else {
        return Vec::new();
    };
    let incidence_ids = relations
        .triples
        .iter()
        .filter(|triple| triple.relation_id == Some(relation_id))
        .filter_map(|triple| triple.skamp_id)
        .collect::<BTreeSet<_>>();
    if incidence_ids.len() != 1 {
        return Vec::new();
    }
    let incidence_id = *incidence_ids
        .first()
        .expect("single relation incidence id exists");
    let incidences = relations
        .skamps
        .iter()
        .filter(|skamp| skamp.id == incidence_id)
        .collect::<Vec<_>>();
    let [incidence] = incidences.as_slice() else {
        return Vec::new();
    };
    let known = section_entity_external_ids(definition);
    incidence
        .items
        .iter()
        .map(|item| {
            known.contains(&item.entity_id).then(|| {
                SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, item.entity_id
                ))
            })
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

fn section_dimension_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let (Some(segments), Some(relations)) = (&definition.segments, &definition.relations) else {
        return Vec::new();
    };
    relations
        .rows
        .iter()
        .map(|relation| {
            let parameter = definition
                .owner_feature_id
                .zip(definition.dimensions.as_ref())
                .and_then(|(owner, dimensions)| {
                    let dimension = dimensions
                        .rows
                        .get(usize::try_from(relation.dimension_id).ok()?)?;
                    dimension.value?;
                    Some(ParameterId(format!(
                        "creo:featdefs:parameter#{owner}:{}",
                        dimension.external_id
                    )))
                });
            let typed = (|| {
                let parameter = parameter.clone()?;
                if relation.relation_type == 14
                    && relation.sign == 1
                    && relation.operand_vectors?[1] == [Some(0); 4]
                    && relation.operand_vectors?[2] == [Some(15), Some(0), Some(0), Some(0)]
                {
                    let vectors = relation.operand_vectors?;
                    let [Some(radius_id), Some(0), Some(0), Some(0)] = vectors[0] else {
                        return None;
                    };
                    let segment = segments.rows.iter().find(|segment| {
                        segment.kind == crate::feature::FeatureSegmentKind::Arc
                            && segment.radius_ref == Some(radius_id)
                    })?;
                    return Some(SketchConstraintDefinition::Radius {
                        entity: SketchEntityId(format!(
                            "creo:featdefs:sketch_entity#{}:{}",
                            definition.id, segment.external_id
                        )),
                        parameter,
                    });
                }
                if relation.relation_type != 0 || !matches!(relation.sign, 0 | 1 | 0xf6) {
                    return None;
                }
                if let Some(vectors) = relation.operand_vectors {
                    if section_linear_distance_vectors(vectors) {
                        if let [Some(first_id), Some(second_id), _, _] = vectors[0] {
                            if let Some(measured) = segments.rows.iter().find(|segment| {
                                segment.point_ids == [first_id, second_id]
                                    || segment.point_ids == [second_id, first_id]
                            }) {
                                if let (Some(first), Some(second)) = (
                                    section_point_locus(definition.id, &segments.rows, first_id),
                                    section_point_locus(definition.id, &segments.rows, second_id),
                                ) {
                                    match measured.vertical_horizontal {
                                        Some(0) => {
                                            return Some(
                                                SketchConstraintDefinition::VerticalDistance {
                                                    first,
                                                    second,
                                                    parameter,
                                                },
                                            );
                                        }
                                        Some(1) => {
                                            return Some(
                                                SketchConstraintDefinition::HorizontalDistance {
                                                    first,
                                                    second,
                                                    parameter,
                                                },
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
                let entities = relation_incidence_entities(definition, relation.relation_id);
                (!entities.is_empty()).then_some(SketchConstraintDefinition::Distance {
                    entities,
                    parameter,
                })
            })();
            let constraint_definition =
                typed.unwrap_or_else(|| SketchConstraintDefinition::Native {
                    native_kind: format!("creo:relation:{}", relation.relation_type),
                    entities: relation_incidence_entities(definition, relation.relation_id),
                    parameter,
                    operands: vec![SketchNativeOperand {
                        native_kind: "relat_ptr".to_string(),
                        object_index: relation.relation_id,
                        native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                    }],
                });
            (
                SketchConstraint {
                    id: SketchConstraintId(format!(
                        "creo:featdefs:sketch_constraint#{}:relation:{}",
                        definition.id, relation.relation_id
                    )),
                    sketch: sketch.clone(),
                    definition: constraint_definition,
                    native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                },
                relation.offset,
            )
        })
        .collect()
}

fn section_linear_distance_vectors(vectors: [[Option<u32>; 4]; 3]) -> bool {
    vectors[0][2..] == [None, Some(1)]
        && matches!(
            vectors[1],
            [Some(0), Some(0), Some(0), Some(0)] | [Some(1), Some(1), Some(0), Some(1)]
        )
        && vectors[2] == [Some(15), Some(16), Some(15), Some(1)]
}

fn section_skamp_locus(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    let entity = SketchEntityId(format!(
        "creo:featdefs:sketch_entity#{}:{}",
        definition.id, item.entity_id
    ));
    if let Some(segment) = definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .find(|segment| segment.external_id == item.entity_id)
    {
        return match (segment.kind, item.sense) {
            (_, 0) => Some(SketchLocus::Entity(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 2) => Some(SketchLocus::End(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 3) => Some(SketchLocus::Start(entity)),
            (crate::feature::FeatureSegmentKind::Arc, 4) => Some(SketchLocus::Center(entity)),
            (_, 2) => Some(SketchLocus::Start(entity)),
            (_, 3) => Some(SketchLocus::End(entity)),
            _ => None,
        };
    }
    let internal_id = definition
        .order_table
        .as_ref()?
        .rows
        .iter()
        .find(|row| row.external_id == item.entity_id)?
        .internal_id;
    let saved = definition
        .saved_section
        .as_ref()?
        .entities
        .iter()
        .find(|saved| match saved {
            crate::feature::FeatureSavedEntity::Line(line) => line.entity_id == internal_id,
            crate::feature::FeatureSavedEntity::Arc(arc) => arc.entity_id == internal_id,
            crate::feature::FeatureSavedEntity::Circle(circle) => circle.entity_id == internal_id,
            crate::feature::FeatureSavedEntity::Spline(spline) => {
                spline.entity_id == Some(internal_id)
            }
            crate::feature::FeatureSavedEntity::Dummy(dummy) => {
                dummy.entity_id == Some(internal_id)
            }
        })?;
    match (saved, item.sense) {
        (_, 0) => Some(SketchLocus::Entity(entity)),
        (crate::feature::FeatureSavedEntity::Line(_), 2) => Some(SketchLocus::Start(entity)),
        (crate::feature::FeatureSavedEntity::Line(_), 3) => Some(SketchLocus::End(entity)),
        (crate::feature::FeatureSavedEntity::Arc(_), 2) => Some(SketchLocus::End(entity)),
        (crate::feature::FeatureSavedEntity::Arc(_), 3) => Some(SketchLocus::Start(entity)),
        (
            crate::feature::FeatureSavedEntity::Arc(_)
            | crate::feature::FeatureSavedEntity::Circle(_),
            4,
        ) => Some(SketchLocus::Center(entity)),
        _ => None,
    }
}

fn section_skamp_segment<'a>(
    segments: &'a [crate::feature::FeatureSegment],
    item: &crate::feature::FeatureSkampItem,
) -> Option<&'a crate::feature::FeatureSegment> {
    segments
        .iter()
        .find(|segment| segment.external_id == item.entity_id)
}

fn section_skamp_endpoint(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    matches!(item.sense, 2 | 3)
        .then(|| section_skamp_locus(definition, item))
        .flatten()
}

fn section_skamp_point_locus(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchLocus> {
    matches!(item.sense, 2..=4)
        .then(|| section_skamp_locus(definition, item))
        .flatten()
}

fn section_skamp_line_pair(
    definition_id: u32,
    segments: &[crate::feature::FeatureSegment],
    first: &crate::feature::FeatureSkampItem,
    second: &crate::feature::FeatureSkampItem,
) -> Option<[SketchEntityId; 2]> {
    if first.sense != 0
        || second.sense != 0
        || section_skamp_segment(segments, first)?.kind != crate::feature::FeatureSegmentKind::Line
        || section_skamp_segment(segments, second)?.kind != crate::feature::FeatureSegmentKind::Line
    {
        return None;
    }
    Some([first, second].map(|item| {
        SketchEntityId(format!(
            "creo:featdefs:sketch_entity#{definition_id}:{}",
            item.entity_id
        ))
    }))
}

fn section_skamp_circular_entity(
    definition: &crate::feature::FeatureDefinition,
    item: &crate::feature::FeatureSkampItem,
) -> Option<SketchEntityId> {
    if item.sense != 0 {
        return None;
    }
    let circular = definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .find(|segment| segment.external_id == item.entity_id)
        .is_some_and(|segment| segment.kind == crate::feature::FeatureSegmentKind::Arc)
        || definition
            .order_table
            .iter()
            .flat_map(|table| &table.rows)
            .find(|row| row.external_id == item.entity_id)
            .and_then(|row| {
                definition
                    .saved_section
                    .as_ref()?
                    .entities
                    .iter()
                    .find(|entity| match entity {
                        crate::feature::FeatureSavedEntity::Arc(arc) => {
                            arc.entity_id == row.internal_id
                        }
                        crate::feature::FeatureSavedEntity::Circle(circle) => {
                            circle.entity_id == row.internal_id
                        }
                        _ => false,
                    })
            })
            .is_some();
    circular.then(|| {
        SketchEntityId(format!(
            "creo:featdefs:sketch_entity#{}:{}",
            definition.id, item.entity_id
        ))
    })
}

fn section_skamp_constraints(
    definition: &crate::feature::FeatureDefinition,
    sketch: &SketchId,
) -> Vec<(SketchConstraint, usize)> {
    let (Some(segments), Some(relations)) = (&definition.segments, &definition.relations) else {
        return Vec::new();
    };
    let section_entities = section_entity_external_ids(definition);
    relations
        .skamps
        .iter()
        .filter_map(|skamp| {
            let constraint_definition = match (skamp.kind, skamp.items.as_slice()) {
                (0, [first, second])
                    if section_skamp_endpoint(definition, first).is_some()
                        && section_skamp_endpoint(definition, second).is_some() =>
                {
                    SketchConstraintDefinition::CoincidentLoci {
                        loci: vec![
                            section_skamp_endpoint(definition, first)?,
                            section_skamp_endpoint(definition, second)?,
                        ],
                    }
                }
                (3, [first, second])
                    if (first.sense == 0
                        && section_skamp_locus(definition, first).is_some()
                        && section_skamp_point_locus(definition, second).is_some())
                        || (second.sense == 0
                            && section_skamp_locus(definition, second).is_some()
                            && section_skamp_point_locus(definition, first).is_some()) =>
                {
                    SketchConstraintDefinition::CoincidentLoci {
                        loci: vec![
                            section_skamp_locus(definition, first)?,
                            section_skamp_locus(definition, second)?,
                        ],
                    }
                }
                (1, [item])
                    if section_skamp_segment(&segments.rows, item)?.kind
                        == crate::feature::FeatureSegmentKind::Line =>
                {
                    SketchConstraintDefinition::Horizontal {
                        entity: SketchEntityId(format!(
                            "creo:featdefs:sketch_entity#{}:{}",
                            definition.id, item.entity_id
                        )),
                    }
                }
                (2, [item])
                    if section_skamp_segment(&segments.rows, item)?.kind
                        == crate::feature::FeatureSegmentKind::Line =>
                {
                    SketchConstraintDefinition::Vertical {
                        entity: SketchEntityId(format!(
                            "creo:featdefs:sketch_entity#{}:{}",
                            definition.id, item.entity_id
                        )),
                    }
                }
                (4, [first, second])
                    if section_skamp_endpoint(definition, first).is_some()
                        && section_skamp_endpoint(definition, second).is_some() =>
                {
                    SketchConstraintDefinition::TangentLoci {
                        first: section_skamp_endpoint(definition, first)?,
                        second: section_skamp_endpoint(definition, second)?,
                    }
                }
                (5, [first, second])
                    if section_skamp_line_pair(definition.id, &segments.rows, first, second)
                        .is_some() =>
                {
                    let [first, second] =
                        section_skamp_line_pair(definition.id, &segments.rows, first, second)?;
                    SketchConstraintDefinition::Perpendicular { first, second }
                }
                (6, [first, second])
                    if section_skamp_circular_entity(definition, first).is_some()
                        && section_skamp_circular_entity(definition, second).is_some() =>
                {
                    SketchConstraintDefinition::Equal {
                        first: section_skamp_circular_entity(definition, first)?,
                        second: section_skamp_circular_entity(definition, second)?,
                    }
                }
                (7, [first, second])
                    if section_skamp_line_pair(definition.id, &segments.rows, first, second)
                        .is_some() =>
                {
                    let [first, second] =
                        section_skamp_line_pair(definition.id, &segments.rows, first, second)?;
                    SketchConstraintDefinition::Parallel { first, second }
                }
                (8, [first, second])
                    if section_skamp_line_pair(definition.id, &segments.rows, first, second)
                        .is_some() =>
                {
                    let [first, second] =
                        section_skamp_line_pair(definition.id, &segments.rows, first, second)?;
                    SketchConstraintDefinition::Equal { first, second }
                }
                (9, [first, second])
                    if first.sense == 0
                        && second.sense == 0
                        && matches!(
                            (
                                section_skamp_segment(&segments.rows, first)?.kind,
                                section_skamp_segment(&segments.rows, second)?.kind,
                            ),
                            (
                                crate::feature::FeatureSegmentKind::Line,
                                crate::feature::FeatureSegmentKind::Point
                            ) | (
                                crate::feature::FeatureSegmentKind::Point,
                                crate::feature::FeatureSegmentKind::Line
                            )
                        ) =>
                {
                    SketchConstraintDefinition::CoincidentLoci {
                        loci: vec![
                            section_skamp_locus(definition, first)?,
                            section_skamp_locus(definition, second)?,
                        ],
                    }
                }
                (14, [axis, first, second])
                    if axis.sense == 0
                        && section_skamp_segment(&segments.rows, axis)?.kind
                            == crate::feature::FeatureSegmentKind::Line
                        && section_skamp_point_locus(definition, first).is_some()
                        && section_skamp_point_locus(definition, second).is_some() =>
                {
                    SketchConstraintDefinition::Symmetric {
                        first: section_skamp_point_locus(definition, first)?,
                        second: section_skamp_point_locus(definition, second)?,
                        axis: SketchEntityId(format!(
                            "creo:featdefs:sketch_entity#{}:{}",
                            definition.id, axis.entity_id
                        )),
                    }
                }
                _ => {
                    let entities = skamp
                        .items
                        .iter()
                        .map(|item| {
                            section_entities.contains(&item.entity_id).then(|| {
                                SketchEntityId(format!(
                                    "creo:featdefs:sketch_entity#{}:{}",
                                    definition.id, item.entity_id
                                ))
                            })
                        })
                        .collect::<Option<Vec<_>>>()?;
                    if entities.is_empty() {
                        return None;
                    }
                    SketchConstraintDefinition::Native {
                        native_kind: format!("creo:skamp:{}", skamp.kind),
                        entities,
                        parameter: None,
                        operands: skamp
                            .items
                            .iter()
                            .map(|item| SketchNativeOperand {
                                native_kind: format!("sense:{}", item.sense),
                                object_index: item.entity_id,
                                native_ref: None,
                            })
                            .collect(),
                    }
                }
            };
            Some((
                SketchConstraint {
                    id: SketchConstraintId(format!(
                        "creo:featdefs:sketch_constraint#{}:skamp:{}",
                        definition.id, skamp.id
                    )),
                    sketch: sketch.clone(),
                    definition: constraint_definition,
                    native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                },
                skamp.offset,
            ))
        })
        .collect()
}

fn section_entity_external_ids(definition: &crate::feature::FeatureDefinition) -> BTreeSet<u32> {
    let mut ids = definition
        .segments
        .iter()
        .flat_map(|segments| &segments.rows)
        .map(|segment| segment.external_id)
        .collect::<BTreeSet<_>>();
    let Some(order) = &definition.order_table else {
        return ids;
    };
    ids.extend(
        definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(saved_section_entity_geometry)
            .filter_map(|(internal_id, _, _)| order.external_id(internal_id)),
    );
    ids
}

fn resolved_profile_chains(
    definition: &crate::feature::FeatureDefinition,
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = &definition.trim_entities else {
        return resolved_segment_profile_chains(definition, emitted);
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

fn resolved_segment_profile_chains(
    definition: &crate::feature::FeatureDefinition,
    emitted: &BTreeSet<u32>,
) -> Vec<Vec<SketchEntityUse>> {
    let Some(table) = &definition.segments else {
        return Vec::new();
    };
    let rows = table
        .rows
        .iter()
        .filter(|segment| {
            emitted.contains(&segment.external_id)
                && matches!(
                    segment.kind,
                    crate::feature::FeatureSegmentKind::Line
                        | crate::feature::FeatureSegmentKind::Arc
                )
        })
        .collect::<Vec<_>>();
    let mut incident = BTreeMap::<u32, Vec<usize>>::new();
    for (index, segment) in rows.iter().enumerate() {
        for point in segment.point_ids {
            incident.entry(point).or_default().push(index);
        }
    }
    let mut remaining = (0..rows.len()).collect::<BTreeSet<_>>();
    let mut profiles = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        let mut component = BTreeSet::from([seed]);
        let mut frontier = vec![seed];
        while let Some(index) = frontier.pop() {
            for point in rows[index].point_ids {
                for adjacent in &incident[&point] {
                    if component.insert(*adjacent) {
                        frontier.push(*adjacent);
                    }
                }
            }
        }
        remaining.retain(|index| !component.contains(index));
        if component.iter().any(|index| {
            rows[*index].point_ids.into_iter().any(|point| {
                incident[&point]
                    .iter()
                    .filter(|row| component.contains(row))
                    .count()
                    != 2
            })
        }) {
            continue;
        }
        let first = component
            .iter()
            .min_by_key(|index| rows[**index].external_id)
            .copied()
            .expect("component contains seed");
        let mut point = rows[first].point_ids[0].min(rows[first].point_ids[1]);
        let start = point;
        let mut unused = component;
        let mut profile = Vec::new();
        while !unused.is_empty() {
            let candidates = incident[&point]
                .iter()
                .filter(|index| unused.contains(index))
                .copied()
                .collect::<BTreeSet<_>>();
            let index = if profile.is_empty() && candidates.contains(&first) {
                first
            } else if candidates.len() == 1 {
                *candidates.first().expect("one candidate")
            } else {
                break;
            };
            let segment = rows[index];
            let traversal_reversed = segment.point_ids[1] == point;
            if !traversal_reversed && segment.point_ids[0] != point {
                break;
            }
            let analytic_reversed = segment.kind == crate::feature::FeatureSegmentKind::Arc
                && segment.arc_orientation == Some(0);
            profile.push(SketchEntityUse {
                entity: SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, segment.external_id
                )),
                reversed: traversal_reversed ^ analytic_reversed,
            });
            point = if traversal_reversed {
                segment.point_ids[0]
            } else {
                segment.point_ids[1]
            };
            unused.remove(&index);
        }
        if unused.is_empty() && point == start {
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
        let segments = definition
            .segments
            .as_ref()
            .map_or(&[][..], |segments| segments.rows.as_slice());
        let points = resolved_section_points(definition);
        let solved = definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        let trim_vertex_coordinates = resolved_trim_vertex_coordinates(definition, &points);
        let emitted = segments
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
        let mut profiles = resolved_profile_chains(definition, &emitted);
        let profile_segments = segments
            .iter()
            .filter(|segment| {
                let id = SketchEntityId(format!(
                    "creo:featdefs:sketch_entity#{}:{}",
                    definition.id, segment.external_id
                ));
                profiles
                    .iter()
                    .flatten()
                    .any(|entity_use| entity_use.entity == id)
            })
            .map(|segment| segment.external_id)
            .collect::<BTreeSet<_>>();
        let sketch_id = SketchId(format!("creo:model:sketch#{}", definition.id));
        let mut entities = segments
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
                    construction: !solved.contains(&segment.external_id)
                        && !profile_segments.contains(&segment.external_id),
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
        for segment in segments
            .iter()
            .filter(|segment| !emitted.contains(&segment.external_id))
        {
            let id = SketchEntityId(format!(
                "creo:featdefs:sketch_entity#{}:{}",
                definition.id, segment.external_id
            ));
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                "unresolved_section_segment",
                Exactness::ByteExact,
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: true,
                native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
                geometry_ref: None,
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
                geometry: SketchGeometry::Native {
                    native_kind: match segment.kind {
                        crate::feature::FeatureSegmentKind::Line => "line",
                        crate::feature::FeatureSegmentKind::Arc => "arc",
                        crate::feature::FeatureSegmentKind::Point => "point",
                    }
                    .to_string(),
                },
            });
        }
        let mut saved_section_geometries = Vec::new();
        let mut generated_saved_geometries = Vec::new();
        for (internal_id, geometry, offset) in definition
            .saved_section
            .iter()
            .flat_map(|saved| &saved.entities)
            .filter_map(saved_section_entity_geometry)
        {
            let Some(external_id) = definition
                .order_table
                .as_ref()
                .and_then(|order| order.external_id(internal_id))
            else {
                continue;
            };
            let entity_id = SketchEntityId(format!(
                "creo:featdefs:sketch_entity#{}:{external_id}",
                definition.id
            ));
            if entities.iter().any(|entity| entity.id == entity_id) {
                continue;
            }
            let generated_kind = match &geometry {
                SketchGeometry::Line { .. } => crate::surface::SurfaceKind::Plane,
                SketchGeometry::Arc { .. } => crate::surface::SurfaceKind::Cylinder,
                _ => continue,
            };
            let generated = saved_entity_is_generated_profile(
                definition.owner_feature_id,
                external_id,
                generated_kind,
                &scan.feature_entity_tables,
                &scan.surface_rows,
            );
            let curve_id = CurveId(format!(
                "creo:featdefs:section_curve#{}:{external_id}",
                definition.id
            ));
            annotate(
                annotations,
                &entity_id.0,
                "FeatDefs",
                offset as u64,
                "saved_section_entity",
                Exactness::Derived,
            );
            if generated {
                generated_saved_geometries.push((external_id, geometry.clone()));
            }
            entities.push(SketchEntity {
                id: entity_id,
                sketch: sketch_id.clone(),
                construction: !generated,
                native_ref: Some(format!("creo:featdefs:saved_entity#{internal_id}")),
                geometry_ref: Some(curve_id.0.clone()),
                endpoint_refs: Vec::new(),
                geometry: geometry.clone(),
            });
            saved_section_geometries.push((external_id, geometry, offset, curve_id));
        }
        profiles.extend(saved_profile_chains(
            definition.id,
            &generated_saved_geometries,
        ));
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
            if nurbs
                .control_points
                .iter()
                .any(|point| point.z.abs() > 1e-12)
            {
                continue;
            }
            annotate(
                annotations,
                &entity_id.0,
                "FeatDefs",
                spline.offset as u64,
                "saved_interpolation_spline",
                Exactness::Derived,
            );
            entities.push(SketchEntity {
                id: entity_id,
                sketch: sketch_id.clone(),
                construction: spline.entity_id.is_none(),
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
        }
        if entities.is_empty() {
            continue;
        }
        for segment in segments {
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
        for (external_id, section_geometry, offset, id) in saved_section_geometries {
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry) else {
                continue;
            };
            annotate(
                annotations,
                &id,
                "FeatDefs",
                offset as u64,
                "placed_saved_section_curve",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("FeatDefs:section#{}:{external_id}", definition.id),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        let mut constraints = segments
            .iter()
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
        for (constraint, offset) in section_dimension_constraints(definition, &sketch_id) {
            annotate(
                annotations,
                &constraint.id.0,
                "FeatDefs",
                offset as u64,
                "section_dimension_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
        for (constraint, offset) in section_skamp_constraints(definition, &sketch_id) {
            if constraints
                .iter()
                .any(|existing| existing.definition == constraint.definition)
            {
                continue;
            }
            annotate(
                annotations,
                &constraint.id.0,
                "FeatDefs",
                offset as u64,
                "section_solver_constraint",
                Exactness::ByteExact,
            );
            constraints.push(constraint);
        }
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
            profiles,
            native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
        });
    }
}

fn surface_kind_for_geometry(geometry: &SurfaceGeometry) -> Option<crate::surface::SurfaceKind> {
    match geometry {
        SurfaceGeometry::Plane { .. } => Some(crate::surface::SurfaceKind::Plane),
        SurfaceGeometry::Cylinder { .. } => Some(crate::surface::SurfaceKind::Cylinder),
        SurfaceGeometry::Cone { .. } => Some(crate::surface::SurfaceKind::Cone),
        SurfaceGeometry::Sphere { .. } | SurfaceGeometry::Torus { .. } => {
            Some(crate::surface::SurfaceKind::TorusOrSphere)
        }
        SurfaceGeometry::Nurbs(_) => Some(crate::surface::SurfaceKind::Spline),
        SurfaceGeometry::Unknown { .. } => None,
    }
}

fn generated_surface_id(
    table: &crate::feature::FeatureEntityTable,
    source_entity_id: u32,
) -> Option<u32> {
    let mut matches = table
        .entries
        .iter()
        .filter(|entry| entry.source_entity_id == Some(source_entity_id))
        .map(|entry| entry.entity_id);
    let surface_id = matches.next()?;
    matches.next().is_none().then_some(())?;
    table
        .surface_ids
        .contains(&surface_id)
        .then_some(surface_id)
}

fn saved_entity_is_generated_profile(
    feature_id: Option<u32>,
    source_entity_id: u32,
    expected_kind: crate::surface::SurfaceKind,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> bool {
    let Some(feature_id) = feature_id else {
        return false;
    };
    tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| &table.entries)
        .filter(|entry| entry.source_entity_id == Some(source_entity_id))
        .any(|entry| {
            rows.iter().any(|row| {
                row.id == entry.entity_id
                    && row.feature_id == feature_id
                    && row.kind == expected_kind
            })
        })
}

fn ordered_line_surface_id(
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
    table: &crate::feature::FeatureEntityTable,
    order: &crate::feature::FeatureOrderTable,
    external_id: u32,
    geometry: &SurfaceGeometry,
) -> Option<u32> {
    order.internal_id(external_id)?;
    let surface_id = generated_surface_id(table, external_id)?;
    let expected_kind = surface_kind_for_geometry(geometry)?;
    surface_rows
        .iter()
        .any(|row| {
            row.id == surface_id && row.feature_id == feature_id && row.kind == expected_kind
        })
        .then_some(surface_id)
}

fn ordered_family_surface_bindings(
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
    table: &crate::feature::FeatureEntityTable,
    order: &crate::feature::FeatureOrderTable,
    external_ids: impl IntoIterator<Item = u32>,
    expected_kind: crate::surface::SurfaceKind,
) -> BTreeMap<u32, u32> {
    let mut bindings = BTreeMap::new();
    let mut bound_surfaces = BTreeSet::new();
    for external_id in external_ids {
        if order.internal_id(external_id).is_none() {
            return BTreeMap::new();
        }
        let Some(surface_id) = generated_surface_id(table, external_id) else {
            return BTreeMap::new();
        };
        if !surface_rows.iter().any(|row| {
            row.id == surface_id && row.feature_id == feature_id && row.kind == expected_kind
        }) || !bound_surfaces.insert(surface_id)
        {
            return BTreeMap::new();
        }
        bindings.insert(external_id, surface_id);
    }
    bindings
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
        let native_table = scan
            .feature_entity_tables
            .iter()
            .filter(|table| table.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        let native_table = match native_table.as_slice() {
            [table] => Some(*table),
            _ => None,
        };
        let points = resolved_section_points(definition);
        let solved_ids = definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        let arc_bindings = definition
            .order_table
            .as_ref()
            .zip(native_table)
            .map_or_else(BTreeMap::new, |(order, table)| {
                ordered_family_surface_bindings(
                    &scan.surface_rows,
                    feature_id,
                    table,
                    order,
                    definition
                        .segments
                        .iter()
                        .flat_map(|segments| &segments.rows)
                        .filter(|segment| {
                            solved_ids.contains(&segment.external_id)
                                && segment.kind == crate::feature::FeatureSegmentKind::Arc
                        })
                        .map(|segment| segment.external_id),
                    crate::surface::SurfaceKind::TorusOrSphere,
                )
            });
        let spline_bindings = definition
            .order_table
            .as_ref()
            .zip(native_table)
            .map_or_else(BTreeMap::new, |(order, table)| {
                ordered_family_surface_bindings(
                    &scan.surface_rows,
                    feature_id,
                    table,
                    order,
                    definition
                        .saved_section
                        .iter()
                        .flat_map(|saved| &saved.entities)
                        .filter_map(|entity| match entity {
                            crate::feature::FeatureSavedEntity::Spline(spline) => {
                                order.external_id(spline.entity_id?)
                            }
                            _ => None,
                        }),
                    crate::surface::SurfaceKind::Spline,
                )
            });
        for segment in definition
            .segments
            .iter()
            .flat_map(|table| &table.rows)
            .filter(|segment| solved_ids.contains(&segment.external_id))
        {
            let Some(geometry) = resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(surface) = revolved_section_surface(transform, &geometry, axis) else {
                continue;
            };
            let native_surface = match segment.kind {
                crate::feature::FeatureSegmentKind::Line => definition
                    .order_table
                    .as_ref()
                    .zip(native_table)
                    .and_then(|(order, table)| {
                        ordered_line_surface_id(
                            &scan.surface_rows,
                            feature_id,
                            table,
                            order,
                            segment.external_id,
                            &surface,
                        )
                    }),
                crate::feature::FeatureSegmentKind::Arc => {
                    arc_bindings.get(&segment.external_id).copied()
                }
                crate::feature::FeatureSegmentKind::Point => None,
            };
            let surface_id = native_surface.map_or_else(
                || {
                    SurfaceId(format!(
                        "creo:feature:revolution_surface#{feature_id}:segment{}",
                        segment.external_id
                    ))
                },
                |id| SurfaceId(format!("creo:visibgeom:surface#{id}")),
            );
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                segment.offset as u64,
                "evaluated_analytic_revolution_surface",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id,
                geometry: surface,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: native_surface.map_or_else(
                        || {
                            format!(
                                "FeatDefs:revolution#{feature_id}:segment{}",
                                segment.external_id
                            )
                        },
                        |id| format!("VisibGeom:{id}"),
                    ),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
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
            let native_surface = definition
                .order_table
                .as_ref()
                .and_then(|order| order.external_id(spline.entity_id?))
                .and_then(|external_id| spline_bindings.get(&external_id).copied());
            let Some(native_surface) = native_surface else {
                continue;
            };
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{native_surface}"));
            let procedural_id = ProceduralSurfaceId(format!(
                "creo:feature:revolution_construction#{feature_id}:{suffix}"
            ));
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
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
                    object_id: format!("VisibGeom:{native_surface}"),
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
                id: id.clone(),
                owner: owner.clone(),
                ordinal: ordinal as u32,
                name: format!("d{}", dimension.external_id),
                expression,
                display: (dimension.dimension_type == 0x03).then_some(DimensionDisplay::Radius),
                value: Some(value),
                dependencies: Vec::new(),
                properties,
                pmi: None,
                native_ref: Some(format!("creo:featdefs:sketch#{}", definition.id)),
            });
            if let Some(feature) = ir
                .model
                .features
                .iter_mut()
                .find(|feature| feature.id == owner)
            {
                feature
                    .source_content
                    .push(FeatureSourceContent::Parameter(id));
            }
        }
    }
}

fn feature_output_bodies(scan: &ContainerScan, ir: &CadIr, feature_id: u32) -> Vec<BodyId> {
    let generated_surfaces = scan
        .surface_rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .map(|row| SurfaceId(format!("creo:visibgeom:surface#{}", row.id)))
        .chain(
            scan.feature_entity_tables
                .iter()
                .filter(|table| table.feature_id == Some(feature_id))
                .flat_map(|table| &table.surface_ids)
                .map(|surface_id| SurfaceId(format!("creo:visibgeom:surface#{surface_id}"))),
        )
        .chain(
            scan.feature_affected_ids
                .iter()
                .filter(|record| {
                    record.feature_id == feature_id
                        && record.kind == crate::feature::AffectedIdKind::Geometry
                })
                .flat_map(|record| &record.ids)
                .map(|surface_id| SurfaceId(format!("creo:visibgeom:surface#{surface_id}"))),
        );
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
            "replay_affected_geometry_ids",
            affected
                .geometry_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_affected_edge_ids",
            affected
                .edge_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_geometry_extent",
            match affected.geometry_extent {
                crate::feature::ReplayExtentSource::Explicit => "explicit",
                crate::feature::ReplayExtentSource::Inherited => "inherited",
            }
            .to_string(),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_edge_extent",
            match affected.edge_extent {
                crate::feature::ReplayExtentSource::Explicit => "explicit",
                crate::feature::ReplayExtentSource::Inherited => "inherited",
            }
            .to_string(),
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
    for table in scan
        .feature_entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
    {
        for entry in &table.entries {
            let Some(source_entity_id) = entry.source_entity_id else {
                continue;
            };
            insert_feature_parameter(
                &mut parameters,
                &format!(
                    "generated_entity.{}.source_section_entity_id",
                    entry.entity_id
                ),
                source_entity_id.to_string(),
            );
            insert_feature_parameter(
                &mut parameters,
                &format!("generated_entity.{}.entry_class", entry.entity_id),
                entry.class_id.to_string(),
            );
        }
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
        914 => Some("Chamfer"),
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

fn feature_edge_selection(scan: &ContainerScan, feature_id: u32) -> Option<EdgeSelection> {
    if let Some(ids) = scan
        .feature_affected_ids
        .iter()
        .find(|record| {
            record.feature_id == feature_id && record.kind == crate::feature::AffectedIdKind::Edges
        })
        .map(|record| record.ids.as_slice())
    {
        if ids.is_empty() {
            return None;
        }
        return Some(EdgeSelection::Native(format!(
            "creo:allfeatur:edgs_affected#{feature_id}:{}",
            ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
        )));
    }

    let replay_edges = scan
        .feature_replay_affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id && !record.edge_ids.is_empty())
        .map(|record| record.edge_ids.clone())
        .collect::<BTreeSet<_>>();
    let replay_edges = replay_edges.into_iter().collect::<Vec<_>>();
    let [ids] = replay_edges.as_slice() else {
        return None;
    };
    Some(EdgeSelection::Native(format!(
        "creo:allfeatur:replay_edgs_affected#{feature_id}:{}",
        ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
    )))
}

fn parallel_support_radius(planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>) -> Option<f64> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let mut radii = Vec::new();
    for first in 0..planes.len() {
        for second in first + 1..planes.len() {
            let first_normal = normalized(planes[first].1)?;
            let second_normal = normalized(planes[second].1)?;
            let alignment = first_normal
                .iter()
                .zip(second_normal)
                .map(|(first, second)| first * second)
                .sum::<f64>();
            if alignment.abs() < 1.0 - 1e-9 {
                continue;
            }
            let gap = planes[second]
                .0
                .iter()
                .zip(planes[first].0)
                .zip(first_normal)
                .map(|((second, first), normal)| (second - first) * normal)
                .sum::<f64>()
                .abs();
            let scale = planes[first]
                .0
                .iter()
                .chain(&planes[second].0)
                .map(|value| value.abs())
                .fold(1.0, f64::max);
            if gap > 1e-9 * scale {
                radii.push(0.5 * gap);
            }
        }
    }
    let radius = *radii.first()?;
    let scale = radius.abs().max(1.0);
    radii
        .iter()
        .all(|candidate| (candidate - radius).abs() <= 1e-9 * scale)
        .then_some(radius)
}

fn round_constant_radius(scan: &ContainerScan, ir: &CadIr, feature_id: u32) -> Option<f64> {
    let cylinder_rows = scan
        .surface_rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
        .collect::<Vec<_>>();
    if cylinder_rows.is_empty() {
        return None;
    }
    let cylinder_radii = cylinder_rows
        .iter()
        .filter_map(|row| {
            let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
            ir.model
                .surfaces
                .iter()
                .find(|surface| surface.id == id)
                .and_then(|surface| match surface.geometry {
                    SurfaceGeometry::Cylinder { radius, .. } => Some(radius),
                    _ => None,
                })
        })
        .collect::<Vec<_>>();
    if cylinder_radii.len() == cylinder_rows.len() {
        return unique_positive_length(&cylinder_radii);
    }
    let named_records = scan
        .feature_affected_ids
        .iter()
        .filter(|record| {
            record.feature_id == feature_id
                && record.kind == crate::feature::AffectedIdKind::Geometry
        })
        .collect::<Vec<_>>();
    let replay_records = scan
        .feature_replay_affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
        .collect::<Vec<_>>();
    let affected_ids = match (named_records.as_slice(), replay_records.as_slice()) {
        ([record], _) => record.ids.as_slice(),
        ([], [record]) => record.geometry_ids.as_slice(),
        _ => return None,
    };
    let support_ids = affected_ids.get(2..)?;
    let support_planes = support_ids
        .iter()
        .filter_map(|id| {
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{id}"));
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == surface_id)?;
            match &surface.geometry {
                SurfaceGeometry::Plane { origin, normal, .. } => Some((
                    [origin.x, origin.y, origin.z],
                    [normal.x, normal.y, normal.z],
                )),
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    (support_planes.len() == support_ids.len()).then(|| parallel_support_radius(support_planes))?
}

fn unique_positive_length(values: &[f64]) -> Option<f64> {
    let value = *values.first()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let scale = values
        .iter()
        .copied()
        .map(f64::abs)
        .fold(value.abs().max(1.0), f64::max);
    values
        .iter()
        .all(|candidate| {
            candidate.is_finite() && *candidate > 0.0 && (*candidate - value).abs() <= 1e-9 * scale
        })
        .then_some(value)
}

fn schema_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    schema_class: u32,
    kind: &str,
) -> IrFeatureDefinition {
    if schema_class == 911 {
        let placement = hole_placement(feature_outline_planes(scan, feature_id));
        let solved = simple_hole_geometry(scan, feature_id);
        let (face, position, direction, diameter, extent) = solved.map_or_else(
            || {
                placement.map_or(
                    (None, None, None, None, None),
                    |(entry_surface_id, direction, extent)| {
                        (
                            Some(FaceSelection::Native(format!(
                                "creo:visibgeom:surface#{entry_surface_id}"
                            ))),
                            None,
                            Some(Vector3::new(direction[0], direction[1], direction[2])),
                            None,
                            Some(extent),
                        )
                    },
                )
            },
            |hole| {
                let SurfaceGeometry::Cylinder { origin, radius, .. } = hole.geometry else {
                    unreachable!("simple hole helper returns a cylinder")
                };
                (
                    Some(FaceSelection::Native(format!(
                        "creo:visibgeom:surface#{}",
                        hole.entry_surface_id
                    ))),
                    Some(origin),
                    Some(Vector3::new(
                        hole.direction[0],
                        hole.direction[1],
                        hole.direction[2],
                    )),
                    Some(Length(2.0 * radius)),
                    Some(hole.extent),
                )
            },
        );
        return IrFeatureDefinition::Hole {
            face,
            position,
            direction,
            kind: if diameter.is_some() {
                HoleKind::Simple
            } else {
                HoleKind::Unresolved {
                    form: None,
                    counterbore_diameter: None,
                    counterbore_depth: None,
                    countersink_diameter: None,
                    countersink_angle: None,
                }
            },
            diameter,
            extent,
        };
    }
    if schema_class == 913 {
        return IrFeatureDefinition::Fillet {
            edges: feature_edge_selection(scan, feature_id).unwrap_or(EdgeSelection::Unresolved),
            radius: round_constant_radius(scan, ir, feature_id).map_or(
                RadiusSpec::Unresolved { form: None },
                |radius| RadiusSpec::Constant {
                    radius: Length(radius),
                },
            ),
        };
    }
    if schema_class == 914 {
        return IrFeatureDefinition::Chamfer {
            edges: feature_edge_selection(scan, feature_id).unwrap_or(EdgeSelection::Unresolved),
            spec: ChamferSpec::Unresolved { form: None },
        };
    }
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
                    op: section_sweep_boolean_operation(
                        kind,
                        false,
                        preceding_features_establish_body(ir),
                    ),
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
                let cap_origins = feature_plane_equations(scan, feature_id);
                if let Some((extent, direction)) =
                    extrusion_extent_and_direction(transform.origin, transform.normal, cap_origins)
                {
                    return IrFeatureDefinition::Extrude {
                        profile,
                        direction: Some(Vector3::new(direction[0], direction[1], direction[2])),
                        extent,
                        op: section_sweep_boolean_operation(
                            kind,
                            ir.model.bodies.iter().any(|body| {
                                body.id
                                    == BodyId(format!("creo:feature:extrusion#{feature_id}:body"))
                            }),
                            preceding_features_establish_body(ir),
                        ),
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

fn preceding_features_establish_body(ir: &CadIr) -> bool {
    ir.model.features.iter().any(|feature| {
        matches!(
            feature.definition,
            IrFeatureDefinition::Extrude { .. }
                | IrFeatureDefinition::Revolve { .. }
                | IrFeatureDefinition::Hole { .. }
                | IrFeatureDefinition::Fillet { .. }
                | IrFeatureDefinition::Chamfer { .. }
        )
    })
}

fn section_sweep_boolean_operation(kind: &str, creates_body: bool, prior_body: bool) -> BooleanOp {
    if creates_body {
        return BooleanOp::NewBody;
    }
    match kind {
        "Protrusion" if prior_body => BooleanOp::Join,
        "Cut" => BooleanOp::Cut,
        _ => BooleanOp::Unresolved,
    }
}

fn named_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    kind: &str,
) -> Option<IrFeatureDefinition> {
    if let Some(role) = match kind {
        "Annotation Feature" => Some(FeatureTreeNodeRole::Annotations),
        "Cross Section" | "Querschnitt" => Some(FeatureTreeNodeRole::CrossSections),
        _ => None,
    } {
        return Some(IrFeatureDefinition::TreeNode { role });
    }
    if kind == "Mirror" {
        return Some(IrFeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Mirror),
            },
        });
    }
    let schema_class = match kind {
        "Datum Plane" | "Bezugsebene" => 923,
        "Hole" => 911,
        "Round" | "Rundung" => 913,
        "Chamfer" => 914,
        _ => return None,
    };
    Some(schema_feature_definition(
        scan,
        ir,
        feature_id,
        schema_class,
        kind,
    ))
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

fn hole_extent_and_direction(
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<([f64; 3], Extent)> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let [(first_origin, first_normal), (second_origin, second_normal)] = planes.as_slice() else {
        return None;
    };
    let first_normal = normalized(*first_normal)?;
    let second_normal = normalized(*second_normal)?;
    let alignment = first_normal
        .iter()
        .zip(second_normal)
        .map(|(first, second)| first * second)
        .sum::<f64>()
        .abs();
    if (alignment - 1.0).abs() > 1e-9 {
        return None;
    }
    let signed_length = second_origin
        .iter()
        .zip(first_origin)
        .zip(first_normal)
        .map(|((second, first), axis)| (second - first) * axis)
        .sum::<f64>();
    let scale = second_origin
        .iter()
        .chain(first_origin)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if signed_length.abs() <= 1e-9 * scale {
        return None;
    }
    Some((
        first_normal.map(|value| value * signed_length.signum()),
        Extent::Blind {
            length: Length(signed_length.abs()),
        },
    ))
}

fn hole_placement(
    planes: impl IntoIterator<Item = (u32, [f64; 3], [f64; 3])>,
) -> Option<(u32, [f64; 3], Extent)> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let [(entry_id, entry_origin, entry_normal), (_, termination_origin, termination_normal)] =
        planes.as_slice()
    else {
        return None;
    };
    let (direction, extent) = hole_extent_and_direction([
        (*entry_origin, *entry_normal),
        (*termination_origin, *termination_normal),
    ])?;
    Some((*entry_id, direction, extent))
}

fn plane_envelope_corners(envelope: &crate::surface::PlaneEnvelope) -> Option<[[f64; 3]; 2]> {
    let corners = match envelope {
        crate::surface::PlaneEnvelope::Standard { corners_3d, .. }
        | crate::surface::PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
    };
    Some([
        [corners[0][0]?, corners[0][1]?, corners[0][2]?],
        [corners[1][0]?, corners[1][1]?, corners[1][2]?],
    ])
}

type HoleCapOutline = (u32, [f64; 3], [f64; 3], [[f64; 3]; 2]);
type PartialCapOutline = (u32, [f64; 3], [f64; 3], Option<[[f64; 3]; 2]>);

fn cap_square_center_radius(corners: [[f64; 3]; 2], axis_index: usize) -> Option<([f64; 3], f64)> {
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let spans = [
        (corners[1][radial[0]] - corners[0][radial[0]]).abs(),
        (corners[1][radial[1]] - corners[0][radial[1]]).abs(),
    ];
    let scale = spans[0]
        .max(spans[1])
        .max(corners[0][axis_index].abs())
        .max(corners[1][axis_index].abs())
        .max(1.0);
    if (corners[1][axis_index] - corners[0][axis_index]).abs() > 1e-9 * scale
        || spans[0] <= 1e-9
        || (spans[0] - spans[1]).abs() > 1e-9 * scale
    {
        return None;
    }
    Some((
        std::array::from_fn(|index| 0.5 * (corners[0][index] + corners[1][index])),
        0.5 * spans[0],
    ))
}

fn hole_cylinder_from_cap_outlines(caps: [HoleCapOutline; 2]) -> Option<SurfaceGeometry> {
    let placement = hole_placement(caps.map(|(id, origin, normal, _)| (id, origin, normal)))?;
    let axis = placement.1;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - 1e-9
            && (0..3).all(|other| other == *index || axis[other].abs() < 1e-9)
    })?;
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let mut centers = Vec::<[f64; 3]>::new();
    let mut radii = Vec::new();
    for (_, _, _, corners) in caps {
        let (center, radius) = cap_square_center_radius(corners, axis_index)?;
        centers.push(center);
        radii.push(radius);
    }
    let scale = centers
        .iter()
        .flatten()
        .chain(&radii)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if radial
        .iter()
        .any(|index| (centers[0][*index] - centers[1][*index]).abs() > 1e-9 * scale)
        || (radii[0] - radii[1]).abs() > 1e-9 * scale
    {
        return None;
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Some(SurfaceGeometry::Cylinder {
        origin: Point3::new(centers[0][0], centers[0][1], centers[0][2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius: radii[0],
    })
}

#[derive(Debug, Clone, PartialEq)]
struct SimpleHoleGeometry {
    entry_surface_id: u32,
    cylinder_ids: [u32; 2],
    direction: [f64; 3],
    extent: Extent,
    geometry: SurfaceGeometry,
}

fn simple_hole_geometry(scan: &ContainerScan, feature_id: u32) -> Option<SimpleHoleGeometry> {
    let cap_rows = feature_outline_planes(scan, feature_id)
        .into_iter()
        .map(|(id, origin, normal)| {
            let envelopes = scan
                .plane_envelopes
                .iter()
                .filter(|envelope| envelope.surface_id == id)
                .collect::<Vec<_>>();
            let [envelope] = envelopes.as_slice() else {
                return None;
            };
            Some((
                id,
                origin,
                normal,
                plane_envelope_corners(&envelope.envelope)?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let [first, second] = cap_rows.as_slice() else {
        return None;
    };
    let tables = scan
        .feature_entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let [entry_plane, termination_plane, first_cylinder, second_cylinder] =
        table.entry_ids.as_slice()
    else {
        return None;
    };
    if *entry_plane != first.0 || *termination_plane != second.0 {
        return None;
    }
    let cylinder_ids = [*first_cylinder, *second_cylinder];
    if cylinder_ids.iter().any(|id| {
        !scan.surface_rows.iter().any(|row| {
            row.id == *id
                && row.feature_id == feature_id
                && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    }) {
        return None;
    }
    let (_, direction, extent) =
        hole_placement([*first, *second].map(|(id, origin, normal, _)| (id, origin, normal)))?;
    Some(SimpleHoleGeometry {
        entry_surface_id: *entry_plane,
        cylinder_ids,
        direction,
        extent,
        geometry: hole_cylinder_from_cap_outlines([*first, *second])?,
    })
}

fn circular_sweep_cylinder_from_cap_outlines(
    caps: [PartialCapOutline; 2],
) -> Option<SurfaceGeometry> {
    let (_, axis, _) = hole_placement(caps.map(|(id, origin, normal, _)| (id, origin, normal)))?;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - 1e-9
            && (0..3).all(|other| other == *index || axis[other].abs() < 1e-9)
    })?;
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let circles = caps
        .iter()
        .filter_map(|(_, _, _, corners)| cap_square_center_radius((*corners)?, axis_index))
        .collect::<Vec<_>>();
    let (center, radius) = *circles.first()?;
    let scale = center
        .iter()
        .chain(std::iter::once(&radius))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if circles.iter().skip(1).any(|(other_center, other_radius)| {
        radial
            .iter()
            .any(|index| (center[*index] - other_center[*index]).abs() > 1e-9 * scale)
            || (radius - other_radius).abs() > 1e-9 * scale
    }) {
        return None;
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Some(SurfaceGeometry::Cylinder {
        origin: Point3::new(center[0], center[1], center[2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius,
    })
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
        if offset.abs() <= 1e-12 {
            continue;
        }
        let scale = offset.abs().max(1.0);
        if !offsets
            .iter()
            .any(|known| (known - offset).abs() <= 1e-9 * scale)
        {
            offsets.push(offset);
        }
    }
    let lower = offsets
        .iter()
        .copied()
        .filter(|offset| *offset < 0.0)
        .min_by(f64::total_cmp);
    let upper = offsets
        .iter()
        .copied()
        .filter(|offset| *offset > 0.0)
        .max_by(f64::total_cmp);
    match (lower, upper) {
        (Some(lower), Some(upper)) => Some(ExtrusionSpan { lower, upper }),
        (Some(lower), None) => Some(ExtrusionSpan { lower, upper: 0.0 }),
        (None, Some(upper)) => Some(ExtrusionSpan { lower: 0.0, upper }),
        (None, None) => None,
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
    fn geometry_signal_excludes_opaque_carriers() {
        let mut ir = CadIr::empty(Units::default());
        let surface_id = SurfaceId("surface".to_string());
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
        ir.model.curves.push(Curve {
            id: CurveId("curve".to_string()),
            geometry: CurveGeometry::Unknown { record: None },
            source_object: None,
        });

        assert!(!has_transferred_geometry(&ir));

        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId("procedural".to_string()),
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Exact {
                parameter_ranges: [[0.0, 1.0], [0.0, 1.0]],
                extension: 0,
            },
            cache_fit_tolerance: None,
        });

        assert!(has_transferred_geometry(&ir));
    }

    #[test]
    fn fc05_row_frame_maps_cyclically_onto_each_model_axis() {
        let center = [11.0, 13.0];
        let reference = [0.6, 0.8];
        assert_eq!(
            fc05_model_frame(0, 17.0, center, reference, -1.0),
            ([17.0, 13.0, 11.0], [-1.0, 0.0, 0.0], [0.0, 0.8, 0.6])
        );
        assert_eq!(
            fc05_model_frame(1, 17.0, center, reference, -1.0),
            ([11.0, 17.0, 13.0], [0.0, -1.0, 0.0], [0.6, 0.0, 0.8])
        );
        assert_eq!(
            fc05_model_frame(2, 17.0, center, reference, -1.0),
            ([13.0, 11.0, 17.0], [0.0, 0.0, -1.0], [0.8, 0.6, 0.0])
        );
    }

    #[test]
    fn full_turn_section_carriers_classify_analytic_revolution_surfaces() {
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 1,
            feature_id: Some(2),
            origin: [0.0, 0.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            offset: 0,
        };
        let axis = RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
        };
        let line = |start: [f64; 2], end: [f64; 2]| SketchGeometry::Line {
            start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
            end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
        };

        assert!(matches!(
            revolved_section_surface(&transform, &line([2.0, 0.0], [2.0, 4.0]), axis),
            Some(SurfaceGeometry::Cylinder { radius, .. }) if radius == 2.0
        ));
        assert!(matches!(
            revolved_section_surface(&transform, &line([0.0, 3.0], [4.0, 3.0]), axis),
            Some(SurfaceGeometry::Plane { origin, .. }) if origin.y == 3.0
        ));
        assert!(matches!(
            revolved_section_surface(&transform, &line([2.0, 0.0], [4.0, 2.0]), axis),
            Some(SurfaceGeometry::Cone { radius, half_angle, .. })
                if radius == 2.0 && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
        ));
        assert!(matches!(
            revolved_section_surface(&transform, &line([4.0, 0.0], [2.0, 2.0]), axis),
            Some(SurfaceGeometry::Cone { axis, radius, half_angle, .. })
                if axis.y == -1.0
                    && radius == 4.0
                    && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
        ));
        let centered_arc = SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 3.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        };
        assert!(matches!(
            revolved_section_surface(&transform, &centered_arc, axis),
            Some(SurfaceGeometry::Sphere { radius, .. }) if radius == 2.0
        ));
        let offset_arc = SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(5.0, 3.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        };
        assert!(matches!(
            revolved_section_surface(&transform, &offset_arc, axis),
            Some(SurfaceGeometry::Torus { major_radius, minor_radius, .. })
                if major_radius == 5.0 && minor_radius == 2.0
        ));
    }

    #[test]
    fn generated_source_ids_bind_carriers_independently_of_table_position() {
        let table = crate::feature::FeatureEntityTable {
            feature_id: Some(17),
            entry_ids: vec![42, 41, 43],
            entries: vec![
                crate::feature::FeatureEntityTableEntry {
                    entity_id: 42,
                    class_id: 200,
                    source_entity_id: Some(10),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::FeatureEntityTableEntry {
                    entity_id: 41,
                    class_id: 200,
                    source_entity_id: Some(8),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::FeatureEntityTableEntry {
                    entity_id: 43,
                    class_id: 200,
                    source_entity_id: Some(9),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
            ],
            surface_ids: vec![41, 42, 43],
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        };
        let order = crate::feature::FeatureOrderTable {
            declared_count: 2,
            entity_ref: Some(3),
            rows: vec![
                crate::feature::FeatureOrderRow {
                    external_id: 8,
                    internal_id: 1,
                    bitmask: 0,
                    offset: 0,
                },
                crate::feature::FeatureOrderRow {
                    external_id: 9,
                    internal_id: 2,
                    bitmask: 0,
                    offset: 0,
                },
            ],
            offset: 0,
        };
        let row = |id, kind| crate::surface::SurfaceRow {
            id,
            kind,
            feature_id: 17,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 0,
        };
        let rows = vec![
            row(41, crate::surface::SurfaceKind::Cylinder),
            row(42, crate::surface::SurfaceKind::Cone),
            row(43, crate::surface::SurfaceKind::TorusOrSphere),
        ];
        let cylinder = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        assert_eq!(
            ordered_line_surface_id(&rows, 17, &table, &order, 8, &cylinder),
            Some(41)
        );
        assert_eq!(
            ordered_line_surface_id(&rows, 17, &table, &order, 9, &cylinder),
            None
        );
        assert_eq!(
            ordered_family_surface_bindings(
                &rows,
                17,
                &table,
                &order,
                [9],
                crate::surface::SurfaceKind::TorusOrSphere,
            ),
            BTreeMap::from([(9, 43)])
        );
        assert!(saved_entity_is_generated_profile(
            Some(17),
            8,
            crate::surface::SurfaceKind::Cylinder,
            std::slice::from_ref(&table),
            &rows,
        ));
        assert!(!saved_entity_is_generated_profile(
            Some(17),
            10,
            crate::surface::SurfaceKind::Cylinder,
            &[table],
            &rows,
        ));
    }

    #[test]
    fn rowless_round_cylinder_requires_the_four_entry_sibling_layout() {
        let row = |id, kind| crate::surface::SurfaceRow {
            id,
            kind,
            feature_id: 23,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 0,
        };
        let rows = vec![
            row(10, crate::surface::SurfaceKind::Plane),
            row(11, crate::surface::SurfaceKind::Plane),
            row(13, crate::surface::SurfaceKind::Cylinder),
        ];
        let table = crate::feature::FeatureEntityTable {
            feature_id: Some(23),
            entry_ids: vec![10, 11, 12, 13],
            entries: Vec::new(),
            surface_ids: vec![10, 11, 13],
            non_surface_entity_ids: vec![12],
            offset: 47,
        };
        assert_eq!(
            rowless_round_cylinder_pairs(
                &BTreeSet::from([23]),
                std::slice::from_ref(&table),
                &rows,
            ),
            vec![(12, 13, 47)]
        );
        assert!(rowless_round_cylinder_pairs(
            &BTreeSet::new(),
            std::slice::from_ref(&table),
            &rows,
        )
        .is_empty());
        let mut materialized_rowless = rows;
        materialized_rowless.push(row(12, crate::surface::SurfaceKind::Cylinder));
        assert!(rowless_round_cylinder_pairs(
            &BTreeSet::from([23]),
            &[table],
            &materialized_rowless,
        )
        .is_empty());
    }

    #[test]
    fn spline_extrusion_preserves_directrix_basis_and_weights() {
        let directrix = NurbsCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point3::new(1.0, 2.0, 3.0),
                Point3::new(4.0, 5.0, 6.0),
                Point3::new(7.0, 8.0, 9.0),
            ],
            weights: Some(vec![1.0, 0.5, 1.0]),
            periodic: false,
        };
        let surface =
            extruded_nurbs_surface(&directrix, [0.0, 0.0, 4.0]).expect("valid extrusion surface");

        assert_eq!((surface.u_degree, surface.v_degree), (2, 1));
        assert_eq!((surface.u_count, surface.v_count), (3, 2));
        assert_eq!(surface.u_knots, directrix.knots);
        assert_eq!(surface.v_knots, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            surface.control_points,
            [
                Point3::new(1.0, 2.0, 3.0),
                Point3::new(1.0, 2.0, 7.0),
                Point3::new(4.0, 5.0, 6.0),
                Point3::new(4.0, 5.0, 10.0),
                Point3::new(7.0, 8.0, 9.0),
                Point3::new(7.0, 8.0, 13.0),
            ]
        );
        assert_eq!(surface.weights, Some(vec![1.0, 1.0, 0.5, 0.5, 1.0, 1.0]));
    }

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
    fn extrusion_arc_pcurve_is_exact_in_both_directions() {
        for (start, end, expected_middle) in [
            (0.0, std::f64::consts::PI, Point2::new(2.0, 5.0)),
            (std::f64::consts::PI, 0.0, Point2::new(2.0, 5.0)),
        ] {
            let pcurve = circular_pcurve([2.0, 2.0], 3.0, start, end);
            let first = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.0).expect("first endpoint");
            let middle = cadmpeg_ir::eval::pcurve_uv(&pcurve, 0.5).expect("arc midpoint");
            let last = cadmpeg_ir::eval::pcurve_uv(&pcurve, 1.0).expect("last endpoint");
            assert!((first.u - (2.0 + 3.0 * start.cos())).abs() < 1e-12);
            assert!((first.v - (2.0 + 3.0 * start.sin())).abs() < 1e-12);
            assert!((middle.u - expected_middle.u).abs() < 1e-12);
            assert!((middle.v - expected_middle.v).abs() < 1e-12);
            assert!((last.u - (2.0 + 3.0 * end.cos())).abs() < 1e-12);
            assert!((last.v - (2.0 + 3.0 * end.sin())).abs() < 1e-12);
        }
    }

    #[test]
    fn extrusion_profile_area_includes_oriented_arc_sector() {
        let arc = SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        };
        let line = SketchGeometry::Line {
            start: Point2::new(-1.0, 0.0),
            end: Point2::new(1.0, 0.0),
        };
        let counterclockwise = vec![
            (arc.clone(), false, [1.0, 0.0], [-1.0, 0.0]),
            (line.clone(), false, [-1.0, 0.0], [1.0, 0.0]),
        ];
        let clockwise = vec![
            (arc, true, [-1.0, 0.0], [1.0, 0.0]),
            (line, true, [1.0, 0.0], [-1.0, 0.0]),
        ];
        assert!(
            (extrusion_profile_signed_area(&counterclockwise).expect("positive area")
                - std::f64::consts::FRAC_PI_2)
                .abs()
                < 1e-12
        );
        assert!(
            (extrusion_profile_signed_area(&clockwise).expect("negative area")
                + std::f64::consts::FRAC_PI_2)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn extrusion_profiles_require_one_oppositely_oriented_hole() {
        let rectangle = |minimum: [f64; 2], maximum: [f64; 2], clockwise: bool| {
            let mut points = [
                minimum,
                [maximum[0], minimum[1]],
                maximum,
                [minimum[0], maximum[1]],
            ];
            if clockwise {
                points.reverse();
            }
            (0..4)
                .map(|index| {
                    let start = points[index];
                    let end = points[(index + 1) % 4];
                    (
                        SketchGeometry::Line {
                            start: Point2::new(start[0], start[1]),
                            end: Point2::new(end[0], end[1]),
                        },
                        false,
                        start,
                        end,
                    )
                })
                .collect::<ExtrusionProfile>()
        };
        let outer = rectangle([-2.0, -2.0], [2.0, 2.0], false);
        let hole = rectangle([-1.0, -1.0], [1.0, 1.0], true);
        let (profiles, outer_area) = ordered_extrusion_profiles(vec![hole.clone(), outer.clone()])
            .expect("strict outer and hole");
        assert_eq!(profiles[0], outer);
        assert!(outer_area > 0.0);
        assert!(extrusion_profile_signed_area(&profiles[1]).expect("hole area") < 0.0);

        assert!(ordered_extrusion_profiles(vec![
            rectangle([-2.0, -2.0], [2.0, 2.0], false),
            rectangle([-1.0, -1.0], [1.0, 1.0], false),
        ])
        .is_none());
        assert!(ordered_extrusion_profiles(vec![
            rectangle([-2.0, -2.0], [2.0, 2.0], false),
            rectangle([1.0, -1.0], [3.0, 1.0], true),
        ])
        .is_none());

        let circular_hole = [
            (std::f64::consts::PI, 0.0, [-0.5, 0.0], [0.5, 0.0]),
            (
                std::f64::consts::TAU,
                std::f64::consts::PI,
                [0.5, 0.0],
                [-0.5, 0.0],
            ),
        ]
        .into_iter()
        .map(|(end_angle, start_angle, start, end)| {
            (
                SketchGeometry::Arc {
                    center: Point2::new(0.0, 0.0),
                    radius: Length(0.5),
                    start_angle: Angle(start_angle),
                    end_angle: Angle(end_angle),
                },
                true,
                start,
                end,
            )
        })
        .collect::<ExtrusionProfile>();
        let (profiles, _) = ordered_extrusion_profiles(vec![
            circular_hole,
            rectangle([-2.0, -2.0], [2.0, 2.0], false),
        ])
        .expect("arc-bounded hole");
        assert!(matches!(profiles[1][0].0, SketchGeometry::Arc { .. }));
    }

    #[test]
    fn extrusion_profile_intersections_include_analytic_tangency() {
        let full_upper_circle = ([0.0, 0.0], 1.0, 0.0, std::f64::consts::PI);
        assert!(line_arc_intersect(
            [[-2.0, 1.0], [2.0, 1.0]],
            full_upper_circle,
            1e-9,
        ));
        assert!(!line_arc_intersect(
            [[-2.0, 1.1], [2.0, 1.1]],
            full_upper_circle,
            1e-9,
        ));
        assert!(arcs_intersect(
            full_upper_circle,
            ([2.0, 0.0], 1.0, std::f64::consts::PI, std::f64::consts::PI),
            1e-9,
        ));
        assert!(!arcs_intersect(
            full_upper_circle,
            ([3.0, 0.0], 1.0, std::f64::consts::PI, std::f64::consts::PI),
            1e-9,
        ));
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
    fn stored_section_sweep_family_defines_boolean_operation() {
        assert_eq!(
            section_sweep_boolean_operation("Protrusion", false, true),
            BooleanOp::Join
        );
        assert_eq!(
            section_sweep_boolean_operation("Cut", false, false),
            BooleanOp::Cut
        );
        assert_eq!(
            section_sweep_boolean_operation("Protrusion", true, false),
            BooleanOp::NewBody
        );
        assert_eq!(
            section_sweep_boolean_operation("Protrusion", false, false),
            BooleanOp::Unresolved
        );
        assert_eq!(
            section_sweep_boolean_operation("Körper", false, true),
            BooleanOp::Unresolved
        );
    }

    #[test]
    fn ordered_hole_cap_planes_define_blind_direction_and_depth() {
        assert_eq!(
            hole_extent_and_direction([
                ([2.0, -21.0, -0.75], [1.0, 0.0, 0.0]),
                ([5.0, -22.5, 0.75], [-1.0, 0.0, 0.0]),
            ]),
            Some((
                [1.0, 0.0, 0.0],
                Extent::Blind {
                    length: Length(3.0),
                },
            ))
        );
        assert_eq!(
            hole_extent_and_direction([
                ([0.0, 0.5, 0.0], [0.0, 1.0, 0.0]),
                ([0.0, -0.5, 0.0], [0.0, 1.0, 0.0]),
            ]),
            Some((
                [-0.0, -1.0, -0.0],
                Extent::Blind {
                    length: Length(1.0),
                },
            ))
        );
        assert_eq!(
            hole_extent_and_direction([
                ([0.0; 3], [1.0, 0.0, 0.0]),
                ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ]),
            None
        );

        assert_eq!(
            hole_placement([
                (902, [0.0, 0.0, 0.85], [0.0, 0.0, 1.0]),
                (905, [0.0, 0.0, 7.35], [0.0, 0.0, -1.0]),
            ]),
            Some((
                902,
                [0.0, 0.0, 1.0],
                Extent::Blind {
                    length: Length(6.5),
                },
            ))
        );
        assert_eq!(
            hole_placement([
                (902, [0.0; 3], [0.0, 0.0, 1.0]),
                (905, [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]),
                (908, [0.0, 0.0, 2.0], [0.0, 0.0, -1.0]),
            ]),
            None
        );
        assert!(matches!(
            hole_cylinder_from_cap_outlines([
                (
                    902,
                    [0.0, 0.0, 0.85],
                    [0.0, 0.0, 1.0],
                    [[-1.5, 17.5, 0.85], [1.5, 20.5, 0.85]],
                ),
                (
                    905,
                    [0.0, 0.0, 7.35],
                    [0.0, 0.0, -1.0],
                    [[-1.5, 17.5, 7.35], [1.5, 20.5, 7.35]],
                ),
            ]),
            Some(SurfaceGeometry::Cylinder { origin, axis, radius, .. })
                if origin == Point3::new(0.0, 19.0, 0.85)
                    && axis == Vector3::new(0.0, 0.0, 1.0)
                    && radius == 1.5
        ));
        assert!(hole_cylinder_from_cap_outlines([
            (
                902,
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [[-1.0, -2.0, 0.0], [1.0, 2.0, 0.0]],
            ),
            (
                905,
                [0.0, 0.0, 1.0],
                [0.0, 0.0, -1.0],
                [[-1.0, -2.0, 1.0], [1.0, 2.0, 1.0]],
            ),
        ])
        .is_none());
        assert!(matches!(
            circular_sweep_cylinder_from_cap_outlines([
                (
                    828,
                    [0.0, 4.0, 0.0],
                    [0.0, 1.0, 0.0],
                    Some([[-13.25, 4.0, -0.75], [-11.75, 4.0, 0.75]]),
                ),
                (
                    831,
                    [0.0, -4.0, 0.0],
                    [0.0, 1.0, 0.0],
                    None,
                ),
            ]),
            Some(SurfaceGeometry::Cylinder { origin, axis, radius, .. })
                if origin == Point3::new(-12.5, 4.0, 0.0)
                    && axis == Vector3::new(0.0, -1.0, 0.0)
                    && radius == 0.75
        ));
    }

    #[test]
    fn unique_parallel_round_supports_define_constant_radius() {
        assert_eq!(unique_positive_length(&[0.5, 0.5 + 1e-12]), Some(0.5));
        assert_eq!(unique_positive_length(&[0.5, 0.6]), None);
        assert_eq!(unique_positive_length(&[0.0]), None);
        assert_eq!(
            parallel_support_radius([
                ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([0.0, 0.0, -6.1], [0.0, 0.0, 1.0]),
                ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ]),
            Some(0.5)
        );
        assert_eq!(
            parallel_support_radius([
                ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
                ([0.0, 0.0, -8.0], [0.0, 0.0, 1.0]),
            ]),
            None
        );
        assert_eq!(
            parallel_support_radius([
                ([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
                ([0.0, 0.0, -7.0], [0.0, 0.0, 1.0]),
            ]),
            Some(0.5)
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
    fn zero_offset_support_plane_does_not_obscure_blind_cap() {
        assert_eq!(
            extrusion_extent_and_direction(
                [0.0; 3],
                [0.0, 1.0, 0.0],
                [
                    ([20.0, 0.0, 6.0], [0.0, 1.0, 0.0]),
                    ([0.0, 48.0, 0.0], [0.0, 1.0, 0.0]),
                ],
            ),
            Some((
                Extent::Blind {
                    length: Length(48.0),
                },
                [0.0, 1.0, 0.0],
            ))
        );
    }

    #[test]
    fn interior_axis_normal_planes_do_not_shorten_blind_extent() {
        assert_eq!(
            extrusion_extent_and_direction(
                [0.0; 3],
                [0.0, -1.0, 0.0],
                [
                    ([0.0, 38.0, 0.0], [0.0, 1.0, 0.0]),
                    ([3.0, 2.5, 7.0], [0.0, -1.0, 0.0]),
                    ([-4.0, 5.75, 1.0], [0.0, 1.0, 0.0]),
                ],
            ),
            Some((
                Extent::Blind {
                    length: Length(38.0),
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
    fn section_point_locus_uses_deterministic_segment_endpoint() {
        let segment = |external_id, point_ids| crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids,
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            offset: 0,
        };
        let segments = [segment(9, [4, 7]), segment(3, [2, 4])];

        assert_eq!(
            section_point_locus(12, &segments, 4),
            Some(SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#12:3".to_string()
            )))
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
        assert_eq!(
            section_entity_external_ids(&definition),
            BTreeSet::from([42])
        );
        let mut constrained = definition.clone();
        constrained.segments = Some(crate::feature::FeatureSegmentTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 0,
        });
        constrained.relations = Some(crate::feature::FeatureRelationTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            skamps: vec![crate::feature::FeatureSkamp {
                id: 5,
                kind: 99,
                flags: 0,
                status: 0,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 42,
                    sense: 4,
                }],
                offset: 30,
            }],
            triples: vec![crate::feature::FeatureRelationTriple {
                relation_id: Some(7),
                equation_id: None,
                skamp_id: Some(5),
                offset: 31,
            }],
            offset: 28,
        });
        let constraints =
            section_skamp_constraints(&constrained, &SketchId("creo:model:sketch#5".to_string()));
        assert!(matches!(
            &constraints[0].0.definition,
            SketchConstraintDefinition::Native { entities, .. }
                if entities == &[SketchEntityId(
                    "creo:featdefs:sketch_entity#5:42".to_string()
                )]
        ));
        assert_eq!(
            relation_incidence_entities(&constrained, 7),
            vec![SketchEntityId(
                "creo:featdefs:sketch_entity#5:42".to_string()
            )]
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
    fn complete_saved_circle_defines_full_section_geometry() {
        let entity =
            crate::feature::FeatureSavedEntity::Circle(crate::feature::FeatureSavedCircle {
                entity_id: 7,
                center: [Some(2.0), Some(-3.0), Some(0.0)],
                radius: Some(4.5),
                offset: 19,
            });

        assert_eq!(
            saved_section_entity_geometry(&entity),
            Some((
                7,
                SketchGeometry::Arc {
                    center: Point2::new(2.0, -3.0),
                    radius: Length(4.5),
                    start_angle: Angle(0.0),
                    end_angle: Angle(std::f64::consts::TAU),
                },
                19,
            ))
        );
        let (_, geometry, _) =
            saved_section_entity_geometry(&entity).expect("complete saved circle");
        assert!(is_full_circle_geometry(&geometry));
    }

    #[test]
    fn generated_saved_geometry_forms_closed_profiles() {
        let line = |external_id: u32, start: (f64, f64), end: (f64, f64)| {
            (
                external_id,
                SketchGeometry::Line {
                    start: Point2::new(start.0, start.1),
                    end: Point2::new(end.0, end.1),
                },
            )
        };
        let geometries = vec![
            line(12, (0.0, 1.0), (1.0, 1.0)),
            line(10, (0.0, 0.0), (1.0, 0.0)),
            line(13, (0.0, 0.0), (0.0, 1.0)),
            line(11, (1.0, 1.0), (1.0, 0.0)),
            line(20, (5.0, 5.0), (6.0, 5.0)),
            (
                30,
                SketchGeometry::Arc {
                    center: Point2::new(8.0, 8.0),
                    radius: Length(2.0),
                    start_angle: Angle(0.0),
                    end_angle: Angle(std::f64::consts::TAU),
                },
            ),
        ];

        let profiles = saved_profile_chains(917, &geometries);

        assert_eq!(profiles.len(), 2);
        assert_eq!(
            profiles[0][0].entity.0,
            "creo:featdefs:sketch_entity#917:30"
        );
        assert_eq!(profiles[1].len(), 4);
        assert_eq!(
            profiles[1][0].entity.0,
            "creo:featdefs:sketch_entity#917:10"
        );
        assert!(!profiles[1][0].reversed);
        assert!(profiles[1][1..].iter().all(|entity| entity.reversed));
        assert!(profiles
            .iter()
            .flatten()
            .all(|entity| !entity.entity.0.ends_with(":20")));
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
    fn unary_solver_incidences_define_line_orientation() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 12,
            offset: 40,
        };
        let arc = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [2, 3],
            center_id: Some(4),
            arc_orientation: Some(1),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 13,
            offset: 41,
        };
        let point = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Point,
            directions: [None; 3],
            point_ids: [4, 4],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 14,
            offset: 42,
        };
        let other_line = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [5, 6],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 15,
            offset: 43,
        };
        let other_arc = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Arc,
            directions: [None; 3],
            point_ids: [5, 6],
            center_id: Some(7),
            arc_orientation: Some(1),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 16,
            offset: 44,
        };
        let relations = crate::feature::FeatureRelationTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::FeatureRelation {
                relation_id: 8,
                used: 1,
                operands: vec![12, 4],
                operand_vectors: None,
                sign: 1,
                dimension_id: 0,
                relation_type: 99,
                body: Vec::new(),
                offset: 80,
            }],
            skamps: vec![
                crate::feature::FeatureSkamp {
                    id: 3,
                    kind: 1,
                    flags: 0,
                    status: 0,
                    items: vec![crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    }],
                    offset: 50,
                },
                crate::feature::FeatureSkamp {
                    id: 4,
                    kind: 2,
                    flags: 0,
                    status: 0,
                    items: vec![crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    }],
                    offset: 60,
                },
                crate::feature::FeatureSkamp {
                    id: 5,
                    kind: 7,
                    flags: 0,
                    status: 0,
                    items: vec![crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 4,
                    }],
                    offset: 70,
                },
                crate::feature::FeatureSkamp {
                    id: 6,
                    kind: 1,
                    flags: 0,
                    status: 0,
                    items: vec![crate::feature::FeatureSkampItem {
                        entity_id: 13,
                        sense: 0,
                    }],
                    offset: 71,
                },
                crate::feature::FeatureSkamp {
                    id: 7,
                    kind: 0,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 2,
                        },
                    ],
                    offset: 72,
                },
                crate::feature::FeatureSkamp {
                    id: 8,
                    kind: 4,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 3,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 2,
                        },
                    ],
                    offset: 73,
                },
                crate::feature::FeatureSkamp {
                    id: 9,
                    kind: 14,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 2,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 3,
                        },
                    ],
                    offset: 74,
                },
                crate::feature::FeatureSkamp {
                    id: 10,
                    kind: 14,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 4,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 4,
                        },
                    ],
                    offset: 75,
                },
                crate::feature::FeatureSkamp {
                    id: 11,
                    kind: 3,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 14,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 4,
                        },
                    ],
                    offset: 76,
                },
                crate::feature::FeatureSkamp {
                    id: 12,
                    kind: 9,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 14,
                            sense: 0,
                        },
                    ],
                    offset: 77,
                },
                crate::feature::FeatureSkamp {
                    id: 13,
                    kind: 5,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 15,
                            sense: 0,
                        },
                    ],
                    offset: 78,
                },
                crate::feature::FeatureSkamp {
                    id: 14,
                    kind: 7,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 15,
                            sense: 0,
                        },
                    ],
                    offset: 79,
                },
                crate::feature::FeatureSkamp {
                    id: 15,
                    kind: 8,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 12,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 15,
                            sense: 0,
                        },
                    ],
                    offset: 80,
                },
                crate::feature::FeatureSkamp {
                    id: 16,
                    kind: 6,
                    flags: 0,
                    status: 0,
                    items: vec![
                        crate::feature::FeatureSkampItem {
                            entity_id: 13,
                            sense: 0,
                        },
                        crate::feature::FeatureSkampItem {
                            entity_id: 16,
                            sense: 0,
                        },
                    ],
                    offset: 81,
                },
            ],
            triples: Vec::new(),
            offset: 45,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 917,
            owner_feature_id: Some(40),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::FeatureSegmentTable {
                declared_count: 5,
                entity_ref: None,
                rows: vec![segment, arc, point, other_line, other_arc],
                offset: 30,
            }),
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(crate::feature::FeatureDimensionTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::FeatureDimension {
                    dimension_type: 2,
                    value: Some(3.0),
                    value_unit: crate::feature::DimensionUnit::Millimeters,
                    direction_byte: 0,
                    auxiliary_value: None,
                    external_id: 42,
                    offset: 75,
                }],
                offset: 74,
            }),
            relations: Some(relations),
            saved_section: None,
            offset: 0,
        };
        let mut equal_radius_definition = definition.clone();
        let equal_radius_segments = &mut equal_radius_definition
            .segments
            .as_mut()
            .expect("segments")
            .rows;
        equal_radius_segments[1].radius_ref = Some(101);
        equal_radius_segments[4].radius_ref = Some(102);
        equal_radius_definition.variables = Some(crate::feature::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 2,
                    u: Some(3.0),
                    v: Some(0.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 4,
                    u: Some(0.0),
                    v: Some(0.0),
                },
            ],
            offset: 89,
        });
        assert_eq!(
            resolved_section_radii(&equal_radius_definition),
            BTreeMap::from([(101, 3.0), (102, 3.0)])
        );
        equal_radius_definition
            .variables
            .as_mut()
            .expect("variables")
            .rows
            .push(crate::feature::FeatureVariableRow {
                variable_type: 3,
                key: 102,
                value: Some(4.0),
                guess: None,
                uvar_id: None,
                dimension_driven: false,
                offset: 91,
            });
        assert!(resolved_section_radii(&equal_radius_definition).is_empty());
        let constraints = section_skamp_constraints(&definition, &SketchId("sketch".into()));

        assert!(matches!(
            constraints[0].0.definition,
            SketchConstraintDefinition::Horizontal { .. }
        ));
        assert!(matches!(
            constraints[1].0.definition,
            SketchConstraintDefinition::Vertical { .. }
        ));
        assert_eq!(
            constraints[2].0.definition,
            SketchConstraintDefinition::Native {
                native_kind: "creo:skamp:7".to_string(),
                entities: vec![SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )],
                parameter: None,
                operands: vec![SketchNativeOperand {
                    native_kind: "sense:4".to_string(),
                    object_index: 12,
                    native_ref: None,
                }],
            }
        );
        assert!(matches!(
            constraints[3].0.definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:skamp:1"
        ));
        assert!(matches!(
            constraints[4].0.definition,
            SketchConstraintDefinition::Native {
                ref native_kind,
                ..
            } if native_kind == "creo:skamp:0"
        ));
        assert_eq!(
            constraints[5].0.definition,
            SketchConstraintDefinition::TangentLoci {
                first: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                second: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
            }
        );
        assert_eq!(
            constraints[6].0.definition,
            SketchConstraintDefinition::Symmetric {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                second: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
                axis: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            }
        );
        assert_eq!(
            constraints[7].0.definition,
            SketchConstraintDefinition::Symmetric {
                first: SketchLocus::Center(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
                second: SketchLocus::Center(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:13".to_string()
                )),
                axis: SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
            }
        );
        assert_eq!(
            constraints[8].0.definition,
            SketchConstraintDefinition::CoincidentLoci {
                loci: vec![
                    SketchLocus::Entity(SketchEntityId(
                        "creo:featdefs:sketch_entity#917:14".to_string()
                    )),
                    SketchLocus::Center(SketchEntityId(
                        "creo:featdefs:sketch_entity#917:13".to_string()
                    )),
                ],
            }
        );
        assert_eq!(
            constraints[9].0.definition,
            SketchConstraintDefinition::CoincidentLoci {
                loci: vec![
                    SketchLocus::Entity(SketchEntityId(
                        "creo:featdefs:sketch_entity#917:12".to_string()
                    )),
                    SketchLocus::Entity(SketchEntityId(
                        "creo:featdefs:sketch_entity#917:14".to_string()
                    )),
                ],
            }
        );
        let first = SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string());
        let second = SketchEntityId("creo:featdefs:sketch_entity#917:15".to_string());
        assert_eq!(
            constraints[10].0.definition,
            SketchConstraintDefinition::Perpendicular {
                first: first.clone(),
                second: second.clone(),
            }
        );
        assert_eq!(
            constraints[11].0.definition,
            SketchConstraintDefinition::Parallel {
                first: first.clone(),
                second: second.clone(),
            }
        );
        assert_eq!(
            constraints[12].0.definition,
            SketchConstraintDefinition::Equal { first, second }
        );
        assert_eq!(
            constraints[13].0.definition,
            SketchConstraintDefinition::Equal {
                first: SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
                second: SketchEntityId("creo:featdefs:sketch_entity#917:16".to_string()),
            }
        );
        let mut distance_definition = definition.clone();
        let distance_segment = &mut distance_definition
            .segments
            .as_mut()
            .expect("segments")
            .rows[0];
        distance_segment.vertical_horizontal = Some(0);
        let distance_relation = &mut distance_definition
            .relations
            .as_mut()
            .expect("relations")
            .rows[0];
        distance_relation.relation_type = 0;
        distance_relation.sign = 1;
        distance_relation.dimension_id = 0;
        distance_relation.operand_vectors = Some([
            [Some(1), Some(2), None, Some(1)],
            [Some(0), Some(0), Some(0), Some(0)],
            [Some(15), Some(16), Some(15), Some(1)],
        ]);
        assert_eq!(
            section_dimension_constraints(&distance_definition, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::VerticalDistance {
                first: SketchLocus::Start(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                second: SketchLocus::End(SketchEntityId(
                    "creo:featdefs:sketch_entity#917:12".to_string()
                )),
                parameter: ParameterId("creo:featdefs:parameter#40:42".to_string()),
            }
        );
        let mut incidence_distance = distance_definition.clone();
        let incidence_relations = incidence_distance.relations.as_mut().expect("relations");
        incidence_relations.rows[0].operand_vectors = None;
        incidence_relations.skamps = vec![crate::feature::FeatureSkamp {
            id: 81,
            kind: 0,
            flags: 0,
            status: 0,
            items: vec![
                crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                },
            ],
            offset: 81,
        }];
        incidence_relations.triples = vec![crate::feature::FeatureRelationTriple {
            relation_id: Some(8),
            equation_id: None,
            skamp_id: Some(81),
            offset: 82,
        }];
        assert_eq!(
            section_dimension_constraints(&incidence_distance, &SketchId("sketch".into()))[0]
                .0
                .definition,
            SketchConstraintDefinition::Distance {
                entities: vec![
                    SketchEntityId("creo:featdefs:sketch_entity#917:12".to_string()),
                    SketchEntityId("creo:featdefs:sketch_entity#917:13".to_string()),
                ],
                parameter: ParameterId("creo:featdefs:parameter#40:42".to_string()),
            }
        );
        let relations = section_dimension_constraints(&definition, &SketchId("sketch".into()));
        assert_eq!(
            relations[0].0.definition,
            SketchConstraintDefinition::Native {
                native_kind: "creo:relation:99".to_string(),
                entities: Vec::new(),
                parameter: Some(ParameterId("creo:featdefs:parameter#40:42".to_string())),
                operands: vec![SketchNativeOperand {
                    native_kind: "relat_ptr".to_string(),
                    object_index: 8,
                    native_ref: Some("creo:featdefs:sketch#917".to_string()),
                }],
            }
        );
        let mut coincident_definition = definition.clone();
        coincident_definition
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids: [7, 8],
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id: 17,
                offset: 87,
            });
        coincident_definition.variables = Some(crate::feature::FeatureVariableTable {
            declared_count: 2,
            entity_ref: None,
            rows: Vec::new(),
            points: vec![
                crate::feature::FeatureSectionPoint {
                    point_id: 2,
                    u: Some(3.0),
                    v: Some(4.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 5,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 4,
                    u: None,
                    v: Some(9.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 6,
                    u: None,
                    v: None,
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 7,
                    u: Some(2.0),
                    v: Some(6.0),
                },
                crate::feature::FeatureSectionPoint {
                    point_id: 8,
                    u: None,
                    v: None,
                },
            ],
            offset: 0,
        });
        coincident_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps = vec![
            crate::feature::FeatureSkamp {
                id: 17,
                kind: 0,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 3,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 2,
                    },
                ],
                offset: 83,
            },
            crate::feature::FeatureSkamp {
                id: 18,
                kind: 3,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 14,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 3,
                    },
                ],
                offset: 84,
            },
            crate::feature::FeatureSkamp {
                id: 19,
                kind: 2,
                flags: 0,
                status: 0,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 15,
                    sense: 0,
                }],
                offset: 85,
            },
            crate::feature::FeatureSkamp {
                id: 20,
                kind: 9,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 15,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 14,
                        sense: 0,
                    },
                ],
                offset: 86,
            },
            crate::feature::FeatureSkamp {
                id: 21,
                kind: 1,
                flags: 0,
                status: 0,
                items: vec![crate::feature::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                }],
                offset: 88,
            },
            crate::feature::FeatureSkamp {
                id: 22,
                kind: 14,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 17,
                        sense: 2,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 17,
                        sense: 3,
                    },
                ],
                offset: 89,
            },
            crate::feature::FeatureSkamp {
                id: 23,
                kind: 5,
                flags: 0,
                status: 0,
                items: vec![
                    crate::feature::FeatureSkampItem {
                        entity_id: 12,
                        sense: 0,
                    },
                    crate::feature::FeatureSkampItem {
                        entity_id: 17,
                        sense: 0,
                    },
                ],
                offset: 90,
            },
        ];
        let related_line = unique_section_skamp_segment(&coincident_definition, 17).expect("line");
        assert_eq!(
            section_line_fixed_coordinate(&coincident_definition, related_line),
            Some(0)
        );
        let coincident_points = resolved_section_points(&coincident_definition);
        assert_eq!(coincident_points.get(&5), Some(&[3.0, 4.0]));
        assert_eq!(coincident_points.get(&4), Some(&[3.0, 9.0]));
        assert_eq!(coincident_points.get(&6), Some(&[3.0, 9.0]));
        assert_eq!(coincident_points.get(&8), Some(&[2.0, 2.0]));
        let mut saved_definition = definition;
        saved_definition.order_table = Some(crate::feature::FeatureOrderTable {
            declared_count: 1,
            entity_ref: None,
            rows: vec![crate::feature::FeatureOrderRow {
                external_id: 14,
                internal_id: 20,
                bitmask: 1,
                offset: 81,
            }],
            offset: 80,
        });
        saved_definition.saved_section = Some(crate::feature::FeatureSavedSection {
            entities: vec![crate::feature::FeatureSavedEntity::Line(
                crate::feature::FeatureSavedLine {
                    entity_id: 20,
                    references: Vec::new(),
                    attributes: Vec::new(),
                    endpoints: [
                        [Some(0.0), Some(0.0), Some(0.0)],
                        [Some(1.0), Some(0.0), Some(0.0)],
                    ],
                    offset: 82,
                },
            )],
            offset: 82,
        });
        assert_eq!(
            section_skamp_endpoint(
                &saved_definition,
                &crate::feature::FeatureSkampItem {
                    entity_id: 14,
                    sense: 3,
                },
            ),
            Some(SketchLocus::End(SketchEntityId(
                "creo:featdefs:sketch_entity#917:14".to_string()
            )))
        );
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

        let mut segment_graph = definition;
        segment_graph.trim_entities = None;
        segment_graph.segments = Some(crate::feature::FeatureSegmentTable {
            declared_count: 5,
            entity_ref: None,
            rows: [
                (10, [1, 2]),
                (11, [3, 2]),
                (12, [3, 4]),
                (13, [4, 1]),
                (20, [8, 9]),
            ]
            .into_iter()
            .map(|(external_id, point_ids)| crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Line,
                directions: [None; 3],
                point_ids,
                center_id: None,
                arc_orientation: None,
                vertical_horizontal: None,
                radius_ref: None,
                radius2_ref: None,
                external_id,
                offset: external_id as usize,
            })
            .collect(),
            offset: 4,
        });
        let segment_profile =
            resolved_profile_chains(&segment_graph, &BTreeSet::from([10, 11, 12, 13, 20]));
        assert_eq!(segment_profile.len(), 1);
        assert_eq!(segment_profile[0].len(), 4);
        assert!(!segment_profile[0][0].reversed);
        assert!(segment_profile[0][1].reversed);
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
    fn nonplanar_saved_spline_places_as_model_curve() {
        let transform = crate::placement::FeatureSectionTransform {
            definition_id: 917,
            feature_id: Some(40),
            origin: [10.0, 20.0, 30.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
            normal: [0.0, -1.0, 0.0],
            offset: 5,
        };
        let local = NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
            weights: None,
            periodic: false,
        };

        let placed = placed_section_nurbs(&transform, &local);

        assert_eq!(placed.control_points[0], Point3::new(11.0, 17.0, 32.0));
        assert_eq!(placed.control_points[1], Point3::new(14.0, 14.0, 35.0));
    }

    #[test]
    fn transferred_geometry_is_derived_from_ir_arenas() {
        let mut ir = CadIr::empty(Units::default());
        assert!(!has_transferred_geometry(&ir));

        ir.model.points.push(Point {
            id: PointId("point".to_string()),
            position: Point3::new(1.0, 2.0, 3.0),
        });
        assert!(has_transferred_geometry(&ir));
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

    #[test]
    fn planar_loop_containment_selects_one_outer_boundary() {
        let make_loop = |face_id: u32, first_curve: u32| crate::topology::Loop {
            face_id,
            half_edges: (0_u32..4)
                .map(|index| HalfEdgeId {
                    curve_id: first_curve + index,
                    side: 0,
                })
                .collect(),
        };
        let outer = make_loop(9, 1);
        let inner = make_loop(9, 5);
        let incidences = (1..=8)
            .map(|vertex| crate::topology::HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: vertex,
                    side: 0,
                },
                start_vertex_id: vertex,
                end_vertex_id: Some(if vertex % 4 == 0 {
                    vertex - 3
                } else {
                    vertex + 1
                }),
            })
            .collect::<Vec<_>>();
        let incidence = incidences
            .iter()
            .map(|binding| (binding.half_edge, binding))
            .collect::<BTreeMap<_, _>>();
        let points = BTreeMap::from([
            (1, [-2.0, -2.0, 0.0]),
            (2, [2.0, -2.0, 0.0]),
            (3, [2.0, 2.0, 0.0]),
            (4, [-2.0, 2.0, 0.0]),
            (5, [-1.0, -1.0, 0.0]),
            (6, [1.0, -1.0, 0.0]),
            (7, [1.0, 1.0, 0.0]),
            (8, [-1.0, 1.0, 0.0]),
        ]);
        let plane = PlaneEquation {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        };

        let ordered = ordered_planar_face_loops(vec![&inner, &outer], plane, &incidence, &points)
            .expect("unique outer loop");
        assert_eq!(ordered[0].half_edges[0].curve_id, 1);
        assert_eq!(ordered[1].half_edges[0].curve_id, 5);

        let disjoint_points = points
            .into_iter()
            .map(|(id, mut point)| {
                if id >= 5 {
                    point[0] += 10.0;
                }
                (id, point)
            })
            .collect::<BTreeMap<_, _>>();
        assert!(ordered_planar_face_loops(
            vec![&outer, &inner],
            plane,
            &incidence,
            &disjoint_points,
        )
        .is_none());
        assert_eq!(
            ordered_face_loops(vec![&outer], None, &incidence, &disjoint_points),
            Some(vec![&outer])
        );
        assert!(
            ordered_face_loops(vec![&outer, &inner], None, &incidence, &disjoint_points,).is_none()
        );
    }

    #[test]
    fn carrier_solver_accepts_unique_plane_plane_cylinder_vertex() {
        let cylinder = CarrierEquation::Cylinder(CylinderEquation {
            origin: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0,
        });
        let cap = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 3.0],
            normal: [0.0, 0.0, 1.0],
        });
        let tangent = CarrierEquation::Plane(PlaneEquation {
            origin: [2.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[cylinder, cap, tangent]),
            Some([2.0, 0.0, 3.0])
        );

        let secant = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(solve_carriers(&[cylinder, cap, secant]), None);

        assert!(matches!(
            carrier_intersection_curve(cap, cylinder),
            Some((CurveGeometry::Circle { center, radius, .. }, "plane_cylinder_circle"))
                if center.z == 3.0 && radius == 2.0
        ));
        let oblique = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 1.0],
        });
        assert!(matches!(
            carrier_intersection_curve(oblique, cylinder),
            Some((CurveGeometry::Ellipse { major_radius, minor_radius, .. }, "plane_cylinder_ellipse"))
                if (major_radius - 2.0 * 2.0_f64.sqrt()).abs() < 1e-12
                    && minor_radius == 2.0
        ));
        assert!(matches!(
            carrier_intersection_curve(tangent, cylinder),
            Some((CurveGeometry::Line { origin, direction }, "plane_cylinder_tangent_line"))
                if origin.x == 2.0 && direction.z == 1.0
        ));
        assert!(carrier_intersection_curve(secant, cylinder).is_none());

        let parallel_cylinder = |origin: [f64; 3], radius| {
            CarrierEquation::Cylinder(CylinderEquation {
                origin,
                axis: [0.0, 0.0, 1.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius,
            })
        };
        assert!(matches!(
            carrier_intersection_curve(
                parallel_cylinder([0.0, 0.0, 0.0], 2.0),
                parallel_cylinder([5.0, 0.0, 0.0], 3.0),
            ),
            Some((CurveGeometry::Line { origin, direction }, "parallel_cylinder_tangent_line"))
                if origin.x == 2.0 && direction.z == 1.0
        ));
        assert_eq!(
            solve_carriers(&[
                cap,
                parallel_cylinder([0.0, 0.0, 0.0], 2.0),
                parallel_cylinder([5.0, 0.0, 0.0], 3.0),
            ]),
            Some([2.0, 0.0, 3.0])
        );
        assert!(matches!(
            carrier_intersection_curve(
                parallel_cylinder([0.0, 0.0, 0.0], 5.0),
                parallel_cylinder([3.0, 0.0, 0.0], 2.0),
            ),
            Some((CurveGeometry::Line { origin, .. }, "parallel_cylinder_tangent_line"))
                if origin.x == 5.0
        ));
        assert!(carrier_intersection_curve(
            parallel_cylinder([0.0, 0.0, 0.0], 3.0),
            parallel_cylinder([4.0, 0.0, 0.0], 3.0),
        )
        .is_none());

        let sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0,
        });
        let equator = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        });
        assert!(matches!(
            carrier_intersection_curve(equator, sphere),
            Some((CurveGeometry::Circle { center, radius, .. }, "plane_sphere_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && radius == 2.0
        ));
        assert_eq!(solve_carriers(&[equator, secant, sphere]), None);
        assert_eq!(
            solve_carriers(&[equator, tangent, sphere]),
            Some([2.0, 0.0, 0.0])
        );
        let second_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [4.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 3.0,
        });
        let first_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 3.0,
        });
        assert!(matches!(
            carrier_intersection_curve(first_sphere, second_sphere),
            Some((CurveGeometry::Circle { center, radius, .. }, "sphere_intersection_circle"))
                if center.x == 2.0 && (radius - 5.0_f64.sqrt()).abs() < 1e-12
        ));
        let sphere_circle_tangent = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 5.0_f64.sqrt(), 0.0],
            normal: [0.0, 1.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[first_sphere, second_sphere, sphere_circle_tangent]),
            Some([2.0, 5.0_f64.sqrt(), 0.0])
        );
        assert!(matches!(
            carrier_intersection_curve(cylinder, sphere),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_sphere_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && radius == 2.0
        ));
        assert_eq!(
            solve_carriers(&[cylinder, sphere, tangent]),
            Some([2.0, 0.0, 0.0])
        );
        assert!(
            carrier_intersection_curve(parallel_cylinder([0.0, 0.0, 0.0], 1.0), sphere,).is_none()
        );

        let cone = CarrierEquation::Cone(ConeEquation {
            origin: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0,
            half_angle: std::f64::consts::FRAC_PI_4,
        });
        assert!(matches!(
            carrier_intersection_curve(cap, cone),
            Some((CurveGeometry::Circle { center, radius, .. }, "plane_cone_circle"))
                if center == Point3::new(0.0, 0.0, 3.0) && (radius - 5.0).abs() < 1e-12
        ));
        let inverse_sqrt_two = 1.0 / 2.0_f64.sqrt();
        let cone_tangent_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, -2.0],
            normal: [inverse_sqrt_two, 0.0, inverse_sqrt_two],
        });
        assert!(matches!(
            carrier_intersection_curve(cone_tangent_plane, cone),
            Some((CurveGeometry::Line { origin, direction }, "plane_cone_tangent_line"))
                if origin.x.abs() < 1e-12
                    && origin.y.abs() < 1e-12
                    && (origin.z + 2.0).abs() < 1e-12
                    && (direction.x + inverse_sqrt_two).abs() < 1e-12
                    && (direction.z - inverse_sqrt_two).abs() < 1e-12
        ));
        assert_eq!(solve_carriers(&[cone, cap, tangent]), None);
        let cone_tangent = CarrierEquation::Plane(PlaneEquation {
            origin: [5.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[cone, cap, cone_tangent]),
            Some([5.0, 0.0, 3.0])
        );
        let cone_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0_f64.sqrt(),
        });
        assert!(matches!(
            carrier_intersection_curve(cone_tangent_sphere, cone),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cone_sphere_tangent_circle"))
                if (center.z + 1.0).abs() < 1e-12 && (radius - 1.0).abs() < 1e-12
        ));
        let cone_sphere_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        let cone_sphere_vertex =
            solve_carriers(&[cone_tangent_sphere, cone, cone_sphere_plane]).expect("unique vertex");
        assert!((cone_sphere_vertex[0] - 1.0).abs() < 1e-12);
        assert!(cone_sphere_vertex[1].abs() < 1e-12);
        assert!((cone_sphere_vertex[2] + 1.0).abs() < 1e-12);
        assert!(carrier_intersection_curve(sphere, cone).is_none());

        let torus = CarrierEquation::Torus(TorusEquation {
            center: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            major_radius: 5.0,
            minor_radius: 2.0,
        });
        let torus_tangent = CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 2.0],
            normal: [0.0, 0.0, 1.0],
        });
        assert!(matches!(
            carrier_intersection_curve(torus_tangent, torus),
            Some((CurveGeometry::Circle { center, radius, .. }, "plane_torus_tangent_circle"))
                if center == Point3::new(0.0, 0.0, 2.0) && radius == 5.0
        ));
        assert!(carrier_intersection_curve(equator, torus).is_none());
        let outer_tangent_cylinder = parallel_cylinder([0.0, 0.0, 0.0], 7.0);
        assert!(matches!(
            carrier_intersection_curve(outer_tangent_cylinder, torus),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_cylinder_torus_tangent_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && radius == 7.0
        ));
        let outer_circle_tangent = CarrierEquation::Plane(PlaneEquation {
            origin: [7.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[outer_tangent_cylinder, torus, outer_circle_tangent]),
            Some([7.0, 0.0, 0.0])
        );
        assert!(
            carrier_intersection_curve(parallel_cylinder([0.0, 0.0, 0.0], 6.0), torus).is_none()
        );
        let torus_tangent_sphere = CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 3.0,
        });
        assert!(matches!(
            carrier_intersection_curve(torus_tangent_sphere, torus),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_sphere_torus_tangent_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && (radius - 3.0).abs() < 1e-12
        ));
        let torus_sphere_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [3.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[torus_tangent_sphere, torus, torus_sphere_plane]),
            Some([3.0, 0.0, 0.0])
        );
        assert!(carrier_intersection_curve(sphere, torus).is_none());
        let second_torus = CarrierEquation::Torus(TorusEquation {
            center: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            major_radius: 9.0,
            minor_radius: 2.0,
        });
        assert!(matches!(
            carrier_intersection_curve(torus, second_torus),
            Some((CurveGeometry::Circle { center, radius, .. }, "coaxial_tori_tangent_circle"))
                if center == Point3::new(0.0, 0.0, 0.0) && (radius - 7.0).abs() < 1e-12
        ));
        let tori_plane = CarrierEquation::Plane(PlaneEquation {
            origin: [7.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        });
        assert_eq!(
            solve_carriers(&[torus, second_torus, tori_plane]),
            Some([7.0, 0.0, 0.0])
        );
        assert!(point_on_carrier([5.0, 0.0, 2.0], torus));
        assert!(!point_on_carrier([5.0, 0.0, 0.0], torus));
        assert_eq!(
            solve_carriers(&[torus, torus_tangent, cone_tangent]),
            Some([5.0, 0.0, 2.0])
        );
    }
}

#[derive(Clone, Copy)]
struct PlaneEquation {
    origin: [f64; 3],
    normal: [f64; 3],
}

#[derive(Clone, Copy)]
struct CylinderEquation {
    origin: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    radius: f64,
}

#[derive(Clone, Copy)]
struct ConeEquation {
    origin: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    radius: f64,
    half_angle: f64,
}

#[derive(Clone, Copy)]
struct SphereEquation {
    center: [f64; 3],
    ref_direction: [f64; 3],
    radius: f64,
}

#[derive(Clone, Copy)]
struct TorusEquation {
    center: [f64; 3],
    axis: [f64; 3],
    ref_direction: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
}

#[derive(Clone, Copy)]
enum CarrierEquation {
    Plane(PlaneEquation),
    Cylinder(CylinderEquation),
    Cone(ConeEquation),
    Sphere(SphereEquation),
    Torus(TorusEquation),
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

fn intersect_two_planes_with_cylinder(
    first: PlaneEquation,
    second: PlaneEquation,
    cylinder: CylinderEquation,
) -> Vec<[f64; 3]> {
    let direction = cross(first.normal, second.normal);
    let denominator = dot(direction, direction);
    if denominator <= 1e-18 {
        return Vec::new();
    }
    let first_distance = dot(first.normal, first.origin);
    let second_distance = dot(second.normal, second.origin);
    let second_cross_direction = cross(second.normal, direction);
    let direction_cross_first = cross(direction, first.normal);
    let line_origin: [f64; 3] = std::array::from_fn(|index| {
        (first_distance * second_cross_direction[index]
            + second_distance * direction_cross_first[index])
            / denominator
    });
    let Some(axis) = normalized(cylinder.axis) else {
        return Vec::new();
    };
    let relative = std::array::from_fn(|index| line_origin[index] - cylinder.origin[index]);
    let relative_axial = dot(relative, axis);
    let direction_axial = dot(direction, axis);
    let radial = std::array::from_fn(|index| relative[index] - relative_axial * axis[index]);
    let radial_direction =
        std::array::from_fn(|index| direction[index] - direction_axial * axis[index]);
    let quadratic = dot(radial_direction, radial_direction);
    if quadratic <= 1e-18 {
        return Vec::new();
    }
    let linear = 2.0 * dot(radial, radial_direction);
    let constant = dot(radial, radial) - cylinder.radius * cylinder.radius;
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    let scale = linear
        .abs()
        .max((4.0 * quadratic * constant).abs().sqrt())
        .max(1.0);
    if discriminant < -1e-12 * scale * scale {
        return Vec::new();
    }
    let root = discriminant.max(0.0).sqrt();
    let mut parameters = vec![(-linear - root) / (2.0 * quadratic)];
    if root > 1e-12 * scale {
        parameters.push((-linear + root) / (2.0 * quadratic));
    }
    parameters
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|index| line_origin[index] + parameter * direction[index])
        })
        .filter(|point: &[f64; 3]| point.iter().all(|value| value.is_finite()))
        .collect()
}

fn intersect_two_planes_with_sphere(
    first: PlaneEquation,
    second: PlaneEquation,
    sphere: SphereEquation,
) -> Vec<[f64; 3]> {
    let direction = cross(first.normal, second.normal);
    let denominator = dot(direction, direction);
    if denominator <= 1e-18 {
        return Vec::new();
    }
    let first_distance = dot(first.normal, first.origin);
    let second_distance = dot(second.normal, second.origin);
    let second_cross_direction = cross(second.normal, direction);
    let direction_cross_first = cross(direction, first.normal);
    let line_origin: [f64; 3] = std::array::from_fn(|index| {
        (first_distance * second_cross_direction[index]
            + second_distance * direction_cross_first[index])
            / denominator
    });
    let relative: [f64; 3] = std::array::from_fn(|index| line_origin[index] - sphere.center[index]);
    let quadratic = denominator;
    let linear = 2.0 * dot(relative, direction);
    let constant = dot(relative, relative) - sphere.radius * sphere.radius;
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    let scale = linear
        .abs()
        .max((4.0 * quadratic * constant).abs().sqrt())
        .max(1.0);
    if discriminant < -1e-12 * scale * scale {
        return Vec::new();
    }
    let root = discriminant.max(0.0).sqrt();
    let mut parameters = vec![(-linear - root) / (2.0 * quadratic)];
    if root > 1e-12 * scale {
        parameters.push((-linear + root) / (2.0 * quadratic));
    }
    parameters
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|index| line_origin[index] + parameter * direction[index])
        })
        .filter(|point: &[f64; 3]| point.iter().all(|value| value.is_finite()))
        .collect()
}

fn intersect_two_planes_with_cone(
    first: PlaneEquation,
    second: PlaneEquation,
    cone: ConeEquation,
) -> Vec<[f64; 3]> {
    let direction = cross(first.normal, second.normal);
    let denominator = dot(direction, direction);
    let Some(axis) = normalized(cone.axis) else {
        return Vec::new();
    };
    if denominator <= 1e-18 || !(0.0..std::f64::consts::FRAC_PI_2).contains(&cone.half_angle) {
        return Vec::new();
    }
    let first_distance = dot(first.normal, first.origin);
    let second_distance = dot(second.normal, second.origin);
    let second_cross_direction = cross(second.normal, direction);
    let direction_cross_first = cross(direction, first.normal);
    let line_origin: [f64; 3] = std::array::from_fn(|index| {
        (first_distance * second_cross_direction[index]
            + second_distance * direction_cross_first[index])
            / denominator
    });
    let relative = std::array::from_fn(|index| line_origin[index] - cone.origin[index]);
    let axial = dot(relative, axis);
    let axial_direction = dot(direction, axis);
    let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
    let radial_direction =
        std::array::from_fn(|index| direction[index] - axial_direction * axis[index]);
    let slope = cone.half_angle.tan();
    let local_radius = cone.radius + axial * slope;
    let radius_rate = axial_direction * slope;
    let quadratic = dot(radial_direction, radial_direction) - radius_rate * radius_rate;
    let linear = 2.0 * (dot(radial, radial_direction) - local_radius * radius_rate);
    let constant = dot(radial, radial) - local_radius * local_radius;
    let scale = quadratic
        .abs()
        .max(linear.abs())
        .max(constant.abs())
        .max(1.0);
    let mut parameters = Vec::<f64>::new();
    if quadratic.abs() <= 1e-14 * scale {
        if linear.abs() <= 1e-14 * scale {
            return Vec::new();
        }
        parameters.push(-constant / linear);
    } else {
        let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
        if discriminant < -1e-12 * scale * scale {
            return Vec::new();
        }
        let root = discriminant.max(0.0).sqrt();
        parameters.push((-linear - root) / (2.0 * quadratic));
        if root > 1e-12 * scale {
            parameters.push((-linear + root) / (2.0 * quadratic));
        }
    }
    parameters
        .into_iter()
        .map(|parameter| {
            std::array::from_fn(|index| line_origin[index] + parameter * direction[index])
        })
        .filter(|point: &[f64; 3]| point.iter().all(|value| value.is_finite()))
        .collect()
}

fn intersect_plane_with_circle(
    plane: PlaneEquation,
    center: [f64; 3],
    circle_axis: [f64; 3],
    radius: f64,
) -> Vec<[f64; 3]> {
    let (Some(plane_normal), Some(circle_normal)) =
        (normalized(plane.normal), normalized(circle_axis))
    else {
        return Vec::new();
    };
    let line_direction = cross(plane_normal, circle_normal);
    let denominator = dot(line_direction, line_direction);
    if denominator <= 1e-18 || radius <= 0.0 {
        return Vec::new();
    }
    let plane_distance = dot(plane_normal, plane.origin);
    let circle_distance = dot(circle_normal, center);
    let weighted = std::array::from_fn(|index| {
        plane_distance * circle_normal[index] - circle_distance * plane_normal[index]
    });
    let line_origin = cross(weighted, line_direction).map(|value| value / denominator);
    let relative: [f64; 3] = std::array::from_fn(|index| line_origin[index] - center[index]);
    let parameter_at_nearest = -dot(relative, line_direction) / denominator;
    let nearest: [f64; 3] = std::array::from_fn(|index| {
        line_origin[index] + parameter_at_nearest * line_direction[index]
    });
    let center_to_nearest: [f64; 3] = std::array::from_fn(|index| nearest[index] - center[index]);
    let remaining = radius.mul_add(radius, -dot(center_to_nearest, center_to_nearest));
    let scale = radius.max(1.0);
    if remaining < -1e-12 * scale * scale {
        return Vec::new();
    }
    let parameter_delta = remaining.max(0.0).sqrt() / denominator.sqrt();
    let mut points = vec![std::array::from_fn(|index| {
        nearest[index] - parameter_delta * line_direction[index]
    })];
    if parameter_delta > 1e-12 * scale {
        points.push(std::array::from_fn(|index| {
            nearest[index] + parameter_delta * line_direction[index]
        }));
    }
    points
}

fn circle_parameters(geometry: &CurveGeometry) -> Option<([f64; 3], [f64; 3], f64)> {
    let CurveGeometry::Circle {
        center,
        axis,
        radius,
        ..
    } = geometry
    else {
        return None;
    };
    Some((
        [center.x, center.y, center.z],
        [axis.x, axis.y, axis.z],
        *radius,
    ))
}

fn point_on_carrier(point: [f64; 3], carrier: CarrierEquation) -> bool {
    match carrier {
        CarrierEquation::Plane(plane) => {
            let residual = dot(plane.normal, point) - dot(plane.normal, plane.origin);
            residual.abs() <= 1e-7
        }
        CarrierEquation::Cylinder(cylinder) => {
            let Some(axis) = normalized(cylinder.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - cylinder.origin[index]);
            let axial = dot(relative, axis);
            let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
            (dot(radial, radial).sqrt() - cylinder.radius).abs() <= 1e-7 * cylinder.radius.max(1.0)
        }
        CarrierEquation::Cone(cone) => {
            let Some(axis) = normalized(cone.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - cone.origin[index]);
            let axial = dot(relative, axis);
            let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let radius = cone.radius + axial * cone.half_angle.tan();
            (dot(radial, radial).sqrt() - radius.abs()).abs() <= 1e-7 * radius.abs().max(1.0)
        }
        CarrierEquation::Sphere(sphere) => {
            let relative = std::array::from_fn(|index| point[index] - sphere.center[index]);
            (dot(relative, relative).sqrt() - sphere.radius).abs() <= 1e-7 * sphere.radius.max(1.0)
        }
        CarrierEquation::Torus(torus) => {
            let Some(axis) = normalized(torus.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - torus.center[index]);
            let axial = dot(relative, axis);
            let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let tube_distance = (dot(radial, radial).sqrt() - torus.major_radius).hypot(axial);
            (tube_distance - torus.minor_radius).abs()
                <= 1e-7 * torus.minor_radius.max(torus.major_radius).max(1.0)
        }
    }
}

fn solve_carriers(carriers: &[CarrierEquation]) -> Option<[f64; 3]> {
    let mut candidates = Vec::new();
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            for third in second + 1..carriers.len() {
                let triple = [carriers[first], carriers[second], carriers[third]];
                let mut planes = Vec::new();
                let mut cylinders = Vec::new();
                let mut cones = Vec::new();
                let mut spheres = Vec::new();
                let mut tori = Vec::new();
                for carrier in triple {
                    match carrier {
                        CarrierEquation::Plane(plane) => planes.push(plane),
                        CarrierEquation::Cylinder(cylinder) => cylinders.push(cylinder),
                        CarrierEquation::Cone(cone) => cones.push(cone),
                        CarrierEquation::Sphere(sphere) => spheres.push(sphere),
                        CarrierEquation::Torus(torus) => tori.push(torus),
                    }
                }
                if planes.len() == 3 {
                    if let Some(point) = solve_planes(&planes) {
                        candidates.push(point);
                    }
                } else if let ([first, second], [cylinder]) =
                    (planes.as_slice(), cylinders.as_slice())
                {
                    candidates.extend(intersect_two_planes_with_cylinder(
                        *first, *second, *cylinder,
                    ));
                } else if let ([plane], [first, second]) = (planes.as_slice(), cylinders.as_slice())
                {
                    let Some((CurveGeometry::Line { origin, direction }, _)) =
                        carrier_intersection_curve(
                            CarrierEquation::Cylinder(*first),
                            CarrierEquation::Cylinder(*second),
                        )
                    else {
                        continue;
                    };
                    let line_origin = [origin.x, origin.y, origin.z];
                    let line_direction = [direction.x, direction.y, direction.z];
                    let denominator = dot(plane.normal, line_direction);
                    if denominator.abs() <= 1e-12 {
                        continue;
                    }
                    let parameter = (dot(plane.normal, plane.origin)
                        - dot(plane.normal, line_origin))
                        / denominator;
                    candidates.push(std::array::from_fn(|index| {
                        line_origin[index] + parameter * line_direction[index]
                    }));
                } else if let ([first, second], [], [sphere]) =
                    (planes.as_slice(), cylinders.as_slice(), spheres.as_slice())
                {
                    if cones.is_empty() && tori.is_empty() {
                        candidates
                            .extend(intersect_two_planes_with_sphere(*first, *second, *sphere));
                    }
                } else if let ([first, second], [cone]) = (planes.as_slice(), cones.as_slice()) {
                    if cylinders.is_empty() && spheres.is_empty() && tori.is_empty() {
                        candidates.extend(intersect_two_planes_with_cone(*first, *second, *cone));
                    }
                } else if let ([plane], [cylinder], [sphere]) =
                    (planes.as_slice(), cylinders.as_slice(), spheres.as_slice())
                {
                    if cones.is_empty() && tori.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Cylinder(*cylinder),
                            CarrierEquation::Sphere(*sphere),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                } else if let ([plane], [first, second]) = (planes.as_slice(), spheres.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && tori.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Sphere(*first),
                            CarrierEquation::Sphere(*second),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                } else if let ([first, second], [torus]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        for (section_plane, cutting_plane) in [(*first, *second), (*second, *first)]
                        {
                            if let Some((geometry, _)) = carrier_intersection_curve(
                                CarrierEquation::Plane(section_plane),
                                CarrierEquation::Torus(*torus),
                            ) {
                                if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                    candidates.extend(intersect_plane_with_circle(
                                        cutting_plane,
                                        center,
                                        axis,
                                        radius,
                                    ));
                                }
                            }
                        }
                    }
                } else if let ([plane], [cylinder], [torus]) =
                    (planes.as_slice(), cylinders.as_slice(), tori.as_slice())
                {
                    if cones.is_empty() && spheres.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Cylinder(*cylinder),
                            CarrierEquation::Torus(*torus),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                } else if let ([plane], [cone], [sphere]) =
                    (planes.as_slice(), cones.as_slice(), spheres.as_slice())
                {
                    if cylinders.is_empty() && tori.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Cone(*cone),
                            CarrierEquation::Sphere(*sphere),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                } else if let ([plane], [sphere], [torus]) =
                    (planes.as_slice(), spheres.as_slice(), tori.as_slice())
                {
                    if cylinders.is_empty() && cones.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Sphere(*sphere),
                            CarrierEquation::Torus(*torus),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                } else if let ([plane], [first, second]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        if let Some((geometry, _)) = carrier_intersection_curve(
                            CarrierEquation::Torus(*first),
                            CarrierEquation::Torus(*second),
                        ) {
                            if let Some((center, axis, radius)) = circle_parameters(&geometry) {
                                candidates.extend(intersect_plane_with_circle(
                                    *plane, center, axis, radius,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    candidates.retain(|point| {
        carriers
            .iter()
            .all(|carrier| point_on_carrier(*point, *carrier))
    });
    let mut unique = Vec::<[f64; 3]>::new();
    for candidate in candidates {
        if !unique.iter().any(|known| {
            known
                .iter()
                .zip(candidate)
                .all(|(left, right)| (left - right).abs() <= 1e-7)
        }) {
            unique.push(candidate);
        }
    }
    let [point] = unique.as_slice() else {
        return None;
    };
    Some(*point)
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

fn placed_carriers(scan: &ContainerScan, ir: &CadIr) -> BTreeMap<u32, CarrierEquation> {
    let mut carriers = placed_planes(scan)
        .into_iter()
        .map(|(id, plane)| (id, CarrierEquation::Plane(plane)))
        .collect::<BTreeMap<_, _>>();
    for row in &scan.surface_rows {
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        let Some(surface) = ir.model.surfaces.iter().find(|surface| surface.id == id) else {
            continue;
        };
        if let SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } = &surface.geometry
        {
            carriers.insert(
                row.id,
                CarrierEquation::Cylinder(CylinderEquation {
                    origin: [origin.x, origin.y, origin.z],
                    axis: [axis.x, axis.y, axis.z],
                    ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    radius: *radius,
                }),
            );
        } else if let SurfaceGeometry::Sphere {
            center,
            axis: _,
            ref_direction,
            radius,
        } = &surface.geometry
        {
            carriers.insert(
                row.id,
                CarrierEquation::Sphere(SphereEquation {
                    center: [center.x, center.y, center.z],
                    ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    radius: *radius,
                }),
            );
        } else if let SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } = &surface.geometry
        {
            if (*ratio - 1.0).abs() <= 1e-12 {
                carriers.insert(
                    row.id,
                    CarrierEquation::Cone(ConeEquation {
                        origin: [origin.x, origin.y, origin.z],
                        axis: [axis.x, axis.y, axis.z],
                        ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                        radius: *radius,
                        half_angle: *half_angle,
                    }),
                );
            }
        } else if let SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } = &surface.geometry
        {
            carriers.insert(
                row.id,
                CarrierEquation::Torus(TorusEquation {
                    center: [center.x, center.y, center.z],
                    axis: [axis.x, axis.y, axis.z],
                    ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    major_radius: *major_radius,
                    minor_radius: *minor_radius,
                }),
            );
        }
    }
    carriers
}

fn geometry_section_record(scan: &ContainerScan, offset: usize) -> Option<UnknownId> {
    scan.sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY)
        .find(|section| {
            offset >= section.offset && offset < section.offset.saturating_add(section.length)
        })
        .map(|section| UnknownId(format!("creo:{}:section#{}", section.name, section.offset)))
}

fn projected_loop_polygon(
    lp: &crate::topology::Loop,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<[f64; 2]>> {
    let dropped_axis = (0..3).max_by(|left, right| {
        plane.normal[*left]
            .abs()
            .total_cmp(&plane.normal[*right].abs())
    })?;
    let polygon = lp
        .half_edges
        .iter()
        .map(|half_edge| {
            let vertex = incidence.get(half_edge)?.start_vertex_id;
            let point = solved_vertices.get(&vertex)?;
            Some(match dropped_axis {
                0 => [point[1], point[2]],
                1 => [point[0], point[2]],
                _ => [point[0], point[1]],
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let area_twice = (0..polygon.len())
        .map(|index| {
            let first = polygon[index];
            let second = polygon[(index + 1) % polygon.len()];
            first[0].mul_add(second[1], -(first[1] * second[0]))
        })
        .sum::<f64>();
    let scale = polygon
        .iter()
        .flat_map(|point| point.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (polygon.len() >= 3 && area_twice.abs() > 1e-12 * scale * scale).then_some(polygon)
}

fn polygon_strictly_contains(polygon: &[[f64; 2]], point: [f64; 2]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let edge = [second[0] - first[0], second[1] - first[1]];
        let relative = [point[0] - first[0], point[1] - first[1]];
        let cross = edge[0].mul_add(relative[1], -(edge[1] * relative[0]));
        let scale = edge[0].abs().max(edge[1].abs()).max(1.0);
        if cross.abs() <= 1e-9 * scale
            && point[0] >= first[0].min(second[0]) - 1e-9 * scale
            && point[0] <= first[0].max(second[0]) + 1e-9 * scale
            && point[1] >= first[1].min(second[1]) - 1e-9 * scale
            && point[1] <= first[1].max(second[1]) + 1e-9 * scale
        {
            return false;
        }
        if (first[1] > point[1]) != (second[1] > point[1]) {
            let intersection = edge[0].mul_add((point[1] - first[1]) / edge[1], first[0]);
            if point[0] < intersection {
                inside = !inside;
            }
        }
    }
    inside
}

fn ordered_planar_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    if loops.len() == 1 {
        return Some(loops);
    }
    let polygons = loops
        .iter()
        .map(|lp| projected_loop_polygon(lp, plane, incidence, solved_vertices))
        .collect::<Option<Vec<_>>>()?;
    let outer = polygons
        .iter()
        .enumerate()
        .filter(|(candidate, polygon)| {
            polygons.iter().enumerate().all(|(index, inner)| {
                index == *candidate
                    || inner
                        .iter()
                        .all(|point| polygon_strictly_contains(polygon, *point))
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [outer] = outer.as_slice() else {
        return None;
    };
    let mut ordered = Vec::with_capacity(loops.len());
    ordered.push(loops[*outer]);
    ordered.extend(
        loops
            .into_iter()
            .enumerate()
            .filter_map(|(index, lp)| (index != *outer).then_some(lp)),
    );
    Some(ordered)
}

fn ordered_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    plane: Option<PlaneEquation>,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    if let Some(plane) = plane {
        ordered_planar_face_loops(loops, plane, incidence, solved_vertices)
    } else {
        let [single] = loops.as_slice() else {
            return None;
        };
        Some(vec![*single])
    }
}

fn transfer_plane_brep(scan: &ContainerScan, ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let planes = placed_planes(scan);
    let carriers = placed_carriers(scan, ir);
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
            let incident_carriers = vertex
                .half_edges
                .iter()
                .filter_map(|half_edge| half_edges.get(half_edge))
                .filter_map(|half_edge| carriers.get(&half_edge.face_id))
                .copied()
                .collect::<Vec<_>>();
            solve_carriers(&incident_carriers).map(|point| (vertex.id, point))
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
    let mut loops_by_face = BTreeMap::<u32, Vec<&crate::topology::Loop>>::new();
    for lp in &scan.loops {
        loops_by_face.entry(lp.face_id).or_default().push(lp);
    }
    let eligible_faces = loops_by_face
        .into_iter()
        .filter_map(|(face_id, loops)| {
            scan.surface_rows
                .iter()
                .any(|row| row.id == face_id)
                .then_some(())?;
            loops
                .iter()
                .all(|lp| {
                    lp.half_edges
                        .iter()
                        .all(|half_edge| edge_vertices.contains_key(&half_edge.curve_id))
                })
                .then_some(())?;
            let ordered = ordered_face_loops(
                loops,
                planes.get(&face_id).copied(),
                &incidence,
                &solved_vertices,
            )?;
            Some((face_id, ordered))
        })
        .collect::<BTreeMap<_, _>>();
    if eligible_faces.is_empty() {
        return;
    }
    let eligible_loops = eligible_faces
        .values()
        .flatten()
        .copied()
        .collect::<Vec<_>>();

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
        let curve = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
        if !ir.model.curves.iter().any(|item| item.id == curve) {
            let offset = row_offsets.get(curve_id).copied().unwrap_or(0);
            annotate(
                annotations,
                &curve,
                "VisibGeom",
                offset as u64,
                "opaque_native_curve_carrier",
                Exactness::Unknown,
            );
            ir.model.curves.push(Curve {
                id: curve,
                geometry: CurveGeometry::Unknown {
                    record: geometry_section_record(scan, offset),
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

    let mut face_adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    for face_id in eligible_faces.keys() {
        face_adjacency.entry(*face_id).or_default();
    }
    for curve_id in &emitted_curves {
        let faces = emitted_half_edges
            .iter()
            .filter(|half_edge| half_edge.curve_id == *curve_id)
            .filter_map(|half_edge| half_edges.get(half_edge))
            .map(|half_edge| half_edge.face_id)
            .collect::<Vec<_>>();
        if let [first, second] = faces.as_slice() {
            if eligible_faces.contains_key(first) && eligible_faces.contains_key(second) {
                face_adjacency.entry(*first).or_default().insert(*second);
                face_adjacency.entry(*second).or_default().insert(*first);
            }
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
            (body_id.to_string(), "native_component_body"),
            (region_id.to_string(), "native_component_region"),
            (shell_id.to_string(), "native_component_shell"),
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
            let native_loops = &eligible_faces[face_id];
            let face = FaceId(format!("creo:visibgeom:face#{face_id}"));
            let loop_ids = (0..native_loops.len())
                .map(|index| {
                    if index == 0 {
                        LoopId(format!("creo:visibgeom:loop#{face_id}"))
                    } else {
                        LoopId(format!("creo:visibgeom:loop#{face_id}:{index}"))
                    }
                })
                .collect::<Vec<_>>();
            let face_offset = scan
                .surface_rows
                .iter()
                .find(|row| row.id == *face_id)
                .map_or(0, |row| row.offset);
            let surface = SurfaceId(format!("creo:visibgeom:surface#{face_id}"));
            if !ir.model.surfaces.iter().any(|item| item.id == surface) {
                annotate(
                    annotations,
                    &surface,
                    "VisibGeom",
                    face_offset as u64,
                    "opaque_native_surface_carrier",
                    Exactness::Unknown,
                );
                ir.model.surfaces.push(Surface {
                    id: surface.clone(),
                    geometry: SurfaceGeometry::Unknown {
                        record: geometry_section_record(scan, face_offset),
                    },
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!("VisibGeom:{face_id}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            let face_sense = scan
                .surface_rows
                .iter()
                .find(|row| row.id == *face_id)
                .map_or(Sense::Forward, |row| {
                    if row.reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    }
                });
            annotate(
                annotations,
                &face,
                "VisibGeom",
                face_offset as u64,
                "native_face",
                Exactness::Derived,
            );
            for loop_id in &loop_ids {
                annotate(
                    annotations,
                    loop_id,
                    "VisibGeom",
                    face_offset as u64,
                    "native_face_loop",
                    Exactness::Derived,
                );
            }
            ir.model.faces.push(Face {
                id: face.clone(),
                shell: shell_id.clone(),
                surface,
                sense: face_sense,
                loops: loop_ids.clone(),
                name: None,
                color: None,
                tolerance: None,
            });
            for (native_loop, loop_id) in native_loops.iter().zip(loop_ids) {
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
                    face: face.clone(),
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
                        previous: coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()]
                            .clone(),
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
        if placed_caps
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
        let axis_sign = -f64::from(pair.parameter_sign);
        let (origin, axis, ref_direction) = fc05_model_frame(
            axis_index,
            axis_origin,
            pair.center_row_frame,
            pair.reference_direction_row_frame,
            axis_sign,
        );
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
            let (center, _, _) = fc05_model_frame(
                axis_index,
                cap_offset,
                pair.center_row_frame,
                pair.reference_direction_row_frame,
                axis_sign,
            );
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

fn fc05_model_frame(
    axis_index: usize,
    axis_ordinate: f64,
    center_row_frame: [f64; 2],
    reference_row_frame: [f64; 2],
    axis_sign: f64,
) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let [first, second] = center_row_frame;
    let [reference_x, reference_z] = reference_row_frame;
    match axis_index {
        0 => (
            [axis_ordinate, second, first],
            [axis_sign, 0.0, 0.0],
            [0.0, reference_z, reference_x],
        ),
        1 => (
            [first, axis_ordinate, second],
            [0.0, axis_sign, 0.0],
            [reference_x, 0.0, reference_z],
        ),
        2 => (
            [second, first, axis_ordinate],
            [0.0, 0.0, axis_sign],
            [reference_z, reference_x, 0.0],
        ),
        _ => unreachable!("model-space axis index is bounded by XYZ"),
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
        let ([cap], [cylinder_id], Some(reference), Some(parameter_sign), Some(_)) = (
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
        let [first, second] = circle.center_row_frame;
        let axis_sign = -f64::from(parameter_sign);
        let (center, axis, ref_direction) = fc05_model_frame(
            axis_index,
            cap.origin[axis_index],
            [first, second],
            reference,
            axis_sign,
        );
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
                origin: Point3::new(center[0], center[1], center[2]),
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

fn carrier_intersection_curve(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Option<(CurveGeometry, &'static str)> {
    match (first, second) {
        (CarrierEquation::Plane(first), CarrierEquation::Plane(second)) => {
            let direction = cross(first.normal, second.normal);
            let denominator = dot(direction, direction);
            if denominator <= 1e-18 {
                return None;
            }
            let first_distance = dot(first.normal, first.origin);
            let second_distance = dot(second.normal, second.origin);
            let weighted = [0, 1, 2].map(|axis| {
                first_distance * second.normal[axis] - second_distance * first.normal[axis]
            });
            let point_numerator = cross(weighted, direction);
            let origin = point_numerator.map(|value| value / denominator);
            let direction = normalized(direction)?;
            Some((
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(direction[0], direction[1], direction[2]),
                },
                "plane_intersection_line",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Cylinder(cylinder))
        | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(cylinder.axis)?;
            let cosine = dot(normal, axis);
            if cosine.abs() <= 1e-10 {
                let signed_distance = dot(
                    normal,
                    std::array::from_fn(|index| cylinder.origin[index] - plane.origin[index]),
                );
                let scale = cylinder.radius.max(1.0);
                if (signed_distance.abs() - cylinder.radius).abs() > 1e-9 * scale {
                    return None;
                }
                let origin: [f64; 3] = std::array::from_fn(|index| {
                    cylinder.origin[index] - signed_distance * normal[index]
                });
                return Some((
                    CurveGeometry::Line {
                        origin: Point3::new(origin[0], origin[1], origin[2]),
                        direction: Vector3::new(axis[0], axis[1], axis[2]),
                    },
                    "plane_cylinder_tangent_line",
                ));
            }
            let axis_parameter = dot(
                normal,
                std::array::from_fn(|index| plane.origin[index] - cylinder.origin[index]),
            ) / cosine;
            let center: [f64; 3] =
                std::array::from_fn(|index| cylinder.origin[index] + axis_parameter * axis[index]);
            if (cosine.abs() - 1.0).abs() <= 1e-10 {
                let reference = normalized(cylinder.ref_direction)?;
                return Some((
                    CurveGeometry::Circle {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(normal[0], normal[1], normal[2]),
                        ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        radius: cylinder.radius,
                    },
                    "plane_cylinder_circle",
                ));
            }
            let projected_axis = normalized(std::array::from_fn(|index| {
                axis[index] - cosine * normal[index]
            }))?;
            Some((
                CurveGeometry::Ellipse {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    major_direction: Vector3::new(
                        projected_axis[0],
                        projected_axis[1],
                        projected_axis[2],
                    ),
                    major_radius: cylinder.radius / cosine.abs(),
                    minor_radius: cylinder.radius,
                },
                "plane_cylinder_ellipse",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let signed_distance = dot(
                normal,
                std::array::from_fn(|index| sphere.center[index] - plane.origin[index]),
            );
            let radius_squared = sphere
                .radius
                .mul_add(sphere.radius, -(signed_distance * signed_distance));
            let scale = sphere.radius.max(1.0);
            if radius_squared <= 1e-18 * scale * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - signed_distance * normal[index]);
            let reference = normalized(std::array::from_fn(|index| {
                sphere.ref_direction[index] - dot(sphere.ref_direction, normal) * normal[index]
            }))
            .unwrap_or_else(|| {
                let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                    normal[0], normal[1], normal[2],
                ));
                [reference.x, reference.y, reference.z]
            });
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: radius_squared.sqrt(),
                },
                "plane_sphere_circle",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Cone(cone))
        | (CarrierEquation::Cone(cone), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(cone.axis)?;
            let alignment = dot(normal, axis);
            let slope = cone.half_angle.tan();
            if slope.abs() > 1e-12 {
                let apex: [f64; 3] = std::array::from_fn(|index| {
                    cone.origin[index] - (cone.radius / slope) * axis[index]
                });
                let plane_distance = dot(
                    normal,
                    std::array::from_fn(|index| apex[index] - plane.origin[index]),
                );
                let scale = cone.radius.max(1.0);
                if plane_distance.abs() <= 1e-9 * scale
                    && (alignment.abs() - cone.half_angle.sin()).abs() <= 1e-10
                {
                    let direction = normalized(std::array::from_fn(|index| {
                        axis[index] - alignment * normal[index]
                    }))?;
                    return Some((
                        CurveGeometry::Line {
                            origin: Point3::new(apex[0], apex[1], apex[2]),
                            direction: Vector3::new(direction[0], direction[1], direction[2]),
                        },
                        "plane_cone_tangent_line",
                    ));
                }
            }
            if (alignment.abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let axial = dot(
                axis,
                std::array::from_fn(|index| plane.origin[index] - cone.origin[index]),
            );
            let radius = (cone.radius + axial * cone.half_angle.tan()).abs();
            if radius <= 1e-12 {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + axial * axis[index]);
            let reference = normalized(cone.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "plane_cone_circle",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(torus.axis)?;
            if (dot(normal, axis).abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let axial = dot(
                axis,
                std::array::from_fn(|index| plane.origin[index] - torus.center[index]),
            );
            let scale = torus.minor_radius.max(torus.major_radius).max(1.0);
            if (axial.abs() - torus.minor_radius).abs() > 1e-9 * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] + axial * axis[index]);
            let reference = normalized(torus.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: torus.major_radius,
                },
                "plane_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Cylinder(first), CarrierEquation::Cylinder(second)) => {
            let first_axis = normalized(first.axis)?;
            let second_axis = normalized(second.axis)?;
            let alignment = dot(first_axis, second_axis);
            if (alignment.abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let relative = std::array::from_fn(|index| second.origin[index] - first.origin[index]);
            let axial = dot(relative, first_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
            let distance = dot(transverse, transverse).sqrt();
            if distance <= 1e-12 {
                return None;
            }
            let external = first.radius + second.radius;
            let internal = (first.radius - second.radius).abs();
            let scale = external.max(distance).max(1.0);
            let first_fraction = if (distance - external).abs() <= 1e-9 * scale {
                first.radius / distance
            } else if (distance - internal).abs() <= 1e-9 * scale {
                let signed = if first.radius >= second.radius {
                    first.radius
                } else {
                    -first.radius
                };
                signed / distance
            } else {
                return None;
            };
            let origin: [f64; 3] = std::array::from_fn(|index| {
                first.origin[index] + first_fraction * transverse[index]
            });
            Some((
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                },
                "parallel_cylinder_tangent_line",
            ))
        }
        (CarrierEquation::Sphere(first), CarrierEquation::Sphere(second)) => {
            let center_delta: [f64; 3] =
                std::array::from_fn(|index| second.center[index] - first.center[index]);
            let distance = dot(center_delta, center_delta).sqrt();
            if distance <= 1e-12
                || distance >= first.radius + second.radius
                || distance <= (first.radius - second.radius).abs()
            {
                return None;
            }
            let axis = center_delta.map(|value| value / distance);
            let axial = (distance * distance + first.radius * first.radius
                - second.radius * second.radius)
                / (2.0 * distance);
            let radius_squared = first.radius.mul_add(first.radius, -(axial * axial));
            if radius_squared <= 1e-18 {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| first.center[index] + axial * axis[index]);
            let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                axis[0], axis[1], axis[2],
            ));
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: reference,
                    radius: radius_squared.sqrt(),
                },
                "sphere_intersection_circle",
            ))
        }
        (CarrierEquation::Cylinder(cylinder), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Cylinder(cylinder)) => {
            let axis = normalized(cylinder.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - cylinder.origin[index]);
            let axial = dot(relative, axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let scale = sphere.radius.max(cylinder.radius).max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale
                || (sphere.radius - cylinder.radius).abs() > 1e-9 * scale
            {
                return None;
            }
            let reference = normalized(cylinder.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(sphere.center[0], sphere.center[1], sphere.center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_sphere_circle",
            ))
        }
        (CarrierEquation::Cylinder(cylinder), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Cylinder(cylinder)) => {
            let cylinder_axis = normalized(cylinder.axis)?;
            let torus_axis = normalized(torus.axis)?;
            if (dot(cylinder_axis, torus_axis).abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let relative: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] - cylinder.origin[index]);
            let axial = dot(relative, cylinder_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * cylinder_axis[index]);
            let scale = torus
                .major_radius
                .max(torus.minor_radius)
                .max(cylinder.radius)
                .max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let outer_radius = torus.major_radius + torus.minor_radius;
            let inner_radius = (torus.major_radius - torus.minor_radius).abs();
            if (cylinder.radius - outer_radius).abs() > 1e-9 * scale
                && (inner_radius <= 1e-12 || (cylinder.radius - inner_radius).abs() > 1e-9 * scale)
            {
                return None;
            }
            let reference = normalized(cylinder.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(torus.center[0], torus.center[1], torus.center[2]),
                    axis: Vector3::new(cylinder_axis[0], cylinder_axis[1], cylinder_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Cone(cone), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Cone(cone)) => {
            let cone_axis = normalized(cone.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - cone.origin[index]);
            let axial = dot(relative, cone_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * cone_axis[index]);
            let scale = cone.radius.max(sphere.radius).max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let slope = cone.half_angle.tan();
            if slope.abs() <= 1e-12 {
                return None;
            }
            let quadratic = 1.0 + slope * slope;
            let linear = 2.0 * (cone.radius * slope - axial);
            let constant =
                cone.radius * cone.radius + axial * axial - sphere.radius * sphere.radius;
            let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
            let discriminant_scale = linear
                .abs()
                .max((4.0 * quadratic * constant).abs().sqrt())
                .max(1.0);
            if discriminant.abs() > 1e-9 * discriminant_scale * discriminant_scale {
                return None;
            }
            let cone_parameter = -linear / (2.0 * quadratic);
            let radius = (cone.radius + cone_parameter * slope).abs();
            if radius <= 1e-12 * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + cone_parameter * cone_axis[index]);
            let reference = normalized(cone.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(cone_axis[0], cone_axis[1], cone_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_cone_sphere_tangent_circle",
            ))
        }
        (CarrierEquation::Sphere(sphere), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Sphere(sphere)) => {
            let axis = normalized(torus.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] - sphere.center[index]);
            let axial = dot(relative, axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let scale = torus
                .major_radius
                .max(torus.minor_radius)
                .max(sphere.radius)
                .max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let meridian_distance = torus.major_radius.hypot(axial);
            if meridian_distance <= 1e-12 {
                return None;
            }
            let external = sphere.radius + torus.minor_radius;
            let internal = (sphere.radius - torus.minor_radius).abs();
            if (meridian_distance - external).abs() > 1e-9 * scale
                && (meridian_distance - internal).abs() > 1e-9 * scale
            {
                return None;
            }
            let sphere_parameter = (meridian_distance * meridian_distance
                + sphere.radius * sphere.radius
                - torus.minor_radius * torus.minor_radius)
                / (2.0 * meridian_distance);
            let radius = sphere_parameter * torus.major_radius / meridian_distance;
            if radius <= 1e-12 * scale {
                return None;
            }
            let center_axial = sphere_parameter * axial / meridian_distance;
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] + center_axial * axis[index]);
            let reference = normalized(torus.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_sphere_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Torus(first), CarrierEquation::Torus(second)) => {
            let first_axis = normalized(first.axis)?;
            let second_axis = normalized(second.axis)?;
            if (dot(first_axis, second_axis).abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let relative: [f64; 3] =
                std::array::from_fn(|index| second.center[index] - first.center[index]);
            let axial = dot(relative, first_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
            let scale = first
                .major_radius
                .max(first.minor_radius)
                .max(second.major_radius)
                .max(second.minor_radius)
                .max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let radial_delta = second.major_radius - first.major_radius;
            let meridian_distance = radial_delta.hypot(axial);
            if meridian_distance <= 1e-12 {
                return None;
            }
            let external = first.minor_radius + second.minor_radius;
            let internal = (first.minor_radius - second.minor_radius).abs();
            if (meridian_distance - external).abs() > 1e-9 * scale
                && (meridian_distance - internal).abs() > 1e-9 * scale
            {
                return None;
            }
            let first_parameter = (meridian_distance * meridian_distance
                + first.minor_radius * first.minor_radius
                - second.minor_radius * second.minor_radius)
                / (2.0 * meridian_distance);
            let radius = first.major_radius + first_parameter * radial_delta / meridian_distance;
            if radius <= 1e-12 * scale {
                return None;
            }
            let center_axial = first_parameter * axial / meridian_distance;
            let center: [f64; 3] =
                std::array::from_fn(|index| first.center[index] + center_axial * first_axis[index]);
            let reference = normalized(first.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_tori_tangent_circle",
            ))
        }
        (
            CarrierEquation::Cone(_),
            CarrierEquation::Cylinder(_) | CarrierEquation::Cone(_) | CarrierEquation::Torus(_),
        )
        | (CarrierEquation::Cylinder(_) | CarrierEquation::Torus(_), CarrierEquation::Cone(_)) => {
            None
        }
    }
}

fn transfer_carrier_intersection_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let carriers = placed_carriers(scan, ir);
    for row in &scan.curve_topology_rows {
        let (Some(first), Some(second)) = (
            carriers.get(&row.faces[0]).copied(),
            carriers.get(&row.faces[1]).copied(),
        ) else {
            continue;
        };
        let Some((geometry, tag)) = carrier_intersection_curve(first, second) else {
            continue;
        };
        let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            tag,
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry,
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

fn rowless_round_cylinder_pairs(
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Vec<(u32, u32, usize)> {
    tables
        .iter()
        .filter_map(|table| {
            let feature_id = table.feature_id?;
            round_feature_ids.contains(&feature_id).then_some(())?;
            let [first, second, rowless, cylinder] = table.entry_ids.as_slice() else {
                return None;
            };
            rows.iter().any(|row| row.id == *first).then_some(())?;
            rows.iter().any(|row| row.id == *second).then_some(())?;
            (!rows.iter().any(|row| row.id == *rowless)).then_some(())?;
            rows.iter()
                .any(|row| {
                    row.id == *cylinder
                        && row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Cylinder
                })
                .then_some(())?;
            Some((*rowless, *cylinder, table.offset))
        })
        .collect()
}

fn transfer_rowless_round_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let round_feature_ids = scan
        .feature_rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for (rowless_id, sibling_id, offset) in rowless_round_cylinder_pairs(
        &round_feature_ids,
        &scan.feature_entity_tables,
        &scan.surface_rows,
    ) {
        let sibling = SurfaceId(format!("creo:visibgeom:surface#{sibling_id}"));
        let Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        }) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == sibling)
            .map(|surface| &surface.geometry)
        else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{rowless_id}"));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "AllFeatur",
            offset as u64,
            "round_rowless_sibling_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: *origin,
                axis: *axis,
                ref_direction: *ref_direction,
                radius: *radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("AllFeatur:{rowless_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

fn transfer_hole_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let hole_feature_ids = scan
        .feature_rows
        .iter()
        .filter(|row| row.root_schema_class == Some(911))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in hole_feature_ids {
        let Some(hole) = simple_hole_geometry(scan, feature_id) else {
            continue;
        };
        for cylinder_id in hole.cylinder_ids {
            let row = scan
                .surface_rows
                .iter()
                .find(|row| row.id == cylinder_id)
                .expect("validated cylinder row");
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "AllFeatur",
                row.offset as u64,
                "hole_cap_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: hole.geometry.clone(),
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
            transferred += 1;
        }
    }
    transferred
}

fn transfer_circular_sweep_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let sweep_feature_ids = scan
        .feature_rows
        .iter()
        .filter(|row| row.root_schema_class == Some(917))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in sweep_feature_ids {
        let tables = scan
            .feature_entity_tables
            .iter()
            .filter(|table| table.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        let [table] = tables.as_slice() else {
            continue;
        };
        let [first_plane, second_plane, first_cylinder, second_cylinder] =
            table.entry_ids.as_slice()
        else {
            continue;
        };
        let placed_planes = feature_outline_planes(scan, feature_id);
        let [first, second] = placed_planes.as_slice() else {
            continue;
        };
        if first.0 != *first_plane || second.0 != *second_plane {
            continue;
        }
        let cap = |plane: &(u32, [f64; 3], [f64; 3])| {
            let envelopes = scan
                .plane_envelopes
                .iter()
                .filter(|envelope| envelope.surface_id == plane.0)
                .collect::<Vec<_>>();
            let corners = match envelopes.as_slice() {
                [envelope] => plane_envelope_corners(&envelope.envelope),
                _ => None,
            };
            (plane.0, plane.1, plane.2, corners)
        };
        let cylinder_ids = [*first_cylinder, *second_cylinder];
        if cylinder_ids.iter().any(|id| {
            !scan.surface_rows.iter().any(|row| {
                row.id == *id
                    && row.feature_id == feature_id
                    && row.kind == crate::surface::SurfaceKind::Cylinder
            })
        }) {
            continue;
        }
        let Some(geometry) = circular_sweep_cylinder_from_cap_outlines([cap(first), cap(second)])
        else {
            continue;
        };
        for cylinder_id in cylinder_ids {
            let row = scan
                .surface_rows
                .iter()
                .find(|row| row.id == cylinder_id)
                .expect("validated cylinder row");
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "AllFeatur",
                row.offset as u64,
                "circular_sweep_cap_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: geometry.clone(),
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
            transferred += 1;
        }
    }
    transferred
}

/// Decode a `.prt` stream into an IR document and loss report.
///
/// The stream is read from its beginning. When `options.container_only` is set,
/// the returned IR contains source metadata and preserved geometry sections but
/// no transferred entities.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if options.container_only {
        let ir = build_container_ir(&scan)?;
        let report = build_report(&scan, &ir, true);
        return Ok(DecodeResult::new(ir, report));
    }

    let ir = build_ir(&scan)?;
    let report = build_report(&scan, &ir, false);
    Ok(DecodeResult::new(ir, report))
}

fn preserve_passthrough_sections(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    for section in scan
        .sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY || section.role == role::THUMBNAIL)
    {
        let end = (section.offset + section.length).min(scan.data.len());
        let section_bytes = &scan.data[section.offset..end];
        let payload_start = if section.role == role::THUMBNAIL {
            let Some(offset) = section_bytes
                .windows(3)
                .position(|window| window == [0xff, 0xd8, 0xff])
            else {
                continue;
            };
            offset
        } else {
            0
        };
        let bytes = &section_bytes[payload_start..];
        let offset = section.offset + payload_start;
        let id = UnknownId(format!("creo:{}:section#{}", section.name, offset));
        let (tag, exactness) = if section.role == role::THUMBNAIL {
            ("jpeg_thumbnail", Exactness::ByteExact)
        } else {
            ("psb_geometry_section", Exactness::Unknown)
        };
        annotate(
            annotations,
            &id,
            &section.name,
            offset as u64,
            tag,
            exactness,
        );
        ir.push_native_unknown(
            "creo",
            UnknownRecord {
                id,
                offset: offset as u64,
                byte_len: bytes.len() as u64,
                sha256: sha256_hex(bytes),
                data: Some(bytes.to_vec()),
                links: Vec::new(),
            },
        )?;
    }
    Ok(())
}

fn build_container_ir(scan: &ContainerScan) -> Result<CadIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    preserve_passthrough_sections(scan, &mut ir, &mut annotations)?;
    ir.annotations = annotations.build();
    Ok(ir)
}

/// Build source metadata, preserved geometry records, and transferred entities.
fn build_ir(scan: &ContainerScan) -> Result<CadIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    preserve_passthrough_sections(scan, &mut ir, &mut annotations)?;
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
    let saved_spline_curve_count = transfer_saved_spline_curves(scan, &mut ir, &mut annotations);
    transfer_resolved_sketches(scan, &mut ir, &mut annotations);
    let feature_revolution_surface_count =
        transfer_resolved_revolution_surfaces(scan, &mut ir, &mut annotations);
    let feature_extrusion_surface_count =
        transfer_feature_extrusion_surfaces(scan, &mut ir, &mut annotations);
    let circular_sweep_cylinder_count =
        transfer_circular_sweep_cylinders(scan, &mut ir, &mut annotations);
    let hole_cylinder_count = transfer_hole_cylinders(scan, &mut ir, &mut annotations);
    let rowless_round_cylinder_count =
        transfer_rowless_round_cylinders(scan, &mut ir, &mut annotations);
    transfer_carrier_intersection_curves(scan, &mut ir, &mut annotations);
    transfer_plane_brep(scan, &mut ir, &mut annotations);
    let feature_extrusion_brep_count =
        transfer_resolved_extrusion_breps(scan, &mut ir, &mut annotations);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "transferred_saved_spline_curve_count".to_string(),
            saved_spline_curve_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_revolution_surface_count".to_string(),
            feature_revolution_surface_count.to_string(),
        );
        source.attributes.insert(
            "transferred_feature_extrusion_surface_count".to_string(),
            feature_extrusion_surface_count.to_string(),
        );
        source.attributes.insert(
            "transferred_circular_sweep_cylinder_count".to_string(),
            circular_sweep_cylinder_count.to_string(),
        );
        source.attributes.insert(
            "transferred_hole_cylinder_count".to_string(),
            hole_cylinder_count.to_string(),
        );
        source.attributes.insert(
            "transferred_rowless_round_cylinder_count".to_string(),
            rowless_round_cylinder_count.to_string(),
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
            || {
                named_feature_definition(scan, &ir, operation.feature_id, &operation.kind)
                    .unwrap_or_else(|| IrFeatureDefinition::Native {
                        kind: operation.kind.clone(),
                        parameters: parameters.clone(),
                        properties: BTreeMap::new(),
                    })
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
        let parent = operation.parent_feature_id.and_then(|parent_feature_id| {
            let parent = IrFeatureId(format!("creo:model:feature#{parent_feature_id}"));
            ir.model
                .features
                .iter()
                .any(|feature| feature.id == parent)
                .then_some(parent)
        });
        let operation_section = scan
            .sections
            .iter()
            .find(|section| {
                operation.offset >= section.offset
                    && operation.offset < section.offset.saturating_add(section.length)
            })
            .map_or("MdlStatus", |section| section.name.as_str());
        let name = operation
            .display_name_stored
            .then(|| format!("{} id {}", operation.kind, operation.feature_id));
        let source_tag = operation.recipe.map(|recipe| match recipe {
            crate::feature::FeatureRecipeKind::Extrude => "protextrude".to_string(),
            crate::feature::FeatureRecipeKind::Revolve => "protrevolve".to_string(),
        });
        if let Some(existing) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id == id)
        {
            if name.is_some() {
                existing.name = name;
            }
            if existing.parent.is_none() {
                existing.parent = parent;
            }
            for dependency in dependencies {
                if !existing.dependencies.contains(&dependency) {
                    existing.dependencies.push(dependency);
                }
            }
            existing.source_properties.extend(source_properties);
            if source_tag.is_some() {
                existing.source_tag = source_tag;
            }
            for output in outputs {
                if !existing.outputs.contains(&output) {
                    existing.outputs.push(output);
                }
            }
            continue;
        }
        annotate(
            &mut annotations,
            &id,
            operation_section,
            operation.offset as u64,
            if operation.display_name_stored {
                "feature_operation_name"
            } else {
                "feature_recipe"
            },
            Exactness::ByteExact,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: (operation_ordinal_base + operation_index) as u64,
            name,
            suppressed: false,
            parent,
            dependencies,
            source_properties,
            source_tag,
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
    let pcurve_endpoints = pcurve_endpoint_records(scan);
    if !pcurve_endpoints.is_empty() {
        for (record, offset) in &pcurve_endpoints {
            annotate(
                &mut annotations,
                &record.id,
                "VisibGeom",
                *offset as u64,
                "pcurve_endpoint_frames",
                Exactness::Derived,
            );
        }
        let records = pcurve_endpoints
            .iter()
            .map(|(record, _)| record)
            .collect::<Vec<_>>();
        let namespace = ir.native.namespace_mut("creo");
        namespace.version = 1;
        namespace.set_arena("pcurve_endpoints", &records)?;
    }
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
    if let Some(name) = &scan.model_name {
        attributes.insert("model_name".to_string(), name.clone());
    }
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
fn has_transferred_geometry(ir: &CadIr) -> bool {
    let model = &ir.model;
    !model.points.is_empty()
        || model
            .surfaces
            .iter()
            .any(|surface| !matches!(&surface.geometry, SurfaceGeometry::Unknown { .. }))
        || model
            .curves
            .iter()
            .any(|curve| !matches!(&curve.geometry, CurveGeometry::Unknown { .. }))
        || !model.subds.is_empty()
        || !model.pcurves.is_empty()
        || model.procedural_surfaces.iter().any(|surface| {
            !matches!(
                &surface.definition,
                ProceduralSurfaceDefinition::Unknown { .. }
            )
        })
        || model
            .procedural_curves
            .iter()
            .any(|curve| !matches!(&curve.definition, ProceduralCurveDefinition::Unknown { .. }))
        || model
            .sketch_entities
            .iter()
            .any(|entity| !matches!(&entity.geometry, SketchGeometry::Native { .. }))
        || !model.tessellations.is_empty()
}

fn build_report(scan: &ContainerScan, ir: &CadIr, container_only: bool) -> DecodeReport {
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
            message: "Container-only decode requested; entity transfer was skipped.".to_string(),
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
             Outline-backed planes, guarded non-axis support frames, topology-bound `fc 05` \
             cylinders with a resolved axis-normal cap plane, four-entry circular-sweep cylinders, \
             and four-entry simple-hole cylinders with complete cap outlines transfer as carriers; \
             other parameter bodies remain structural records.",
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
            "General model B-rep transfer remains incomplete. Exact planar components transfer \
             when every loop is solved from placed-plane intersections and one strict containment \
             outer boundary exists per face. Selected \
             cylinders transfer when either an exact `fc 05` record and placed cap outline or a \
             four-entry class-917 circular-sweep or class-911 simple-hole table with a complete \
             square cap outline establishes the complete axis placement, parameterization, and \
             radius. VisibGeom \
             stores one surface prototype per family (a first-instance template), not per-instance \
             located geometry, so other prototype scalars cannot be emitted as model surfaces \
             without mislabeling most instances \
             ([spec §4.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#32-surface-prototypes)). {geom_sections} PSB geometry section(s) were preserved verbatim as unknown \
             records."
        ),
        provenance: None,
    });

    if !container_only && placed_plane_count != 0 {
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

    if !container_only && !scan.datum_planes.is_empty() {
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
                  typed or native design records. Curve-equation assignments transfer with their \
                  source, dependencies, and closed arithmetic values. Full neutral operation \
                  semantics, configurations, remaining expression families, materials, and \
                  display data remain untransferred."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "creo".to_string(),
        container_only,
        geometry_transferred: has_transferred_geometry(ir),
        losses,
        notes: summary.notes,
    }
}
