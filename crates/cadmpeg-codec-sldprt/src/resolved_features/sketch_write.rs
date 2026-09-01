//! Sketch record patching in native streams.

use super::SKETCH_POINT_TOLERANCE;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchGeometry};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::io::Write;

const EPS_SKETCH_WRITE_GEOMETRY: f64 = 1.0e-9;

pub(super) fn sketch_brep(
    source: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
) -> Result<cadmpeg_ir::CadIr, cadmpeg_core::CodecError> {
    let (origin, normal, u_axis) = sketch.resolved_placement().ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch {} requires resolved model-space placement",
            sketch.id.0
        ))
    })?;
    let mut ir = cadmpeg_ir::CadIr::empty(source.units.clone());
    let prefix = format!("generated:sldprt:sketch:{}", sketch.id.0);
    let body_id = BodyId(format!("{prefix}:body"));
    let region_id = RegionId(format!("{prefix}:region"));
    let shell_id = ShellId(format!("{prefix}:shell"));
    let face_id = FaceId(format!("{prefix}:face"));
    let surface_id = SurfaceId(format!("{prefix}:surface"));
    let v_axis = normal.cross(u_axis);
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        },
        source_object: None,
    });
    let ordered_entities = source
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
        .collect::<Vec<_>>();
    let entities = ordered_entities
        .iter()
        .copied()
        .map(|entity| (entity.id.clone(), entity))
        .collect::<HashMap<_, _>>();
    let referenced = sketch
        .profiles
        .iter()
        .flatten()
        .map(|entity_use| entity_use.entity.clone())
        .collect::<HashSet<_>>();
    if let Some(entity) = ordered_entities.iter().find(|entity| {
        !referenced.contains(&entity.id) && !matches!(entity.geometry, SketchGeometry::Point { .. })
    }) {
        return Err(cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch writing cannot encode unprofiled curve {}",
            entity.id.0
        )));
    }
    let profiles = sketch.profiles.clone();
    let mut face_loops = Vec::new();
    let mut vertex_by_position = HashMap::<(u64, u64), VertexId>::new();
    for (profile_index, profile) in profiles.iter().enumerate() {
        if profile.is_empty() {
            continue;
        }
        let endpoints = profile
            .iter()
            .map(|entity_use| {
                let entity = entities.get(&entity_use.entity).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "sketch {} references missing entity {}",
                        sketch.id.0, entity_use.entity.0
                    ))
                })?;
                let generated = generated_sketch_curve(&entity.geometry, sketch, v_axis)?;
                Ok(if entity_use.reversed {
                    (generated.end, generated.start)
                } else {
                    (generated.start, generated.end)
                })
            })
            .collect::<Result<Vec<_>, cadmpeg_core::CodecError>>()?;
        if endpoints.iter().enumerate().any(|(index, (_, end))| {
            let (next_start, _) = endpoints[(index + 1) % endpoints.len()];
            !same_sketch_point(*end, next_start)
        }) {
            return Err(cadmpeg_core::CodecError::NotImplemented(format!(
                "source-less SLDPRT sketch profile {profile_index} is not a closed endpoint chain"
            )));
        }
        let loop_id = LoopId(format!("{prefix}:loop:{profile_index}"));
        face_loops.push(loop_id.clone());
        let mut coedge_ids = Vec::new();
        for (use_index, entity_use) in profile.iter().enumerate() {
            let entity = entities.get(&entity_use.entity).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "sketch {} references missing entity {}",
                    sketch.id.0, entity_use.entity.0
                ))
            })?;
            let generated = generated_sketch_curve(&entity.geometry, sketch, v_axis)?;
            let start_vertex = sketch_vertex(
                &mut ir,
                &mut vertex_by_position,
                &prefix,
                generated.start,
                origin,
                u_axis,
                v_axis,
            );
            let end_vertex = sketch_vertex(
                &mut ir,
                &mut vertex_by_position,
                &prefix,
                generated.end,
                origin,
                u_axis,
                v_axis,
            );
            let start_3d = lift_point(generated.start, origin, u_axis, v_axis);
            let end_3d = lift_point(generated.end, origin, u_axis, v_axis);
            let delta = Vector3::new(
                end_3d.x - start_3d.x,
                end_3d.y - start_3d.y,
                end_3d.z - start_3d.z,
            );
            let length = delta.norm();
            if length == 0.0 && matches!(entity.geometry, SketchGeometry::Line { .. }) {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "sketch entity {} has zero length",
                    entity.id.0
                )));
            }
            let curve_id = CurveId(format!("{prefix}:curve:{profile_index}:{use_index}"));
            let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{use_index}"));
            let coedge_id = CoedgeId(format!("{prefix}:coedge:{profile_index}:{use_index}"));
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: generated.curve,
                source_object: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: start_vertex,
                end: end_vertex,
                param_range: Some(generated.param_range.unwrap_or([0.0, length])),
                tolerance: None,
            });
            coedge_ids.push(coedge_id.clone());
            ir.model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id,
                sense: if entity_use.reversed {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                use_curve: None,
                use_curve_parameter_range: None,
                pcurves: Vec::new(),
            });
        }
        let count = coedge_ids.len();
        for (index, coedge) in ir
            .model
            .coedges
            .iter_mut()
            .rev()
            .take(count)
            .rev()
            .enumerate()
        {
            coedge.next = coedge_ids[(index + 1) % count].clone();
            coedge.previous = coedge_ids[(index + count - 1) % count].clone();
        }
        ir.model.loops.push(Loop {
            id: loop_id,
            face: face_id.clone(),
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges: coedge_ids,
            vertex_uses: Vec::new(),
        });
    }
    for (ordinal, entity) in ordered_entities.iter().enumerate() {
        let SketchGeometry::Point { position } = entity.geometry else {
            continue;
        };
        let point_id = PointId(format!("{prefix}:free-point:{ordinal}"));
        let vertex_id = VertexId(format!("{prefix}:free-vertex:{ordinal}"));
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: lift_point(position, origin, u_axis, v_axis),
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: None,
        });
        let edge_id = EdgeId(format!("{prefix}:point-edge:{ordinal}"));
        let loop_id = LoopId(format!("{prefix}:point-loop:{ordinal}"));
        let coedge_id = CoedgeId(format!("{prefix}:point-coedge:{ordinal}"));
        ir.model.edges.push(Edge {
            id: edge_id.clone(),
            curve: None,
            start: vertex_id.clone(),
            end: vertex_id,
            param_range: None,
            tolerance: None,
        });
        ir.model.coedges.push(Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            next: coedge_id.clone(),
            previous: coedge_id.clone(),
            radial_next: coedge_id.clone(),
            sense: Sense::Forward,
            use_curve: None,
            use_curve_parameter_range: None,
            pcurves: Vec::new(),
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face: face_id.clone(),
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges: vec![coedge_id],
            vertex_uses: Vec::new(),
        });
        face_loops.push(loop_id);
    }
    if face_loops.is_empty() {
        return Err(cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch {} has no profiles",
            sketch.id.0
        )));
    }
    ir.model.faces.push(Face {
        id: face_id.clone(),
        shell: shell_id.clone(),
        surface: surface_id,
        sense: Sense::Forward,
        loops: face_loops,
        name: sketch.name.clone(),
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: vec![face_id],
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
        kind: BodyKind::Sheet,
        regions: vec![region_id],
        transform: None,
        name: sketch.name.clone(),
        color: None,
        visible: None,
    });
    ir.model.finalize();
    Ok(ir)
}

