// SPDX-License-Identifier: Apache-2.0
//! E5-stream decode route: analytic carriers, plane fitting, and topology transfer.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve, Pcurve,
    PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, Surface, SurfaceCurveFamily,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::assemble::{
    annotate, circle_parameter_range_from_surface_branch, insert_unresolved_carrier_loss,
    link_payload_carriers, neutral_model_is_admissible, ordered_range, preserve_raw_payload,
    quintic_jet_pcurve, rational_pcurve_arc, source_meta, unit_vector,
};
use crate::container::{self, ContainerScan};
use crate::families::FamilyOutput;
use crate::solve::UnionFind;

/// Decode direct E5 circle carriers.  Their edge and face references are a
/// separate record layer, so curves remain unattached until that layer is
/// decoded rather than being assigned speculatively.
pub(crate) fn try_decode_e5(scan: &ContainerScan) -> Option<FamilyOutput> {
    let stream_range = container::e5_record_stream(&scan.data)?;
    let stream = &scan.data[stream_range];
    let circles = crate::families::e5::records::e5_circles(stream);
    let mut surfaces = crate::families::e5::records::e5_surfaces(stream);
    let topology = crate::families::e5::graph::parse_topology(stream);
    let vertex_count = topology.as_ref().map_or_else(
        || {
            crate::families::e5::records::e5_edges(stream)
                .into_iter()
                .flat_map(|edge| [edge.start_vertex_id, edge.end_vertex_id])
                .collect::<HashSet<_>>()
                .len()
        },
        |topology| topology.vertex_refs.len(),
    );
    let points = crate::families::e5::records::e5_vertices(&scan.data, vertex_count);
    if let Some(topology) = &topology {
        append_e5_planes(stream, topology, &points, &mut surfaces);
    }
    if circles.is_empty() && surfaces.is_empty() && points.is_empty() {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut unknowns,
        &mut annotations,
        scan,
        "catia:payload:unknown#e5",
    );
    for (index, point) in points.iter().enumerate() {
        let point_id = PointId(format!("catia:e5:pt#{index}"));
        annotate(
            &mut annotations,
            &point_id,
            "e5_0d_03",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: *point,
            source_object: None,
        });
        let vertex_id = VertexId(format!("catia:e5:v#{index}"));
        annotate(
            &mut annotations,
            &vertex_id,
            "MainDataStream+SurfacicReps",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        );
        annotations.derived(&vertex_id, "point");
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    for (index, circle) in circles.iter().enumerate() {
        let id = CurveId(format!("catia:e5:curve#{index}"));
        annotate(
            &mut annotations,
            &id,
            "e5_0d_03",
            circle.pos as u64,
            "circle_carrier",
            Exactness::ByteExact,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: circle.geometry.clone(),
            source_object: None,
        });
    }
    for (index, surface) in surfaces.iter().enumerate() {
        let id = SurfaceId(format!("catia:e5:surf#{index}"));
        annotate(
            &mut annotations,
            &id,
            "e5_0d_03",
            surface.pos as u64,
            "analytic_surface",
            if matches!(surface.geometry, SurfaceGeometry::Plane { .. }) {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: surface.geometry.clone(),
            source_object: None,
        });
    }
    let mut topology_ir = ir.clone();
    let mut topology_annotations = annotations.clone();
    let topology_transferred = topology.as_ref().is_some_and(|topology| {
        transfer_e5_topology(
            &mut topology_ir,
            &mut topology_annotations,
            topology,
            &surfaces,
        ) && neutral_model_is_admissible(&topology_ir, &unknowns)
    });
    if topology_transferred {
        ir = topology_ir;
        annotations = topology_annotations;
    } else if !ir.model.vertices.is_empty() {
        attach_e5_free_vertices(&mut ir, &mut annotations);
    }
    let mut losses = if topology_transferred {
        let message = if topology
            .as_ref()
            .is_some_and(|topology| topology.bodies.is_empty())
        {
            "The E5 reference graph is closed; body ownership and shell orientation use an incidence-derived gauge because the stream has no class-0x01 body root."
        } else {
            "The E5 reference graph is closed; face and loop orientation transfer, but body/shell orientation uses an incidence-derived gauge because the root's two trailing orientation signs remain unresolved."
        };
        vec![LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: message.to_string(),
            provenance: None,
        }]
    } else {
        vec![LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "E5 analytic carriers were decoded, but the reference graph could not be transferred with a closed surface/pcurve/vertex binding."
                .to_string(),
            provenance: None,
        }]
    };
    insert_unresolved_carrier_loss(&ir, &mut losses);
    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let annotations = annotations.build();
    Some(FamilyOutput {
        ir,
        report: DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            losses,
            notes: container::summarize(scan).notes,
        },
        annotations,
        unknowns,
    })
}

pub(crate) fn append_e5_planes(
    stream: &[u8],
    topology: &crate::families::e5::graph::E5Topology,
    points: &[Point3],
    surfaces: &mut Vec<crate::families::e5::records::E5Surface>,
) {
    let carrier_axes: HashMap<u32, Vector3> = surfaces
        .iter()
        .filter_map(|surface| {
            let (SurfaceGeometry::Cylinder { axis, .. }
            | SurfaceGeometry::Cone { axis, .. }
            | SurfaceGeometry::Torus { axis, .. }) = surface.geometry
            else {
                return None;
            };
            Some((surface.record_id, axis))
        })
        .collect();
    for plane in crate::families::e5::records::e5_planes(stream) {
        let mut normal: Option<Vector3> = None;
        let mut consistent = true;
        for face in topology
            .faces
            .iter()
            .filter(|face| face.surface == plane.record_id)
        {
            for loop_ in &face.loops {
                for edge_ref in &loop_.edge_uses {
                    let Some(edge) = topology.edges.get(edge_ref) else {
                        consistent = false;
                        continue;
                    };
                    let Some(support) = topology.curve_supports.get(&edge.support) else {
                        consistent = false;
                        continue;
                    };
                    for pcurve_ref in &support.pcurves {
                        let Some(crate::families::e5::graph::E5Pcurve::Line {
                            surface,
                            direction,
                            ..
                        }) = topology.pcurves.get(pcurve_ref)
                        else {
                            continue;
                        };
                        if direction[0].abs() <= 1e-9 || direction[1].abs() > 1e-9 {
                            continue;
                        }
                        let Some(&candidate) = carrier_axes.get(surface) else {
                            continue;
                        };
                        let candidate = canonical_direction(candidate);
                        if normal.is_some_and(|value| {
                            value.x * candidate.x + value.y * candidate.y + value.z * candidate.z
                                < 1.0 - 1e-10
                        }) {
                            consistent = false;
                        } else {
                            normal = Some(candidate);
                        }
                    }
                }
            }
        }
        let expected_normal = normal.filter(|_| consistent);
        let Some((normal, u_axis)) = solve_e5_plane_frame(
            plane.record_id,
            plane.origin,
            topology,
            points,
            expected_normal,
        ) else {
            continue;
        };
        surfaces.push(crate::families::e5::records::E5Surface {
            pos: plane.pos,
            record_id: plane.record_id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                normal,
                u_axis,
            },
            uv_scale: [1.0, 1.0],
        });
    }
}

pub(crate) fn solve_e5_plane_frame(
    surface_ref: u32,
    origin: [f64; 3],
    topology: &crate::families::e5::graph::E5Topology,
    points: &[Point3],
    expected_normal: Option<Vector3>,
) -> Option<(Vector3, Vector3)> {
    if topology.vertex_refs.len() != points.len() {
        return None;
    }
    let point_by_ref: HashMap<u32, Point3> = topology
        .vertex_refs
        .iter()
        .copied()
        .zip(points.iter().copied())
        .collect();
    let mut segments = Vec::new();
    for face in topology
        .faces
        .iter()
        .filter(|face| face.surface == surface_ref)
    {
        for loop_ in &face.loops {
            for (&pcurve_ref, &edge_ref) in loop_.pcurves.iter().zip(&loop_.edge_uses) {
                let edge = topology.edges.get(&edge_ref)?;
                let pcurve = topology.pcurves.get(&pcurve_ref)?;
                let uv = e5_native_uv_endpoints(pcurve)?;
                let (Some(start), Some(end)) = (
                    point_by_ref.get(&edge.start_vertex),
                    point_by_ref.get(&edge.end_vertex),
                ) else {
                    return None;
                };
                segments.push((uv, [*start, *end]));
            }
        }
    }
    if segments.is_empty() || segments.len() > 16 {
        return None;
    }
    let mut candidates = Vec::new();
    for mask in 0usize..(1usize << segments.len()) {
        let mut pairs = Vec::with_capacity(2 * segments.len());
        for (index, (uv, xyz)) in segments.iter().enumerate() {
            let reversed = mask & (1 << index) != 0;
            pairs.push((uv[0], xyz[usize::from(reversed)]));
            pairs.push((uv[1], xyz[usize::from(!reversed)]));
        }
        let Some((u_axis, v_axis, residual)) = fit_e5_plane_axes(origin, &pairs).or_else(|| {
            expected_normal.and_then(|normal| fit_rank_one_e5_plane_axes(origin, &pairs, normal))
        }) else {
            continue;
        };
        let Some(u_axis) = unit_vector(u_axis) else {
            continue;
        };
        let Some(v_axis) = unit_vector(v_axis) else {
            continue;
        };
        if residual > 2e-3 || u_axis.dot(v_axis).abs() > 1e-6 {
            continue;
        }
        let Some(normal) = unit_vector(Vector3::new(
            u_axis.y * v_axis.z - u_axis.z * v_axis.y,
            u_axis.z * v_axis.x - u_axis.x * v_axis.z,
            u_axis.x * v_axis.y - u_axis.y * v_axis.x,
        )) else {
            continue;
        };
        if expected_normal.is_some_and(|expected| normal.dot(expected).abs() < 1.0 - 1e-6) {
            continue;
        }
        if !candidates
            .iter()
            .any(|(_, existing): &(Vector3, Vector3)| (*existing).dot(u_axis) > 1.0 - 1e-8)
        {
            candidates.push((normal, u_axis));
        }
    }
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }
    let canonical: Vec<_> = candidates
        .into_iter()
        .filter(|(_, u_axis)| {
            [u_axis.x, u_axis.y, u_axis.z]
                .into_iter()
                .find(|value| value.abs() > 1e-12)
                .is_some_and(|value| value > 0.0)
        })
        .collect();
    (canonical.len() == 1).then(|| canonical[0])
}

