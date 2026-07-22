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
    Feature, FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId, UnknownId,
    VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
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
    feature_id: u32,
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
    body: Vec<u8>,
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
            feature_id: definition.id,
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
                    body: relation.body.clone(),
                })
                .collect(),
        })
        .collect()
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

fn transfer_plane_brep(scan: &ContainerScan, ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let planes = scan
        .plane_local_systems
        .iter()
        .filter_map(|frame| {
            Some((
                frame.surface_id,
                PlaneEquation {
                    origin: frame.origin?,
                    normal: frame.normal?,
                },
            ))
        })
        .collect::<BTreeMap<_, _>>();
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
            source_object: None,
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
            curve: None,
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
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges: coedge_ids.clone(),
                vertex_uses: Vec::new(),
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
                    pcurves: Vec::new(),
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }
        }
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

    let (mut ir, annotations, unknowns) = build_ir(&scan)?;
    let report = build_report(&scan, options.container_only);
    let mut source_fidelity = cadmpeg_ir::SourceFidelity {
        annotations,
        ..cadmpeg_ir::SourceFidelity::default()
    };
    source_fidelity.attach_native_unknown_records(&mut ir, "creo", &unknowns)?;
    Ok(DecodeResult::with_source_fidelity(
        ir,
        report,
        source_fidelity,
    ))
}

/// Build source metadata, preserved geometry records, and datum-plane surfaces.
fn build_ir(
    scan: &ContainerScan,
) -> Result<(CadIr, cadmpeg_ir::Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
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
        unknowns.push(UnknownRecord {
            id,
            offset: section.offset as u64,
            byte_len: bytes.len() as u64,
            sha256: sha256_hex(bytes),
            data: Some(bytes.to_vec()),
            links: Vec::new(),
        });
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
    transfer_plane_brep(scan, &mut ir, &mut annotations);
    for (ordinal, operation) in scan.feature_operations.iter().enumerate() {
        let id = IrFeatureId(format!("creo:mdlstatus:feature#{}", operation.feature_id));
        let mut parameters = BTreeMap::new();
        for affected in scan
            .feature_affected_ids
            .iter()
            .filter(|record| record.feature_id == operation.feature_id)
        {
            let name = match affected.kind {
                crate::feature::AffectedIdKind::Geometry => "affected_geometry_ids",
                crate::feature::AffectedIdKind::Edges => "affected_edge_ids",
                crate::feature::AffectedIdKind::StrongParents => "strong_parent_feature_ids",
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
        for direction in scan
            .feature_direction_bytes
            .iter()
            .filter(|record| record.feature_id == operation.feature_id)
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
            .find(|definition| definition.id == operation.feature_id)
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
        let parent = scan
            .feature_affected_ids
            .iter()
            .find(|record| {
                record.feature_id == operation.feature_id
                    && record.kind == crate::feature::AffectedIdKind::StrongParents
            })
            .and_then(|record| record.ids.first())
            .filter(|parent| {
                scan.feature_operations
                    .iter()
                    .any(|candidate| candidate.feature_id == **parent)
            })
            .map(|parent| IrFeatureId(format!("creo:mdlstatus:feature#{parent}")));
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
            ordinal: ordinal as u64,
            name: Some(format!("{} id {}", operation.kind, operation.feature_id)),
            suppressed: false,
            parent,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: IrFeatureDefinition::Native {
                kind: operation.kind.clone(),
                parameters,
                properties: BTreeMap::new(),
            },
            native_ref: None,
        });
    }
    let sketches = sketch_records(scan);
    if !sketches.is_empty() {
        for sketch in &sketches {
            let offset = scan
                .feature_definitions
                .iter()
                .find(|definition| definition.id == sketch.feature_id)
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
    Ok((ir, annotations.build(), unknowns))
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
    let placed_plane_count = scan
        .plane_local_systems
        .iter()
        .filter(|frame| frame.origin.is_some() && frame.normal.is_some() && frame.u_axis.is_some())
        .count();

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
             Complete plane support frames transfer as placed carriers; other parameter bodies \
             remain structural records.",
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
             transfer when their vertices are determined by placed-plane intersections. VisibGeom \
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
                  transfer of non-plane per-instance surfaces, curves, and vertices."
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
                  native design records. Full neutral operation semantics, configurations, \
                  expressions, materials, and display data remain untransferred."
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