struct GeneratedSketchCurve {
    curve: CurveGeometry,
    start: Point2,
    end: Point2,
    param_range: Option<[f64; 2]>,
}

fn generated_sketch_curve(
    geometry: &SketchGeometry,
    sketch: &Sketch,
    v_axis: Vector3,
) -> Result<GeneratedSketchCurve, cadmpeg_core::CodecError> {
    let (origin, normal, u_axis) = sketch.resolved_placement().ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch {} requires resolved model-space placement",
            sketch.id.0
        ))
    })?;
    let lift = |point| lift_point(point, origin, u_axis, v_axis);
    let vector = |u: f64, v: f64| {
        Vector3::new(
            u_axis.x * u + v_axis.x * v,
            u_axis.y * u + v_axis.y * v,
            u_axis.z * u + v_axis.z * v,
        )
    };
    match geometry {
        SketchGeometry::Line { start, end } => {
            let origin = lift(*start);
            let target = lift(*end);
            let delta = Vector3::new(
                target.x - origin.x,
                target.y - origin.y,
                target.z - origin.z,
            );
            let length = delta.norm();
            if length == 0.0 {
                return Err(cadmpeg_core::CodecError::Malformed(
                    "source-less SLDPRT sketch contains a zero-length line".into(),
                ));
            }
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Line {
                    origin,
                    direction: Vector3::new(
                        delta.x / length,
                        delta.y / length,
                        delta.z / length,
                    ),
                },
                start: *start,
                end: *end,
                param_range: Some([0.0, length]),
            })
        }
        SketchGeometry::Circle { center, radius } => {
            let point = offset_point(*center, Point2::new(radius.0, 0.0));
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Circle {
                    center: lift(*center),
                    axis: normal,
                    ref_direction: u_axis,
                    radius: radius.0,
                },
                start: point,
                end: point,
                param_range: Some([0.0, std::f64::consts::TAU]),
            })
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Ok(GeneratedSketchCurve {
            curve: CurveGeometry::Circle {
                center: lift(*center),
                axis: normal,
                ref_direction: u_axis,
                radius: radius.0,
            },
            start: offset_point(*center, polar(radius.0, start_angle.0)),
            end: offset_point(*center, polar(radius.0, end_angle.0)),
            param_range: Some([start_angle.0, end_angle.0]),
        }),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        } => {
            let point = |parameter: f64| {
                Point2::new(
                    center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                        - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
                    center.v
                        + major_angle.0.sin() * major_radius.0 * parameter.cos()
                        + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
                )
            };
            let start = start_angle.as_ref().map_or(0.0, |angle| angle.0);
            let end = end_angle
                .as_ref()
                .map_or(std::f64::consts::TAU, |angle| angle.0);
            let full = start_angle.is_none() && end_angle.is_none();
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Ellipse {
                    center: lift(*center),
                    axis: normal,
                    major_direction: vector(major_angle.0.cos(), major_angle.0.sin()),
                    major_radius: major_radius.0,
                    minor_radius: minor_radius.0,
                },
                start: point(start),
                end: if full { point(start) } else { point(end) },
                param_range: Some([start, end]),
            })
        }
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => {
            if *periodic || control_points.len() < 2 {
                return Err(cadmpeg_core::CodecError::NotImplemented(
                    "source-less SLDPRT sketch writing requires a non-periodic NURBS with at least two poles".into(),
                ));
            }
            let start = control_points[0];
            let end = control_points[control_points.len() - 1];
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Nurbs(NurbsCurve {
                    degree: *degree,
                    knots: knots.clone(),
                    control_points: control_points.iter().copied().map(lift).collect(),
                    weights: weights.clone(),
                    periodic: false,
                }),
                start,
                end,
                param_range: knots
                    .get(*degree as usize)
                    .zip(knots.get(knots.len().saturating_sub(*degree as usize + 1)))
                    .map(|(start, end)| [*start, *end]),
            })
        }
        SketchGeometry::Point { .. }
        | SketchGeometry::Text { .. }
        | SketchGeometry::ReferenceLine { .. }
        | SketchGeometry::Hyperbola { .. }
        | SketchGeometry::Parabola { .. }
        | SketchGeometry::ExternalReference { .. }
        | SketchGeometry::Native { .. } => Err(
            cadmpeg_core::CodecError::NotImplemented(
                "source-less SLDPRT sketch writing does not support point or native-only profile entities".into(),
            ),
        ),
    }
}