pub(crate) fn e5_native_uv_endpoints(
    pcurve: &crate::families::e5::graph::E5Pcurve,
) -> Option<[[f64; 2]; 2]> {
    match pcurve {
        crate::families::e5::graph::E5Pcurve::Line {
            origin,
            direction,
            range,
            ..
        } => Some(range.map(|parameter| {
            [
                origin[0] + parameter * direction[0],
                origin[1] + parameter * direction[1],
            ]
        })),
        crate::families::e5::graph::E5Pcurve::Circle {
            center,
            radius,
            range,
            ..
        } => Some(range.map(|parameter| {
            let angle = parameter / radius;
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]
        })),
        crate::families::e5::graph::E5Pcurve::Jet { points, .. } => {
            Some([*points.first()?, *points.last()?])
        }
    }
}

pub(crate) fn fit_e5_plane_axes(
    origin: [f64; 3],
    pairs: &[([f64; 2], Point3)],
) -> Option<(Vector3, Vector3, f64)> {
    let suu = pairs.iter().map(|(uv, _)| uv[0] * uv[0]).sum::<f64>();
    let suv = pairs.iter().map(|(uv, _)| uv[0] * uv[1]).sum::<f64>();
    let svv = pairs.iter().map(|(uv, _)| uv[1] * uv[1]).sum::<f64>();
    let determinant = suu * svv - suv * suv;
    if determinant.abs() <= 1e-18 {
        return None;
    }
    let mut u = [0.0; 3];
    let mut v = [0.0; 3];
    for axis in 0..3 {
        let bu = pairs
            .iter()
            .map(|(uv, point)| uv[0] * ([point.x, point.y, point.z][axis] - origin[axis]))
            .sum::<f64>();
        let bv = pairs
            .iter()
            .map(|(uv, point)| uv[1] * ([point.x, point.y, point.z][axis] - origin[axis]))
            .sum::<f64>();
        u[axis] = (bu * svv - bv * suv) / determinant;
        v[axis] = (suu * bv - suv * bu) / determinant;
    }
    let mut residual = 0.0f64;
    for (uv, point) in pairs {
        let predicted = [
            origin[0] + uv[0] * u[0] + uv[1] * v[0],
            origin[1] + uv[0] * u[1] + uv[1] * v[1],
            origin[2] + uv[0] * u[2] + uv[1] * v[2],
        ];
        residual = residual.max(
            ((predicted[0] - point.x).powi(2)
                + (predicted[1] - point.y).powi(2)
                + (predicted[2] - point.z).powi(2))
            .sqrt(),
        );
    }
    Some((
        Vector3::new(u[0], u[1], u[2]),
        Vector3::new(v[0], v[1], v[2]),
        residual,
    ))
}

pub(crate) fn fit_rank_one_e5_plane_axes(
    origin: [f64; 3],
    pairs: &[([f64; 2], Point3)],
    normal: Vector3,
) -> Option<(Vector3, Vector3, f64)> {
    let (uv, point) = pairs.iter().find(|(uv, _)| uv[0].hypot(uv[1]) > 1e-9)?;
    let uv_norm = uv[0].hypot(uv[1]);
    let q = [uv[0] / uv_norm, uv[1] / uv_norm];
    let displacement = Vector3::new(
        point.x - origin[0],
        point.y - origin[1],
        point.z - origin[2],
    );
    if (displacement.norm() - uv_norm).abs() > 2e-3 {
        return None;
    }
    let mapped_q = unit_vector(displacement)?;
    let mapped_r = unit_vector(Vector3::new(
        normal.y * mapped_q.z - normal.z * mapped_q.y,
        normal.z * mapped_q.x - normal.x * mapped_q.z,
        normal.x * mapped_q.y - normal.y * mapped_q.x,
    ))?;
    let u_axis = Vector3::new(
        q[0] * mapped_q.x - q[1] * mapped_r.x,
        q[0] * mapped_q.y - q[1] * mapped_r.y,
        q[0] * mapped_q.z - q[1] * mapped_r.z,
    );
    let v_axis = Vector3::new(
        q[1] * mapped_q.x + q[0] * mapped_r.x,
        q[1] * mapped_q.y + q[0] * mapped_r.y,
        q[1] * mapped_q.z + q[0] * mapped_r.z,
    );
    let residual = plane_frame_residual(origin, pairs, u_axis, v_axis);
    Some((u_axis, v_axis, residual))
}

pub(crate) fn plane_frame_residual(
    origin: [f64; 3],
    pairs: &[([f64; 2], Point3)],
    u_axis: Vector3,
    v_axis: Vector3,
) -> f64 {
    pairs.iter().fold(0.0f64, |residual, (uv, point)| {
        let predicted = [
            origin[0] + uv[0] * u_axis.x + uv[1] * v_axis.x,
            origin[1] + uv[0] * u_axis.y + uv[1] * v_axis.y,
            origin[2] + uv[0] * u_axis.z + uv[1] * v_axis.z,
        ];
        residual.max(
            ((predicted[0] - point.x).powi(2)
                + (predicted[1] - point.y).powi(2)
                + (predicted[2] - point.z).powi(2))
            .sqrt(),
        )
    })
}

pub(crate) fn canonical_direction(mut direction: Vector3) -> Vector3 {
    let first = [direction.x, direction.y, direction.z]
        .into_iter()
        .find(|value| value.abs() > 1e-12)
        .unwrap_or(1.0);
    if first < 0.0 {
        direction = Vector3::new(-direction.x, -direction.y, -direction.z);
    }
    direction
}

pub(crate) fn attach_e5_free_vertices(ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let body_id = BodyId("catia:e5:body#unbound-points".to_string());
    let region_id = RegionId("catia:e5:region#unbound-points".to_string());
    let shell_id = ShellId("catia:e5:shell#unbound-points".to_string());
    for id in [&body_id.0, &region_id.0, &shell_id.0] {
        annotate(
            annotations,
            id,
            "e5_0d_03",
            0,
            "unbound_point_owner",
            Exactness::Inferred,
        );
    }
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Wire,
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
        id: shell_id,
        region: region_id,
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices: ir
            .model
            .vertices
            .iter()
            .map(|vertex| vertex.id.clone())
            .collect(),
    });
}

pub(crate) struct E5IntersectionSidePlan {
    surface: SurfaceId,
    pcurve: PcurveGeometry,
    pcurve_range: [f64; 2],
    curve: CurveGeometry,
    curve_range: [f64; 2],
}

/// Boundary lowering plan built by [`plan_e5_boundary`].
#[allow(clippy::struct_field_names)]
struct E5BoundaryPlan {
    pcurve_plan: BTreeMap<u32, (PcurveGeometry, [f64; 2])>,
    edge_curve_plan: BTreeMap<u32, (CurveGeometry, [f64; 2])>,
    surface_curve_plan: BTreeMap<u32, (SurfaceId, PcurveGeometry, [f64; 2])>,
    intersection_plan: BTreeMap<u32, IntcurveSupportContext>,
}

/// Body/region/shell ownership resolved by [`resolve_e5_ownership`].
struct E5Ownership {
    body_faces: Vec<(Option<u32>, Vec<u32>)>,
    ownership: Vec<E5BodyOwnership>,
    face_shell: HashMap<u32, ShellId>,
}