fn sketch_vertex(
    ir: &mut cadmpeg_ir::CadIr,
    vertices: &mut HashMap<(u64, u64), VertexId>,
    prefix: &str,
    position: Point2,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
) -> VertexId {
    if let Some((_, id)) = vertices.iter().find(|((u, v), _)| {
        same_sketch_point(
            Point2::new(f64::from_bits(*u), f64::from_bits(*v)),
            position,
        )
    }) {
        return id.clone();
    }
    let key = (position.u.to_bits(), position.v.to_bits());
    let ordinal = vertices.len();
    let point_id = PointId(format!("{prefix}:point:{ordinal}"));
    let vertex_id = VertexId(format!("{prefix}:vertex:{ordinal}"));
    ir.model.points.push(Point {
        id: point_id.clone(),
        position: lift_point(position, origin, u_axis, v_axis),
        source_object: None,
    });
    ir.model.vertices.push(Vertex {
        id: vertex_id.clone(),
        point: point_id,
        tolerance: None,
    });
    vertices.insert(key, vertex_id.clone());
    vertex_id
}

pub(super) fn same_sketch_point(left: Point2, right: Point2) -> bool {
    (left.u - right.u).abs() <= SKETCH_POINT_TOLERANCE
        && (left.v - right.v).abs() <= SKETCH_POINT_TOLERANCE
}