pub(crate) fn transfer_e5_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::families::e5::graph::E5Topology,
    decoded_surfaces: &[crate::families::e5::records::E5Surface],
) -> bool {
    if topology.vertex_refs.len() != ir.model.vertices.len()
        || topology.vertex_refs.len() != ir.model.points.len()
        || topology.vertex_refs.is_empty()
    {
        return false;
    }

    for curve in ir.model.curves.drain(..) {
        annotations.remove_entity(&curve.id);
    }

    let surface_for_ref: HashMap<u32, (SurfaceId, &crate::families::e5::records::E5Surface)> =
        decoded_surfaces
            .iter()
            .enumerate()
            .map(|(index, surface)| {
                (
                    surface.record_id,
                    (SurfaceId(format!("catia:e5:surf#{index}")), surface),
                )
            })
            .collect();
    let vertex_for_ref: HashMap<u32, VertexId> = topology
        .vertex_refs
        .iter()
        .enumerate()
        .map(|(index, reference)| (*reference, VertexId(format!("catia:e5:v#{index}"))))
        .collect();
    let point_for_ref: HashMap<u32, Point3> = topology
        .vertex_refs
        .iter()
        .zip(&ir.model.points)
        .map(|(reference, point)| (*reference, point.position))
        .collect();

    let Some(boundary) = plan_e5_boundary(topology, &surface_for_ref, &point_for_ref) else {
        return false;
    };
    let E5BoundaryPlan {
        pcurve_plan,
        edge_curve_plan,
        surface_curve_plan,
        intersection_plan,
    } = boundary;

    prune_e5_unused_surfaces(
        ir,
        annotations,
        topology,
        &surface_for_ref,
        &intersection_plan,
        &surface_curve_plan,
    );

    let Some(e5_ownership) = resolve_e5_ownership(topology) else {
        return false;
    };
    let E5Ownership {
        body_faces,
        ownership,
        face_shell,
    } = e5_ownership;

    let edge_ids: HashMap<u32, EdgeId> = topology
        .edges
        .keys()
        .map(|record_id| (*record_id, EdgeId(format!("catia:e5:edge#{record_id}"))))
        .collect();
    emit_e5_curves_and_edges(
        ir,
        annotations,
        topology,
        &vertex_for_ref,
        &edge_ids,
        &edge_curve_plan,
        &intersection_plan,
        &surface_curve_plan,
    );
    emit_e5_pcurves(ir, annotations, pcurve_plan);
    emit_e5_bodies(ir, annotations, &body_faces, &ownership);
    emit_e5_faces_loops_coedges(
        ir,
        annotations,
        topology,
        &surface_for_ref,
        &face_shell,
        &edge_ids,
    );
    true
}

/// Lowers every face loop to boundary curves, pcurves, and intersection contexts,
/// or returns `None` when any binding fails admission.
#[allow(clippy::question_mark)]
fn plan_e5_boundary(
    topology: &crate::families::e5::graph::E5Topology,
    surface_for_ref: &HashMap<u32, (SurfaceId, &crate::families::e5::records::E5Surface)>,
    point_for_ref: &HashMap<u32, Point3>,
) -> Option<E5BoundaryPlan> {
    let mut pcurve_plan = BTreeMap::<u32, (PcurveGeometry, [f64; 2])>::new();
    let mut edge_curve_plan = BTreeMap::<u32, (CurveGeometry, [f64; 2])>::new();
    let mut surface_curve_plan = BTreeMap::<u32, (SurfaceId, PcurveGeometry, [f64; 2])>::new();
    let mut occurrence_intersection_sides =
        BTreeMap::<u32, Vec<(SurfaceId, PcurveGeometry, [f64; 2])>>::new();
    for face in &topology.faces {
        let Some((_, decoded_surface)) = surface_for_ref.get(&face.surface) else {
            return None;
        };
        for loop_ in &face.loops {
            if loop_.resolved_members().is_none() {
                return None;
            }
            for (&pcurve_ref, &edge_ref) in loop_.pcurves.iter().zip(&loop_.edge_uses) {
                let Some(edge) = topology.edges.get(&edge_ref) else {
                    return None;
                };
                let Some(support) = topology.curve_supports.get(&edge.support) else {
                    return None;
                };
                let _ = support;
                let Some(pcurve) = topology.pcurves.get(&pcurve_ref) else {
                    return None;
                };
                let Some((geometry, range, endpoints)) =
                    e5_pcurve_on_surface(pcurve, decoded_surface)
                else {
                    return None;
                };
                let (Some(start), Some(end)) = (
                    point_for_ref.get(&edge.start_vertex),
                    point_for_ref.get(&edge.end_vertex),
                ) else {
                    return None;
                };
                let forward = endpoints[0]
                    .distance(*start)
                    .max(endpoints[1].distance(*end));
                let reverse_error = endpoints[0]
                    .distance(*end)
                    .max(endpoints[1].distance(*start));
                let reversed = e5_stored_pcurve_reversed(topology, edge_ref, pcurve_ref, range)
                    .or_else(|| {
                        ((forward - reverse_error).abs() > 1e-9).then_some(reverse_error < forward)
                    });
                let Some(reversed) = reversed else {
                    return None;
                };
                if if reversed { reverse_error } else { forward } > 2e-3 {
                    return None;
                }
                let oriented_pcurve = if reversed {
                    let Some(reversed) = reverse_e5_pcurve_geometry(&geometry, range) else {
                        return None;
                    };
                    reversed
                } else {
                    geometry.clone()
                };
                if support.intersection {
                    let side = (
                        surface_for_ref[&face.surface].0.clone(),
                        oriented_pcurve.clone(),
                        range,
                    );
                    let sides = occurrence_intersection_sides.entry(edge_ref).or_default();
                    if !sides.contains(&side) {
                        sides.push(side);
                    }
                }
                if let Some((mut curve, mut curve_range)) = e5_boundary_curve(
                    &decoded_surface.geometry,
                    pcurve,
                    &geometry,
                    range,
                    endpoints,
                ) {
                    if reversed {
                        let Some(reversed_curve) = reverse_e5_boundary_curve(&curve, curve_range)
                        else {
                            return None;
                        };
                        (curve, curve_range) = reversed_curve;
                    }
                    if !support.intersection {
                        if let Some(existing) = edge_curve_plan.get(&edge_ref) {
                            if existing != &(curve, curve_range) {
                                return None;
                            }
                        } else {
                            edge_curve_plan.insert(edge_ref, (curve, curve_range));
                        }
                    }
                } else if !support.intersection {
                    surface_curve_plan.entry(edge_ref).or_insert_with(|| {
                        (
                            surface_for_ref[&face.surface].0.clone(),
                            oriented_pcurve,
                            range,
                        )
                    });
                }
                if let Some((existing, existing_range)) = pcurve_plan.get(&pcurve_ref) {
                    if existing != &geometry || existing_range != &range {
                        return None;
                    }
                } else {
                    pcurve_plan.insert(pcurve_ref, (geometry, range));
                }
            }
        }
    }
    let mut intersection_sides = BTreeMap::<u32, BTreeMap<u32, E5IntersectionSidePlan>>::new();
    for (&edge_ref, edge) in &topology.edges {
        let Some(support) = topology.curve_supports.get(&edge.support) else {
            return None;
        };
        if !support.intersection {
            continue;
        }
        let (Some(start), Some(end)) = (
            point_for_ref.get(&edge.start_vertex),
            point_for_ref.get(&edge.end_vertex),
        ) else {
            return None;
        };
        for pcurve_ref in &support.pcurves {
            let Some(pcurve) = topology.pcurves.get(pcurve_ref) else {
                continue;
            };
            let surface_ref = match pcurve {
                crate::families::e5::graph::E5Pcurve::Line { surface, .. }
                | crate::families::e5::graph::E5Pcurve::Circle { surface, .. }
                | crate::families::e5::graph::E5Pcurve::Jet { surface, .. } => *surface,
            };
            let Some((surface_id, decoded_surface)) = surface_for_ref.get(&surface_ref) else {
                continue;
            };
            let Some((geometry, range, endpoints)) = e5_pcurve_on_surface(pcurve, decoded_surface)
            else {
                continue;
            };
            let forward = endpoints[0]
                .distance(*start)
                .max(endpoints[1].distance(*end));
            let reverse_error = endpoints[0]
                .distance(*end)
                .max(endpoints[1].distance(*start));
            let reversed = e5_stored_pcurve_reversed(topology, edge_ref, *pcurve_ref, range)
                .or_else(|| {
                    ((forward - reverse_error).abs() > 1e-9).then_some(reverse_error < forward)
                });
            let Some(reversed) = reversed else {
                continue;
            };
            if if reversed { reverse_error } else { forward } > 2e-3 {
                continue;
            }
            let Some((mut curve, mut curve_range)) = e5_boundary_curve(
                &decoded_surface.geometry,
                pcurve,
                &geometry,
                range,
                endpoints,
            ) else {
                continue;
            };
            if reversed {
                let Some(reversed_curve) = reverse_e5_boundary_curve(&curve, curve_range) else {
                    continue;
                };
                (curve, curve_range) = reversed_curve;
            }
            let pcurve = if reversed {
                let Some(reversed) = reverse_e5_pcurve_geometry(&geometry, range) else {
                    continue;
                };
                reversed
            } else {
                geometry
            };
            intersection_sides.entry(edge_ref).or_default().insert(
                *pcurve_ref,
                E5IntersectionSidePlan {
                    surface: surface_id.clone(),
                    pcurve,
                    pcurve_range: range,
                    curve,
                    curve_range,
                },
            );
        }
    }

    let mut intersection_plan = BTreeMap::<u32, IntcurveSupportContext>::new();
    for (&edge_ref, sides) in &intersection_sides {
        let Some(edge) = topology.edges.get(&edge_ref) else {
            return None;
        };
        let Some(support) = topology.curve_supports.get(&edge.support) else {
            return None;
        };
        let [left_ref, right_ref] = support.pcurves.as_slice() else {
            continue;
        };
        let (Some(left), Some(right)) = (sides.get(left_ref), sides.get(right_ref)) else {
            continue;
        };
        if !equivalent_e5_curve_carriers(&left.curve, &right.curve)
            || ((left.curve_range[1] - left.curve_range[0])
                - (right.curve_range[1] - right.curve_range[0]))
                .abs()
                > 1e-9
        {
            continue;
        }
        edge_curve_plan.insert(edge_ref, (left.curve.clone(), left.curve_range));
        intersection_plan.insert(
            edge_ref,
            IntcurveSupportContext {
                sides: [left, right].map(|side| IntcurveSupportSide {
                    surface: Some(side.surface.clone()),
                    pcurve: Some(side.pcurve.clone()),
                    pcurve_parameter_range: Some(side.pcurve_range),
                }),
                parameter_range: left.curve_range,
                discontinuities: std::array::from_fn(|_| Vec::new()),
            },
        );
    }
    for (&edge_ref, sides) in &occurrence_intersection_sides {
        if intersection_plan.contains_key(&edge_ref) {
            continue;
        }
        let Some(context) = e5_occurrence_intersection_context(sides) else {
            if let Some(side) = sides.first() {
                surface_curve_plan
                    .entry(edge_ref)
                    .or_insert_with(|| side.clone());
            }
            continue;
        };
        edge_curve_plan.insert(
            edge_ref,
            (
                CurveGeometry::Unknown { record: None },
                context.parameter_range,
            ),
        );
        intersection_plan.insert(edge_ref, context);
    }

    for (&edge_ref, (_, _, range)) in &surface_curve_plan {
        edge_curve_plan
            .entry(edge_ref)
            .or_insert((CurveGeometry::Unknown { record: None }, *range));
    }
    Some(E5BoundaryPlan {
        pcurve_plan,
        edge_curve_plan,
        surface_curve_plan,
        intersection_plan,
    })
}