pub(super) fn patch_line_profiles(
    ir: &cadmpeg_ir::CadIr,
    native: &mut crate::native::SldprtNative,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut requested = HashMap::<(String, usize, u16), Point3>::new();
    let mut curves = Vec::new();
    for sketch in &ir.model.sketches {
        let lane_id = sketch.native_ref.as_ref().ok_or_else(|| {
            cadmpeg_core::CodecError::NotImplemented(
                "SLDPRT sketch write-back requires native sketch provenance".into(),
            )
        })?;
        let (origin, normal, u_axis) = sketch.resolved_placement().ok_or_else(|| {
            cadmpeg_core::CodecError::NotImplemented(format!(
                "SLDPRT sketch write-back requires resolved placement for {}",
                sketch.id.0
            ))
        })?;
        let v_axis = normal.cross(u_axis);
        for entity in ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
        {
            if entity.endpoint_refs.len() != 2 {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "SLDPRT sketch entity {} lacks two endpoint references",
                    entity.id.0
                )));
            }
            match &entity.geometry {
                SketchGeometry::Point { position } => {
                    let reference = &entity.endpoint_refs[0];
                    let (stream, attr) = parse_point_ref(reference)?;
                    let point = lift_point(*position, origin, u_axis, v_axis);
                    let key = (lane_id.clone(), stream, attr);
                    if let Some(previous) = requested.insert(key, point) {
                        if distance(previous, point) > EPS_SKETCH_WRITE_GEOMETRY {
                            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                                "SLDPRT shared sketch point {reference} has conflicting positions"
                            )));
                        }
                    }
                }
                SketchGeometry::Line { start, end } => {
                    for (reference, point) in entity.endpoint_refs.iter().zip([start, end]) {
                        let (stream, attr) = parse_point_ref(reference)?;
                        let point = lift_point(*point, origin, u_axis, v_axis);
                        let key = (lane_id.clone(), stream, attr);
                        if let Some(previous) = requested.insert(key, point) {
                            if distance(previous, point) > EPS_SKETCH_WRITE_GEOMETRY {
                                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                                    "SLDPRT shared sketch point {reference} has conflicting positions"
                                )));
                            }
                        }
                    }
                }
                geometry @ (SketchGeometry::Circle { .. }
                | SketchGeometry::Arc { .. }
                | SketchGeometry::Ellipse { .. }
                | SketchGeometry::Nurbs { .. }) => {
                    let geometry_ref = entity.geometry_ref.as_deref().ok_or_else(|| {
                        cadmpeg_core::CodecError::Malformed(
                            "SLDPRT sketch curve lacks native carrier provenance".into(),
                        )
                    })?;
                    let (stream, carrier_attr) = parse_point_ref(geometry_ref)?;
                    let (_, start_attr) = parse_point_ref(&entity.endpoint_refs[0])?;
                    let (_, end_attr) = parse_point_ref(&entity.endpoint_refs[1])?;
                    if let Some(endpoints) = bounded_endpoints(geometry) {
                        for (reference, point) in entity.endpoint_refs.iter().zip(endpoints) {
                            let (point_stream, attr) = parse_point_ref(reference)?;
                            let point = lift_point(point, origin, u_axis, v_axis);
                            let key = (lane_id.clone(), point_stream, attr);
                            if let Some(previous) = requested.insert(key, point) {
                                if distance(previous, point) > EPS_SKETCH_WRITE_GEOMETRY {
                                    return Err(cadmpeg_core::CodecError::malformed(format_args!(
                                        "SLDPRT shared sketch point {reference} has conflicting positions"
                                    )));
                                }
                            }
                        }
                    }
                    curves.push(CurvePatch {
                        lane_id: lane_id.clone(),
                        stream,
                        carrier_attr,
                        start_attr,
                        end_attr,
                        geometry: geometry.clone(),
                        origin,
                        u_axis,
                        v_axis,
                    });
                }
                _ => {
                    return Err(cadmpeg_core::CodecError::NotImplemented(
                        "SLDPRT sketch write-back does not support this curve family".into(),
                    ));
                }
            }
        }
    }
    for ((lane_id, stream_ordinal, attr), point) in requested {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.id == lane_id)
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "SLDPRT sketch lane {lane_id} is missing"
                ))
            })?;
        patch_direct_stream_point(&mut lane.native_payload, stream_ordinal, attr, point)?;
    }
    for request in curves {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.id == request.lane_id)
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "SLDPRT sketch lane {} is missing",
                    request.lane_id
                ))
            })?;
        patch_direct_curve(&mut lane.native_payload, &request)?;
    }
    Ok(())
}