/// Drops surfaces no face, intersection side, or surface curve references.
fn prune_e5_unused_surfaces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::families::e5::graph::E5Topology,
    surface_for_ref: &HashMap<u32, (SurfaceId, &crate::families::e5::records::E5Surface)>,
    intersection_plan: &BTreeMap<u32, IntcurveSupportContext>,
    surface_curve_plan: &BTreeMap<u32, (SurfaceId, PcurveGeometry, [f64; 2])>,
) {
    let used_surfaces = topology
        .faces
        .iter()
        .filter_map(|face| surface_for_ref.get(&face.surface))
        .map(|(id, _)| id.clone())
        .chain(
            intersection_plan
                .values()
                .flat_map(|context| context.sides.iter().filter_map(|side| side.surface.clone())),
        )
        .chain(
            surface_curve_plan
                .values()
                .map(|(surface, _, _)| surface.clone()),
        )
        .collect::<HashSet<_>>();
    let unused_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| !used_surfaces.contains(&surface.id))
        .map(|surface| surface.id.clone())
        .collect::<Vec<_>>();
    ir.model
        .surfaces
        .retain(|surface| used_surfaces.contains(&surface.id));
    for surface in unused_surfaces {
        annotations.remove_entity(surface);
    }
}

/// Resolves body face groupings into region/shell components, or `None` on failure.
#[allow(clippy::question_mark)]
fn resolve_e5_ownership(topology: &crate::families::e5::graph::E5Topology) -> Option<E5Ownership> {
    let body_faces: Vec<(Option<u32>, Vec<u32>)> = if topology.bodies.is_empty() {
        vec![(
            None,
            topology.faces.iter().map(|face| face.record_id).collect(),
        )]
    } else {
        topology
            .bodies
            .iter()
            .map(|body| (Some(body.record_id), body.faces.clone()))
            .collect()
    };
    let Some(ownership) = e5_ownership_plan(topology, &body_faces) else {
        return None;
    };
    let mut face_shell = HashMap::new();
    for (body, plan) in ownership.iter().enumerate() {
        for (component, faces) in plan.components.iter().enumerate() {
            let shell = ShellId(format!("catia:e5:shell#{body}-{component}"));
            for face in faces {
                face_shell.insert(*face, shell.clone());
            }
        }
    }
    Some(E5Ownership {
        body_faces,
        ownership,
        face_shell,
    })
}

/// Emits the boundary curve, intersection/surface-curve procedural, and edge layers.
#[allow(clippy::too_many_arguments)]
fn emit_e5_curves_and_edges(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::families::e5::graph::E5Topology,
    vertex_for_ref: &HashMap<u32, VertexId>,
    edge_ids: &HashMap<u32, EdgeId>,
    edge_curve_plan: &BTreeMap<u32, (CurveGeometry, [f64; 2])>,
    intersection_plan: &BTreeMap<u32, IntcurveSupportContext>,
    surface_curve_plan: &BTreeMap<u32, (SurfaceId, PcurveGeometry, [f64; 2])>,
) {
    let edge_curve_ids: HashMap<u32, CurveId> = edge_curve_plan
        .keys()
        .map(|&record_id| (record_id, CurveId(format!("catia:e5:curve#{record_id}"))))
        .collect();
    for (&record_id, (geometry, _)) in edge_curve_plan {
        let id = edge_curve_ids[&record_id].clone();
        annotate(
            annotations,
            &id,
            "e5_0d_03",
            0,
            "lifted_boundary_curve",
            Exactness::Derived,
        );
        annotations.derived(&id, "geometry");
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: geometry.clone(),
            source_object: None,
        });
    }
    for (&record_id, context) in intersection_plan {
        let curve = edge_curve_ids[&record_id].clone();
        let id = ProceduralCurveId(format!("catia:e5:intersection#{record_id}"));
        annotate(
            annotations,
            &id,
            "e5_0d_03",
            0,
            "c1_surface_intersection",
            Exactness::Derived,
        );
        annotations.derived(&id, "curve").derived(&id, "definition");
        ir.model.procedural_curves.push(ProceduralCurve {
            id,
            curve,
            definition: ProceduralCurveDefinition::Intersection {
                context: context.clone(),
                discontinuity_flag: false,
            },
            cache_fit_tolerance: None,
        });
    }
    for (&record_id, (surface, pcurve, range)) in surface_curve_plan {
        if intersection_plan.contains_key(&record_id) {
            continue;
        }
        let curve = edge_curve_ids[&record_id].clone();
        let id = ProceduralCurveId(format!("catia:e5:surface-curve#{record_id}"));
        annotate(
            annotations,
            &id,
            "e5_0d_03",
            0,
            "parametric_surface_curve",
            Exactness::Derived,
        );
        annotations.derived(&id, "curve").derived(&id, "definition");
        ir.model.procedural_curves.push(ProceduralCurve {
            id,
            curve,
            definition: ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric,
                context: IntcurveSupportContext {
                    sides: [
                        IntcurveSupportSide {
                            surface: Some(surface.clone()),
                            pcurve: Some(pcurve.clone()),
                            pcurve_parameter_range: None,
                        },
                        IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                            pcurve_parameter_range: None,
                        },
                    ],
                    parameter_range: *range,
                    discontinuities: std::array::from_fn(|_| Vec::new()),
                },
            },
            cache_fit_tolerance: None,
        });
    }
    for (&record_id, edge) in &topology.edges {
        let id = edge_ids[&record_id].clone();
        annotate(
            annotations,
            &id,
            "e5_0d_03",
            0,
            "ff_edge_use",
            Exactness::ByteExact,
        );
        for field in ["start", "end"] {
            annotations.derived(&id, field);
        }
        if edge_curve_ids.contains_key(&record_id) {
            annotations
                .derived(&id, "curve")
                .derived(&id, "param_range");
        }
        ir.model.edges.push(Edge {
            id,
            curve: edge_curve_ids.get(&record_id).cloned(),
            start: vertex_for_ref[&edge.start_vertex].clone(),
            end: vertex_for_ref[&edge.end_vertex].clone(),
            param_range: edge_curve_plan.get(&record_id).map(|(_, range)| *range),
            tolerance: None,
        });
    }
}

/// Emits the surface pcurve layer.
fn emit_e5_pcurves(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    pcurve_plan: BTreeMap<u32, (PcurveGeometry, [f64; 2])>,
) {
    for (record_id, (geometry, range)) in pcurve_plan {
        let id = PcurveId(format!("catia:e5:pcurve#{record_id}"));
        annotate(
            annotations,
            &id,
            "e5_0d_03",
            0,
            "surface_parameter_curve",
            Exactness::ByteExact,
        );
        annotations.derived(&id, "geometry");
        ir.model.pcurves.push(Pcurve {
            id,
            geometry,
            wrapper_reversed: None,
            parameter_range: Some(range),
            fit_tolerance: None,
            native_tail_flags: None,
        });
    }
}

/// Emits the body/region/shell layer.
fn emit_e5_bodies(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    body_faces: &[(Option<u32>, Vec<u32>)],
    ownership: &[E5BodyOwnership],
) {
    for (body_index, (record_id, _)) in body_faces.iter().enumerate() {
        let body_id = BodyId(record_id.map_or_else(
            || format!("catia:e5:body#inferred-{body_index}"),
            |id| format!("catia:e5:body#{id}"),
        ));
        let plan = &ownership[body_index];
        let region_ids: Vec<RegionId> = (0..plan.components.len())
            .map(|component| RegionId(format!("catia:e5:region#{body_index}-{component}")))
            .collect();
        annotate(
            annotations,
            &body_id,
            "e5_0d_03",
            0,
            "01_body",
            if record_id.is_some() {
                Exactness::ByteExact
            } else {
                Exactness::Inferred
            },
        );
        annotations
            .derived(&body_id, "kind")
            .derived(&body_id, "regions");
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: plan.kind,
            regions: region_ids.clone(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        for (component, component_faces) in plan.components.iter().enumerate() {
            let region_id = region_ids[component].clone();
            let shell_id = ShellId(format!("catia:e5:shell#{body_index}-{component}"));
            annotate(
                annotations,
                &region_id,
                "e5_0d_03",
                0,
                "derived_region",
                Exactness::Inferred,
            );
            annotations
                .derived(&region_id, "body")
                .derived(&region_id, "shells");
            ir.model.regions.push(Region {
                id: region_id.clone(),
                body: body_id.clone(),
                shells: vec![shell_id.clone()],
            });
            annotate(
                annotations,
                &shell_id,
                "e5_0d_03",
                0,
                "derived_shell",
                Exactness::Inferred,
            );
            annotations
                .derived(&shell_id, "region")
                .derived(&shell_id, "faces");
            ir.model.shells.push(Shell {
                id: shell_id,
                region: region_id,
                faces: component_faces
                    .iter()
                    .map(|face| FaceId(format!("catia:e5:face#{face}")))
                    .collect(),
                wire_edges: Vec::new(),
                free_vertices: Vec::new(),
            });
        }
    }
}

/// Emits the face/loop/coedge layer and the radial-next fixup.
fn emit_e5_faces_loops_coedges(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::families::e5::graph::E5Topology,
    surface_for_ref: &HashMap<u32, (SurfaceId, &crate::families::e5::records::E5Surface)>,
    face_shell: &HashMap<u32, ShellId>,
    edge_ids: &HashMap<u32, EdgeId>,
) {
    let mut coedges_by_edge = HashMap::<u32, Vec<usize>>::new();
    for face in &topology.faces {
        let face_id = FaceId(format!("catia:e5:face#{}", face.record_id));
        let loop_ids: Vec<LoopId> = face
            .loops
            .iter()
            .map(|loop_| LoopId(format!("catia:e5:loop#{}", loop_.record_id)))
            .collect();
        annotate(
            annotations,
            &face_id,
            "e5_0d_03",
            0,
            "00_advanced_face",
            Exactness::ByteExact,
        );
        for field in ["shell", "surface", "sense", "loops"] {
            annotations.derived(&face_id, field);
        }
        ir.model.faces.push(Face {
            id: face_id.clone(),
            shell: face_shell[&face.record_id].clone(),
            surface: surface_for_ref[&face.surface].0.clone(),
            sense: if face.trailer_sign > 0 {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops: loop_ids,
            name: None,
            color: None,
            tolerance: None,
        });

        for loop_ in &face.loops {
            let loop_id = LoopId(format!("catia:e5:loop#{}", loop_.record_id));
            let coedge_ids_by_member: Vec<CoedgeId> = (0..loop_.edge_uses.len())
                .map(|index| CoedgeId(format!("catia:e5:coedge#{}-{index}", loop_.record_id)))
                .collect();
            let members = loop_
                .resolved_members()
                .expect("E5 loop membership passed topology admission");
            let coedge_ids: Vec<CoedgeId> = members
                .iter()
                .map(|member| coedge_ids_by_member[member.serialized_index].clone())
                .collect();
            annotate(
                annotations,
                &loop_id,
                "e5_0d_03",
                0,
                "09_loop",
                Exactness::ByteExact,
            );
            annotations
                .derived(&loop_id, "face")
                .derived(&loop_id, "coedges");
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges: coedge_ids.clone(),
                vertex_uses: Vec::new(),
            });
            for (position, member) in members.iter().enumerate() {
                let index = member.serialized_index;
                let edge_ref = loop_.edge_uses[index];
                let pcurve_ref = loop_.pcurves[index];
                let id = coedge_ids_by_member[index].clone();
                annotate(
                    annotations,
                    &id,
                    "e5_0d_03",
                    0,
                    "serialized_loop_member",
                    Exactness::ByteExact,
                );
                for field in ["owner_loop", "edge", "next", "previous", "sense", "pcurves"] {
                    annotations.derived(&id, field);
                }
                let arena_index = ir.model.coedges.len();
                coedges_by_edge
                    .entry(edge_ref)
                    .or_default()
                    .push(arena_index);
                ir.model.coedges.push(Coedge {
                    id: id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_ids[&edge_ref].clone(),
                    next: coedge_ids[(position + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(position + coedge_ids.len() - 1) % coedge_ids.len()]
                        .clone(),
                    radial_next: id,
                    sense: if member.reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurves: vec![cadmpeg_ir::topology::PcurveUse {
                        pcurve: PcurveId(format!("catia:e5:pcurve#{pcurve_ref}")),
                        isoparametric: None,
                    }],
                });
            }
        }
    }
    for occurrences in coedges_by_edge.values() {
        for (position, &arena_index) in occurrences.iter().enumerate() {
            let radial = occurrences[(position + 1) % occurrences.len()];
            ir.model.coedges[arena_index].radial_next = ir.model.coedges[radial].id.clone();
        }
    }
}

pub(crate) fn e5_stored_pcurve_reversed(
    topology: &crate::families::e5::graph::E5Topology,
    edge_ref: u32,
    pcurve_ref: u32,
    native_range: [f64; 2],
) -> Option<bool> {
    let parameters = topology.edge_representation_parameters(edge_ref, pcurve_ref)?;
    parameter_ranges_reversed(parameters, native_range)
}

pub(crate) fn parameter_ranges_reversed(
    parameters: [f64; 2],
    native_range: [f64; 2],
) -> Option<bool> {
    let bound_span = parameters[1] - parameters[0];
    let native_span = native_range[1] - native_range[0];
    (bound_span.abs() > f64::EPSILON && native_span.abs() > f64::EPSILON)
        .then_some(bound_span.is_sign_negative() != native_span.is_sign_negative())
}

pub(crate) fn e5_pcurve_on_surface(
    pcurve: &crate::families::e5::graph::E5Pcurve,
    decoded_surface: &crate::families::e5::records::E5Surface,
) -> Option<(PcurveGeometry, [f64; 2], [Point3; 2])> {
    let surface = &decoded_surface.geometry;
    match pcurve {
        crate::families::e5::graph::E5Pcurve::Line {
            origin: raw_origin,
            direction,
            range,
            ..
        } => {
            let origin = e5_surface_uv(decoded_surface, *raw_origin);
            let direction = Point2::new(
                direction[0] * decoded_surface.uv_scale[0],
                direction[1] * decoded_surface.uv_scale[1],
            );
            let uv = range.map(|parameter| {
                Point2::new(
                    origin.u + parameter * direction.u,
                    origin.v + parameter * direction.v,
                )
            });
            Some((
                PcurveGeometry::Line { origin, direction },
                *range,
                uv.map(|point| cadmpeg_ir::eval::surface_point(surface, point.u, point.v))
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?,
            ))
        }
        crate::families::e5::graph::E5Pcurve::Circle {
            center,
            radius,
            range,
            ..
        } => {
            let angular_range = ordered_range([range[0] / radius, range[1] / radius]);
            let geometry = rational_pcurve_arc(*center, *radius, angular_range)?;
            let PcurveGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                periodic,
            } = geometry
            else {
                return None;
            };
            let scale = decoded_surface.uv_scale;
            let geometry = PcurveGeometry::Nurbs {
                degree,
                knots,
                control_points: control_points
                    .into_iter()
                    .map(|point| Point2::new(point.u * scale[0], point.v * scale[1]))
                    .collect(),
                weights,
                periodic,
            };
            let endpoints = angular_range.map(|angle| {
                cadmpeg_ir::eval::surface_point(
                    surface,
                    (center[0] + radius * angle.cos()) * scale[0],
                    (center[1] + radius * angle.sin()) * scale[1],
                )
            });
            Some((
                geometry,
                angular_range,
                endpoints
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?,
            ))
        }
        crate::families::e5::graph::E5Pcurve::Jet {
            degree,
            knots,
            points,
            first_derivatives,
            second_derivatives,
            range,
            ..
        } => {
            let scale = decoded_surface.uv_scale;
            let points = points
                .iter()
                .map(|point| [point[0] * scale[0], point[1] * scale[1]])
                .collect::<Vec<_>>();
            let first_derivatives = first_derivatives
                .iter()
                .map(|value| [value[0] * scale[0], value[1] * scale[1]])
                .collect::<Vec<_>>();
            let second_derivatives = second_derivatives
                .iter()
                .map(|value| [value[0] * scale[0], value[1] * scale[1]])
                .collect::<Vec<_>>();
            let geometry = quintic_jet_pcurve(
                *degree,
                knots,
                &points,
                &first_derivatives,
                &second_derivatives,
            )?;
            let endpoints = [*points.first()?, *points.last()?]
                .map(|uv| cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1]));
            Some((
                geometry,
                *range,
                endpoints
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?,
            ))
        }
    }
}