fn bounded_endpoints(geometry: &SketchGeometry) -> Option<[Point2; 2]> {
    match geometry {
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some([
            offset_point(*center, polar(radius.0, start_angle.0)),
            offset_point(*center, polar(radius.0, end_angle.0)),
        ]),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle: Some(start),
            end_angle: Some(end),
        } => {
            let point = |parameter: f64| {
                Point2::new(
                    center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                        - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
                    center.v
                        + major_angle.0.sin() * major_radius.0 * parameter.cos()
                        + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
                )
            };
            Some([point(start.0), point(end.0)])
        }
        SketchGeometry::Nurbs {
            control_points,
            periodic: false,
            ..
        } if control_points.len() >= 2 => {
            Some([control_points[0], control_points[control_points.len() - 1]])
        }
        _ => None,
    }
}

struct CurvePatch {
    lane_id: String,
    stream: usize,
    carrier_attr: u16,
    start_attr: u16,
    end_attr: u16,
    geometry: SketchGeometry,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
}

fn parse_point_ref(reference: &str) -> Result<(usize, u16), cadmpeg_core::CodecError> {
    let (stream, id) = reference.split_once(':').ok_or_else(|| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "invalid SLDPRT sketch endpoint reference {reference}"
        ))
    })?;
    let attr = id.rsplit('#').next().and_then(|value| value.parse().ok());
    match (stream.parse().ok(), attr) {
        (Some(stream), Some(attr)) => Ok((stream, attr)),
        _ => Err(cadmpeg_core::CodecError::malformed(format_args!(
            "invalid SLDPRT sketch endpoint reference {reference}"
        ))),
    }
}

fn lift_point(point: Point2, origin: Point3, u_axis: Vector3, v_axis: Vector3) -> Point3 {
    Point3::new(
        origin.x + point.u * u_axis.x + point.v * v_axis.x,
        origin.y + point.u * u_axis.y + point.v * v_axis.y,
        origin.z + point.u * u_axis.z + point.v * v_axis.z,
    )
}

pub(super) fn distance(left: Point3, right: Point3) -> f64 {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
}

fn patch_direct_stream_point(
    payload: &mut Vec<u8>,
    stream_ordinal: usize,
    attr: u16,
    point_mm: Point3,
) -> Result<(), cadmpeg_core::CodecError> {
    let xyz_m = [point_mm.x * 0.001, point_mm.y * 0.001, point_mm.z * 0.001];
    edit_stream(payload, stream_ordinal, |body| {
        if !crate::brep::patch_point(body, attr, xyz_m) {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "SLDPRT sketch point {attr} is missing"
            )));
        }
        Ok(())
    })
}