pub(crate) fn e5_boundary_curve(
    surface: &SurfaceGeometry,
    native_pcurve: &crate::families::e5::graph::E5Pcurve,
    pcurve: &PcurveGeometry,
    range: [f64; 2],
    endpoints: [Point3; 2],
) -> Option<(CurveGeometry, [f64; 2])> {
    if let (
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        },
        crate::families::e5::graph::E5Pcurve::Circle { center, radius, .. },
    ) = (surface, native_pcurve)
    {
        let v_axis = (*normal).cross(*u_axis);
        let center = (*origin)
            .translated(*u_axis, center[0])
            .translated(v_axis, center[1]);
        return Some((
            CurveGeometry::Circle {
                center,
                axis: *normal,
                ref_direction: *u_axis,
                radius: *radius,
            },
            crate::nurbs::canonical_periodic_range(range)?,
        ));
    }
    if let (
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        },
        crate::families::e5::graph::E5Pcurve::Jet { .. },
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        },
    ) = (surface, native_pcurve, pcurve)
    {
        let v_axis = (*normal).cross(*u_axis);
        return Some((
            CurveGeometry::Nurbs(NurbsCurve {
                degree: *degree,
                knots: knots.clone(),
                control_points: control_points
                    .iter()
                    .map(|point| {
                        (*origin)
                            .translated(*u_axis, point.u)
                            .translated(v_axis, point.v)
                    })
                    .collect(),
                weights: weights.clone(),
                periodic: *periodic,
            }),
            range,
        ));
    }
    let PcurveGeometry::Line { origin, direction } = pcurve else {
        return None;
    };
    let start_uv = Point2::new(
        origin.u + range[0] * direction.u,
        origin.v + range[0] * direction.v,
    );
    let span = range[1] - range[0];
    let span_direction = Point2::new(span * direction.u, span * direction.v);

    let circle = if direction.v.abs() <= 1e-12 && direction.u.abs() > 1e-12 {
        e5_constant_v_circle(surface, start_uv.v)
    } else if direction.u.abs() <= 1e-12 && direction.v.abs() > 1e-12 {
        e5_constant_u_circle(surface, start_uv.u)
    } else {
        None
    };
    if let Some((center, radius, axis)) = circle {
        let candidates = [axis, axis.scale(-1.0)]
            .into_iter()
            .filter_map(|axis| {
                let ref_direction = cadmpeg_ir::geometry::derive_reference_direction(axis);
                let range = circle_parameter_range_from_surface_branch(
                    surface,
                    center,
                    radius,
                    axis,
                    ref_direction,
                    endpoints[0],
                    endpoints[1],
                    start_uv,
                    span_direction,
                )?;
                Some((
                    axis,
                    ref_direction,
                    crate::nurbs::canonical_periodic_range(range)?,
                ))
            })
            .collect::<Vec<_>>();
        let [(axis, ref_direction, curve_range)] = candidates.as_slice() else {
            return None;
        };
        return Some((
            CurveGeometry::Circle {
                center,
                axis: *axis,
                ref_direction: *ref_direction,
                radius,
            },
            *curve_range,
        ));
    }

    if !(matches!(surface, SurfaceGeometry::Plane { .. })
        || (direction.u.abs() <= 1e-12
            && matches!(
                surface,
                SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. }
            )))
    {
        return None;
    }
    let delta = endpoints[1].vector_from(endpoints[0]);
    let length = delta.norm();
    (length > f64::EPSILON).then_some((
        CurveGeometry::Line {
            origin: endpoints[0],
            direction: delta.scale(1.0 / length),
        },
        [0.0, length],
    ))
}

pub(crate) fn reverse_e5_boundary_curve(
    curve: &CurveGeometry,
    range: [f64; 2],
) -> Option<(CurveGeometry, [f64; 2])> {
    match curve {
        CurveGeometry::Line { origin, direction } => {
            let length = range[1] - range[0];
            (length >= 0.0).then_some((
                CurveGeometry::Line {
                    origin: (*origin).translated(*direction, range[1]),
                    direction: (*direction).scale(-1.0),
                },
                [0.0, length],
            ))
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let sweep = range[1] - range[0];
            if sweep < 0.0 {
                return None;
            }
            let tangent = (*axis).cross(*ref_direction);
            let end = range[1];
            let ref_direction = (*ref_direction).scale(end.cos()) + tangent.scale(end.sin());
            Some((
                CurveGeometry::Circle {
                    center: *center,
                    axis: (*axis).scale(-1.0),
                    ref_direction,
                    radius: *radius,
                },
                [0.0, sweep],
            ))
        }
        CurveGeometry::Nurbs(nurbs) => {
            let first = *nurbs.knots.first()?;
            let last = *nurbs.knots.last()?;
            let mut knots = nurbs
                .knots
                .iter()
                .rev()
                .map(|knot| first + last - knot)
                .collect::<Vec<_>>();
            for knot in &mut knots {
                if knot.abs() <= 1e-15 {
                    *knot = 0.0;
                }
            }
            Some((
                CurveGeometry::Nurbs(NurbsCurve {
                    degree: nurbs.degree,
                    knots,
                    control_points: nurbs.control_points.iter().rev().copied().collect(),
                    weights: nurbs
                        .weights
                        .as_ref()
                        .map(|weights| weights.iter().rev().copied().collect()),
                    periodic: nurbs.periodic,
                }),
                range,
            ))
        }
        _ => None,
    }
}

pub(crate) fn reverse_e5_pcurve_geometry(
    geometry: &PcurveGeometry,
    range: [f64; 2],
) -> Option<PcurveGeometry> {
    match geometry {
        PcurveGeometry::Line { origin, direction } => Some(PcurveGeometry::Line {
            origin: Point2::new(
                origin.u + (range[0] + range[1]) * direction.u,
                origin.v + (range[0] + range[1]) * direction.v,
            ),
            direction: Point2::new(-direction.u, -direction.v),
        }),
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => {
            let sum = range[0] + range[1];
            let mut reversed_knots = knots
                .iter()
                .rev()
                .map(|knot| sum - knot)
                .collect::<Vec<_>>();
            for knot in &mut reversed_knots {
                if *knot == -0.0 {
                    *knot = 0.0;
                }
            }
            Some(PcurveGeometry::Nurbs {
                degree: *degree,
                knots: reversed_knots,
                control_points: control_points.iter().rev().copied().collect(),
                weights: weights
                    .as_ref()
                    .map(|weights| weights.iter().rev().copied().collect()),
                periodic: *periodic,
            })
        }
        _ => None,
    }
}

pub(crate) fn e5_occurrence_intersection_context(
    sides: &[(SurfaceId, PcurveGeometry, [f64; 2])],
) -> Option<IntcurveSupportContext> {
    let [left, right] = sides else {
        return None;
    };
    if (left.2[0] - right.2[0]).abs() > 1e-9 || (left.2[1] - right.2[1]).abs() > 1e-9 {
        return None;
    }
    Some(IntcurveSupportContext {
        sides: [left, right].map(|side| IntcurveSupportSide {
            surface: Some(side.0.clone()),
            pcurve: Some(side.1.clone()),
            pcurve_parameter_range: None,
        }),
        parameter_range: left.2,
        discontinuities: std::array::from_fn(|_| Vec::new()),
    })
}

pub(crate) fn equivalent_e5_curve_carriers(left: &CurveGeometry, right: &CurveGeometry) -> bool {
    match (left, right) {
        (
            CurveGeometry::Line {
                origin: left_origin,
                direction: left_direction,
            },
            CurveGeometry::Line {
                origin: right_origin,
                direction: right_direction,
            },
        ) => {
            (*left_origin).distance(*right_origin) <= 2e-3
                && (*left_direction).dot(*right_direction).abs() >= 1.0 - 1e-9
        }
        (
            CurveGeometry::Circle {
                center: left_center,
                axis: left_axis,
                radius: left_radius,
                ..
            },
            CurveGeometry::Circle {
                center: right_center,
                axis: right_axis,
                radius: right_radius,
                ..
            },
        ) => {
            (*left_center).distance(*right_center) <= 2e-3
                && (left_radius - right_radius).abs() <= 2e-3
                && (*left_axis).dot(*right_axis).abs() >= 1.0 - 1e-9
        }
        (CurveGeometry::Nurbs(left), CurveGeometry::Nurbs(right)) => left == right,
        _ => false,
    }
}

pub(crate) fn e5_constant_v_circle(
    surface: &SurfaceGeometry,
    v: f64,
) -> Option<(Point3, f64, Vector3)> {
    match surface {
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => Some(((*origin).translated(*axis, v), *radius, *axis)),
        SurfaceGeometry::Cone {
            origin,
            axis,
            radius,
            half_angle,
            ..
        } => Some((
            (*origin).translated(*axis, v),
            (radius + v * half_angle.tan()).abs(),
            *axis,
        )),
        SurfaceGeometry::Sphere {
            center,
            axis,
            radius,
            ..
        } => Some((
            (*center).translated(*axis, radius * v.sin()),
            radius * v.cos().abs(),
            *axis,
        )),
        SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => Some((
            (*center).translated(*axis, minor_radius * v.sin()),
            (major_radius + minor_radius * v.cos()).abs(),
            *axis,
        )),
        _ => None,
    }
}

pub(crate) fn e5_constant_u_circle(
    surface: &SurfaceGeometry,
    u: f64,
) -> Option<(Point3, f64, Vector3)> {
    match surface {
        SurfaceGeometry::Sphere {
            center,
            axis,
            radius,
            ref_direction,
        } => {
            let tangent = (*axis).cross(*ref_direction);
            let radial = (*ref_direction).scale(u.cos()) + tangent.scale(u.sin());
            Some((*center, *radius, (*axis).cross(radial)))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let tangent = (*axis).cross(*ref_direction);
            let radial = (*ref_direction).scale(u.cos()) + tangent.scale(u.sin());
            Some((
                (*center).translated(radial, *major_radius),
                *minor_radius,
                (*axis).cross(radial),
            ))
        }
        _ => None,
    }
}