fn patch_direct_curve(
    payload: &mut Vec<u8>,
    request: &CurvePatch,
) -> Result<(), cadmpeg_core::CodecError> {
    edit_stream(payload, request.stream, |body| {
        patch_direct_curve_body(body, request)
    })
}

fn patch_direct_curve_body(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_core::CodecError> {
    if matches!(request.geometry, SketchGeometry::Nurbs { .. }) {
        return patch_direct_nurbs(body, request);
    }
    let Some(CurveGeometry::Circle {
        axis,
        ref_direction,
        ..
    }) = crate::brep::curve_by_attr(body, request.carrier_attr)
    else {
        return patch_direct_ellipse(body, request);
    };
    let (center_2d, radius, angles) = match request.geometry {
        SketchGeometry::Circle { center, radius } => (center, radius.0, None),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (center, radius.0, Some((start_angle.0, end_angle.0))),
        _ => {
            return Err(cadmpeg_core::CodecError::Malformed(
                "SLDPRT sketch carrier family changed".into(),
            ));
        }
    };
    let center = lift_point(center_2d, request.origin, request.u_axis, request.v_axis);
    let curve = CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    };
    let (_, values) = crate::writer::curve_values(&curve, 0.001)?;
    if !crate::brep::patch_compact_values(body, request.carrier_attr, &values) {
        return Err(cadmpeg_core::CodecError::Malformed(
            "SLDPRT sketch circle carrier cannot be patched".into(),
        ));
    }
    let endpoints = angles.map_or(
        [offset_point(center_2d, polar(radius, 0.0)); 2],
        |(start, end)| {
            [
                offset_point(center_2d, polar(radius, start)),
                offset_point(center_2d, polar(radius, end)),
            ]
        },
    );
    for (attr, endpoint) in [request.start_attr, request.end_attr]
        .into_iter()
        .zip(endpoints)
    {
        let point = lift_point(endpoint, request.origin, request.u_axis, request.v_axis);
        if !crate::brep::patch_point(
            body,
            attr,
            [point.x * 0.001, point.y * 0.001, point.z * 0.001],
        ) {
            return Err(cadmpeg_core::CodecError::Malformed(
                "SLDPRT sketch curve endpoint is missing".into(),
            ));
        }
    }
    Ok(())
}

fn edit_stream(
    payload: &mut Vec<u8>,
    stream_ordinal: usize,
    edit: impl FnOnce(&mut [u8]) -> Result<(), cadmpeg_core::CodecError>,
) -> Result<(), cadmpeg_core::CodecError> {
    let stream = crate::parasolid::extract_streams_with_offsets(payload)
        .get(stream_ordinal)
        .cloned()
        .ok_or_else(|| {
            cadmpeg_core::CodecError::Malformed("SLDPRT sketch stream is missing".into())
        })?;
    let body_offset = stream.header.body_offset;
    if let Some(start) = payload
        .windows(stream.payload.len())
        .position(|candidate| candidate == stream.payload.as_slice())
    {
        return edit(&mut payload[start + body_offset..start + stream.payload.len()]);
    }
    let (start, end) = compressed_member(payload, &stream.payload).ok_or_else(|| {
        cadmpeg_core::CodecError::Malformed(
            "compressed retained SLDPRT sketch stream is missing".into(),
        )
    })?;
    let mut inflated = stream.payload;
    edit(&mut inflated[body_offset..])?;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&inflated)?;
    payload.splice(start..end, encoder.finish()?);
    Ok(())
}

fn compressed_member(payload: &[u8], target: &[u8]) -> Option<(usize, usize)> {
    for start in 0..payload.len().saturating_sub(1) {
        if payload[start] != 0x78 || !matches!(payload[start + 1], 0x01 | 0x9c | 0xda) {
            continue;
        }
        // Cap inflation at `target.len() + 1` bytes: this scan only accepts a member
        // whose inflated body equals `target`, so any stream that expands past the
        // target length can never match and need not be materialized.
        let ceiling = target.len().saturating_add(1);
        let mut decoder = flate2::read::ZlibDecoder::new(&payload[start..]).take(ceiling as u64);
        let mut inflated = Vec::with_capacity(ceiling);
        let mut chunk = [0_u8; 8192];
        let mut valid = true;
        loop {
            match decoder.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => inflated.extend_from_slice(&chunk[..read]),
                Err(_) => {
                    valid = false;
                    break;
                }
            }
        }
        if valid && inflated == target {
            return Some((start, start + decoder.into_inner().total_in() as usize));
        }
    }
    None
}