pub(crate) fn e5_surface_uv(
    surface: &crate::families::e5::records::E5Surface,
    raw: [f64; 2],
) -> Point2 {
    Point2::new(raw[0] * surface.uv_scale[0], raw[1] * surface.uv_scale[1])
}

pub(crate) struct E5BodyOwnership {
    kind: BodyKind,
    components: Vec<Vec<u32>>,
}

pub(crate) fn e5_ownership_plan(
    topology: &crate::families::e5::graph::E5Topology,
    body_faces: &[(Option<u32>, Vec<u32>)],
) -> Option<Vec<E5BodyOwnership>> {
    if body_faces.is_empty() || body_faces.iter().any(|(_, faces)| faces.is_empty()) {
        return None;
    }
    let mut body_by_face = HashMap::new();
    for (body, (_, faces)) in body_faces.iter().enumerate() {
        for face in faces {
            if body_by_face.insert(*face, body).is_some() {
                return None;
            }
        }
    }
    let mut uses = vec![HashMap::<u32, usize>::new(); body_faces.len()];
    let mut bodies_by_edge = topology
        .edges
        .keys()
        .map(|edge| (*edge, HashSet::new()))
        .collect::<HashMap<_, _>>();
    for face in &topology.faces {
        let body = *body_by_face.get(&face.record_id)?;
        for edge in face.loops.iter().flat_map(|loop_| &loop_.edge_uses) {
            bodies_by_edge.get_mut(edge)?.insert(body);
            *uses[body].entry(*edge).or_default() += 1;
        }
    }
    if body_by_face.len() != topology.faces.len()
        || bodies_by_edge.values().any(|bodies| bodies.len() != 1)
    {
        return None;
    }
    body_faces
        .iter()
        .enumerate()
        .map(|(body, (_, faces))| {
            let face_indices: HashMap<u32, usize> = faces
                .iter()
                .copied()
                .enumerate()
                .map(|(index, face)| (face, index))
                .collect();
            if face_indices.len() != faces.len() {
                return None;
            }
            let mut parents = UnionFind::new(faces.len());
            let mut first_face_by_edge = HashMap::<u32, usize>::new();
            for face in topology
                .faces
                .iter()
                .filter(|face| body_by_face[&face.record_id] == body)
            {
                let face_index = face_indices[&face.record_id];
                for edge in face.loops.iter().flat_map(|loop_| &loop_.edge_uses) {
                    if let Some(other) = first_face_by_edge.insert(*edge, face_index) {
                        parents.union(face_index, other);
                    }
                }
            }
            let mut labels = HashMap::<usize, usize>::new();
            let mut components = Vec::<Vec<u32>>::new();
            let mut face_components = Vec::with_capacity(faces.len());
            for (face_index, face) in faces.iter().copied().enumerate() {
                let root = parents.find(face_index);
                let next = labels.len();
                let component = *labels.entry(root).or_insert(next);
                face_components.push(component);
                if component == components.len() {
                    components.push(Vec::new());
                }
                components[component].push(face);
            }
            let body_uses = &uses[body];
            let mut closed_components = vec![true; components.len()];
            let mut component_has_edges = vec![false; components.len()];
            for (&edge, &count) in body_uses {
                let component = face_components[first_face_by_edge[&edge]];
                component_has_edges[component] = true;
                closed_components[component] &= count == 2;
            }
            let closed_component_count = closed_components
                .iter()
                .zip(component_has_edges)
                .filter(|(closed, has_edges)| **closed && *has_edges)
                .count();
            let kind = if body_uses.values().any(|count| *count > 2)
                || (closed_component_count != 0 && closed_component_count != components.len())
            {
                BodyKind::General
            } else if closed_component_count == components.len() && !components.is_empty() {
                BodyKind::Solid
            } else {
                BodyKind::Sheet
            };
            Some(E5BodyOwnership { kind, components })
        })
        .collect()
}

#[cfg(test)]
mod route_tests {
    use crate::assemble::{quintic_jet_pcurve, rational_pcurve_arc};
    use crate::families::e5::decode::{
        e5_boundary_curve, e5_occurrence_intersection_context, e5_ownership_plan,
        e5_pcurve_on_surface, equivalent_e5_curve_carriers, fit_rank_one_e5_plane_axes,
        parameter_ranges_reversed,
    };

    use crate::families::e5::graph::{E5Edge, E5Face, E5Loop, E5Topology};

    use cadmpeg_ir::eval::pcurve_uv;
    use cadmpeg_ir::geometry::{CurveGeometry, PcurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::BodyKind;

    use std::collections::BTreeMap;

    #[test]
    fn affine_bound_parameters_preserve_or_reverse_native_direction() {
        assert_eq!(
            parameter_ranges_reversed([0.0, 13.0], [0.0, 122.0]),
            Some(false)
        );
        assert_eq!(
            parameter_ranges_reversed([13.0, 0.0], [0.0, 122.0]),
            Some(true)
        );
        assert_eq!(parameter_ranges_reversed([1.0, 1.0], [0.0, 1.0]), None);
    }

    #[test]
    fn e5_ownership_requires_complete_bodies_and_partitions_face_components() {
        let face = |record_id, edge_use| E5Face {
            record_id,
            surface: 100 + record_id,
            trailer_sign: 1,
            loops: vec![E5Loop {
                record_id: 200 + record_id,
                surface: 100 + record_id,
                pcurves: vec![300 + record_id],
                edge_uses: vec![edge_use],
                reversed: vec![false],
                oriented_members: Some(vec![crate::families::e5::graph::E5OrientedMember {
                    serialized_index: 0,
                    reversed: false,
                }]),
                outer: Some(true),
                orientation_signs: Vec::new(),
            }],
        };
        let edge = |record_id| E5Edge {
            record_id,
            support: 20,
            start_vertex: 30,
            end_vertex: 31,
            parameter_start: 40,
            parameter_end: 41,
            tail: Vec::new(),
        };
        let topology = |faces: Vec<E5Face>, edges: Vec<u32>| E5Topology {
            bodies: Vec::new(),
            faces,
            edges: edges
                .into_iter()
                .map(|record_id| (record_id, edge(record_id)))
                .collect(),
            pcurves: BTreeMap::new(),
            bounds: BTreeMap::new(),
            curve_supports: BTreeMap::new(),
            vertex_refs: Vec::new(),
        };

        let plan =
            e5_ownership_plan(&topology(vec![face(1, 10)], vec![10]), &[(None, vec![1])]).unwrap();
        assert_eq!(plan[0].kind, BodyKind::Sheet);
        assert_eq!(plan[0].components, vec![vec![1]]);

        let plan = e5_ownership_plan(
            &topology(vec![face(1, 10), face(2, 10)], vec![10]),
            &[(None, vec![1, 2])],
        )
        .unwrap();
        assert_eq!(plan[0].kind, BodyKind::Solid);
        assert_eq!(plan[0].components, vec![vec![1, 2]]);

        let plan = e5_ownership_plan(
            &topology(vec![face(1, 10), face(2, 11)], vec![10, 11]),
            &[(None, vec![1, 2])],
        )
        .unwrap();
        assert_eq!(plan[0].kind, BodyKind::Sheet);
        assert_eq!(plan[0].components, vec![vec![1], vec![2]]);

        let plan = e5_ownership_plan(
            &topology(vec![face(1, 10), face(2, 10), face(3, 11)], vec![10, 11]),
            &[(None, vec![1, 2, 3])],
        )
        .unwrap();
        assert_eq!(plan[0].kind, BodyKind::General);
        assert_eq!(plan[0].components, vec![vec![1, 2], vec![3]]);

        assert!(e5_ownership_plan(
            &topology(vec![face(1, 10), face(2, 10)], vec![10]),
            &[(Some(1), vec![1]), (Some(2), vec![2])],
        )
        .is_none());
        assert!(e5_ownership_plan(
            &topology(vec![face(1, 10)], vec![10, 11]),
            &[(None, vec![1])],
        )
        .is_none());
        assert!(e5_ownership_plan(&topology(Vec::new(), Vec::new()), &[]).is_none());
    }

    #[test]
    fn rational_arc_preserves_angular_parameterization() {
        let arc =
            rational_pcurve_arc([2.0, -3.0], 4.0, [0.0, std::f64::consts::PI]).expect("semicircle");
        for (parameter, expected) in [
            (0.0, [6.0, -3.0]),
            (std::f64::consts::FRAC_PI_2, [2.0, 1.0]),
            (std::f64::consts::PI, [-2.0, -3.0]),
        ] {
            let point = pcurve_uv(&arc, parameter).expect("arc evaluation");
            assert!((point.u - expected[0]).abs() < 1e-12);
            assert!((point.v - expected[1]).abs() < 1e-12);
        }
    }

    #[test]
    fn rational_arc_rejects_unbounded_subdivision_counts() {
        assert!(rational_pcurve_arc([0.0, 0.0], 1.0, [0.0, 1.0e300]).is_none());
    }

    #[test]
    fn e5_cylinder_isoparametric_boundary_lifts_to_circle_carrier() {
        let surface = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let pcurve = PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(0.0, 3.0),
            direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        };
        let native = crate::families::e5::graph::E5Pcurve::Line {
            surface: 0,
            origin: [0.0, 3.0],
            direction: [1.0, 0.0],
            range: [0.0, std::f64::consts::FRAC_PI_2],
        };
        let (curve, range) = e5_boundary_curve(
            &surface,
            &native,
            &pcurve,
            [0.0, std::f64::consts::FRAC_PI_2],
            [Point3::new(2.0, 0.0, 3.0), Point3::new(0.0, 2.0, 3.0)],
        )
        .expect("cylinder boundary circle");
        assert!(matches!(
            curve,
            CurveGeometry::Circle {
                center,
                radius,
                ..
            } if center == Point3::new(0.0, 0.0, 3.0) && radius == 2.0
        ));
        assert!((range[1] - range[0] - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn e5_plane_circle_boundary_lifts_to_world_circle_carrier() {
        let surface = SurfaceGeometry::Plane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let native = crate::families::e5::graph::E5Pcurve::Circle {
            surface: 0,
            center: [4.0, 5.0],
            codes: [0, 0],
            radius: 2.0,
            range: [0.0, std::f64::consts::PI],
            tail: [0.0, 0.0],
        };
        let pcurve = rational_pcurve_arc([4.0, 5.0], 2.0, [0.0, std::f64::consts::FRAC_PI_2])
            .expect("plane pcurve");
        let (curve, range) = e5_boundary_curve(
            &surface,
            &native,
            &pcurve,
            [0.0, std::f64::consts::FRAC_PI_2],
            [Point3::new(7.0, 7.0, 3.0), Point3::new(5.0, 9.0, 3.0)],
        )
        .expect("plane boundary circle");
        assert_eq!(range, [0.0, std::f64::consts::FRAC_PI_2]);
        assert!(matches!(
            curve,
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } if center == Point3::new(5.0, 7.0, 3.0)
                && axis == Vector3::new(0.0, 0.0, 1.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 2.0
        ));
    }

    #[test]
    fn e5_plane_jet_boundary_lifts_control_net_affinely() {
        let surface = SurfaceGeometry::Plane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let points = vec![[0.0, 0.0], [1.0, 2.0]];
        let first = vec![[1.0, 2.0], [1.0, 2.0]];
        let second = vec![[0.0, 0.0], [0.0, 0.0]];
        let native = crate::families::e5::graph::E5Pcurve::Jet {
            surface: 0,
            degree: 5,
            knots: vec![0.0, 1.0],
            multiplicities: vec![6, 6],
            points: points.clone(),
            first_derivatives: first.clone(),
            second_derivatives: second.clone(),
            range: [0.0, 1.0],
        };
        let pcurve =
            quintic_jet_pcurve(5, &[0.0, 1.0], &points, &first, &second).expect("quintic pcurve");
        let (curve, range) = e5_boundary_curve(
            &surface,
            &native,
            &pcurve,
            [0.0, 1.0],
            [Point3::new(1.0, 2.0, 3.0), Point3::new(2.0, 4.0, 3.0)],
        )
        .expect("plane jet curve");
        assert_eq!(range, [0.0, 1.0]);
        let CurveGeometry::Nurbs(nurbs) = curve else {
            panic!("expected NURBS curve");
        };
        assert_eq!(
            nurbs.control_points.first(),
            Some(&Point3::new(1.0, 2.0, 3.0))
        );
        assert_eq!(
            nurbs.control_points.last(),
            Some(&Point3::new(2.0, 4.0, 3.0))
        );
    }

    #[test]
    fn e5_intersection_requires_equivalent_two_sided_carriers() {
        let left = CurveGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        };
        let right = CurveGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, -1.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 4.0,
        };
        assert!(equivalent_e5_curve_carriers(&left, &right));
        let displaced = CurveGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.01),
            axis: Vector3::new(0.0, 0.0, -1.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 4.0,
        };
        assert!(!equivalent_e5_curve_carriers(&left, &displaced));
    }

    #[test]
    fn e5_nonplanar_jet_normalizes_positions_and_derivatives() {
        let surface = crate::families::e5::records::E5Surface {
            pos: 0,
            record_id: 7,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            uv_scale: [0.5, 1.0],
        };
        let pcurve = crate::families::e5::graph::E5Pcurve::Jet {
            surface: 7,
            degree: 5,
            knots: vec![0.0, 1.0],
            multiplicities: vec![6, 6],
            points: vec![[0.0, 3.0], [std::f64::consts::PI, 3.0]],
            first_derivatives: vec![[std::f64::consts::PI, 0.0], [std::f64::consts::PI, 0.0]],
            second_derivatives: vec![[0.0, 0.0], [0.0, 0.0]],
            range: [0.0, 1.0],
        };
        let (geometry, range, endpoints) =
            e5_pcurve_on_surface(&pcurve, &surface).expect("normalized cylinder jet");
        assert_eq!(range, [0.0, 1.0]);
        let PcurveGeometry::Nurbs { control_points, .. } = geometry else {
            panic!("expected NURBS pcurve");
        };
        assert_eq!(
            control_points.first(),
            Some(&cadmpeg_ir::math::Point2::new(0.0, 3.0))
        );
        assert_eq!(
            control_points.last(),
            Some(&cadmpeg_ir::math::Point2::new(
                std::f64::consts::FRAC_PI_2,
                3.0
            ))
        );
        assert!(endpoints[0].distance(Point3::new(2.0, 0.0, 3.0)) < 1e-12);
        assert!(endpoints[1].distance(Point3::new(0.0, 2.0, 3.0)) < 1e-12);
    }

    #[test]
    fn e5_nonplanar_circle_scales_its_rational_uv_control_net() {
        let surface = crate::families::e5::records::E5Surface {
            pos: 0,
            record_id: 7,
            geometry: SurfaceGeometry::Torus {
                center: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 5.0,
                minor_radius: 2.0,
            },
            uv_scale: [0.2, 0.5],
        };
        let pcurve = crate::families::e5::graph::E5Pcurve::Circle {
            surface: 7,
            center: [10.0, 4.0],
            codes: [0, 0],
            radius: 2.0,
            range: [0.0, std::f64::consts::PI],
            tail: [0.0, 0.0],
        };
        let (geometry, range, endpoints) =
            e5_pcurve_on_surface(&pcurve, &surface).expect("normalized torus circle");
        assert_eq!(range, [0.0, std::f64::consts::FRAC_PI_2]);
        let PcurveGeometry::Nurbs { control_points, .. } = geometry else {
            panic!("expected rational NURBS pcurve");
        };
        let first = control_points.first().expect("first control");
        let last = control_points.last().expect("last control");
        assert!((first.u - 2.4).abs() < 1e-12 && (first.v - 2.0).abs() < 1e-12);
        assert!((last.u - 2.0).abs() < 1e-12 && (last.v - 3.0).abs() < 1e-12);
        let expected = [Point2::new(2.4, 2.0), Point2::new(2.0, 3.0)].map(|uv| {
            cadmpeg_ir::eval::surface_point(&surface.geometry, uv.u, uv.v).expect("torus point")
        });
        assert!(endpoints[0].distance(expected[0]) < 1e-12);
        assert!(endpoints[1].distance(expected[1]) < 1e-12);
    }

    #[test]
    fn occurrence_intersection_accepts_roundoff_equivalent_side_ranges() {
        let sides = vec![
            (
                SurfaceId("left".to_string()),
                PcurveGeometry::Line {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(1.0, 0.0),
                },
                [-2.0, 3.0],
            ),
            (
                SurfaceId("right".to_string()),
                PcurveGeometry::Line {
                    origin: Point2::new(0.0, 1.0),
                    direction: Point2::new(1.0, 0.0),
                },
                [-2.0 - 1e-14, 3.0 + 1e-14],
            ),
        ];
        let context = e5_occurrence_intersection_context(&sides).expect("intersection context");
        assert_eq!(context.parameter_range, [-2.0, 3.0]);
        assert_eq!(
            context.sides[0].surface.as_ref().expect("left surface").0,
            "left"
        );
        assert_eq!(
            context.sides[1].surface.as_ref().expect("right surface").0,
            "right"
        );
    }

    #[test]
    fn quintic_jet_reproduces_endpoint_second_order_data() {
        let curve = quintic_jet_pcurve(
            5,
            &[0.0, 2.0],
            &[[0.0, 0.0], [2.0, 0.0]],
            &[[1.0, 0.0], [1.0, 0.0]],
            &[[0.0, 0.0], [0.0, 0.0]],
        )
        .expect("linear quintic segment");
        for parameter in [0.0, 0.5, 1.0, 2.0] {
            let point = pcurve_uv(&curve, parameter).expect("jet evaluation");
            assert!((point.u - parameter).abs() < 1e-12);
            assert!(point.v.abs() < 1e-12);
        }
    }

    #[test]
    fn rank_one_plane_endpoints_complete_with_known_normal() {
        let pairs = [
            ([0.0, -2.0], Point3::new(-2.0, 0.0, 0.0)),
            ([0.0, 2.0], Point3::new(2.0, 0.0, 0.0)),
        ];
        let (u_axis, v_axis, residual) =
            fit_rank_one_e5_plane_axes([0.0; 3], &pairs, Vector3::new(0.0, 1.0, 0.0))
                .expect("rank-one frame");
        assert!(residual < 1e-12);
        assert!((v_axis.x - 1.0).abs() < 1e-12);
        assert!((u_axis.z - 1.0).abs() < 1e-12);
    }
}