fn patch_direct_nurbs(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_core::CodecError> {
    let SketchGeometry::Nurbs {
        degree,
        ref knots,
        ref control_points,
        ref weights,
        periodic,
    } = request.geometry
    else {
        unreachable!();
    };
    let curve = cadmpeg_ir::geometry::NurbsCurve {
        degree,
        knots: knots.clone(),
        control_points: control_points
            .iter()
            .map(|point| lift_point(*point, request.origin, request.u_axis, request.v_axis))
            .collect(),
        weights: weights.clone(),
        periodic,
    };
    if !crate::brep::patch_nurbs_by_attr(body, request.carrier_attr, &curve) {
        return Err(cadmpeg_core::CodecError::NotImplemented(
            "SLDPRT sketch NURBS edit changes native storage shape".into(),
        ));
    }
    Ok(())
}

fn patch_direct_ellipse(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_core::CodecError> {
    let Some(CurveGeometry::Ellipse { axis, .. }) =
        crate::brep::curve_by_attr(body, request.carrier_attr)
    else {
        return Err(cadmpeg_core::CodecError::Malformed(
            "SLDPRT sketch analytic carrier is missing".into(),
        ));
    };
    let SketchGeometry::Ellipse {
        center,
        major_angle,
        major_radius,
        minor_radius,
        start_angle,
        end_angle,
    } = request.geometry
    else {
        return Err(cadmpeg_core::CodecError::Malformed(
            "SLDPRT sketch carrier family changed".into(),
        ));
    };
    let center_3d = lift_point(center, request.origin, request.u_axis, request.v_axis);
    let major_direction = Vector3::new(
        request.u_axis.x * major_angle.0.cos() + request.v_axis.x * major_angle.0.sin(),
        request.u_axis.y * major_angle.0.cos() + request.v_axis.y * major_angle.0.sin(),
        request.u_axis.z * major_angle.0.cos() + request.v_axis.z * major_angle.0.sin(),
    );
    let curve = CurveGeometry::Ellipse {
        center: center_3d,
        axis,
        major_direction,
        major_radius: major_radius.0,
        minor_radius: minor_radius.0,
    };
    let (_, values) = crate::writer::curve_values(&curve, 0.001)?;
    if !crate::brep::patch_compact_values(body, request.carrier_attr, &values) {
        return Err(cadmpeg_core::CodecError::Malformed(
            "SLDPRT sketch ellipse carrier cannot be patched".into(),
        ));
    }
    let parameters = match (start_angle, end_angle) {
        (Some(start), Some(end)) => [start.0, end.0],
        (None, None) => [0.0, 0.0],
        _ => {
            return Err(cadmpeg_core::CodecError::Malformed(
                "SLDPRT sketch ellipse has only one bounded endpoint".into(),
            ));
        }
    };
    for (attr, parameter) in [request.start_attr, request.end_attr]
        .into_iter()
        .zip(parameters)
    {
        let local = Point2::new(
            center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
            center.v
                + major_angle.0.sin() * major_radius.0 * parameter.cos()
                + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
        );
        let point = lift_point(local, request.origin, request.u_axis, request.v_axis);
        if !crate::brep::patch_point(
            body,
            attr,
            [point.x * 0.001, point.y * 0.001, point.z * 0.001],
        ) {
            return Err(cadmpeg_core::CodecError::Malformed(
                "SLDPRT sketch ellipse endpoint is missing".into(),
            ));
        }
    }
    Ok(())
}

fn polar(radius: f64, angle: f64) -> Point2 {
    Point2::new(radius * angle.cos(), radius * angle.sin())
}

fn offset_point(origin: Point2, delta: Point2) -> Point2 {
    Point2::new(origin.u + delta.u, origin.v + delta.v)
}

#[cfg(test)]
mod sketch_write_tests;
