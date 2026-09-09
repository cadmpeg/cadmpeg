//! Sketch record patching in native streams.

use super::SKETCH_POINT_TOLERANCE;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchGeometry, SketchGeometryDefinition};
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
            sketch.id.as_str()
        ))
    })?;
    let mut ir = cadmpeg_ir::CadIr::empty();
    let sketch_key = sketch.id.as_str().replace('%', "%25").replace('#', "%23");
    let prefix = format!("generated:sldprt:sketch#{sketch_key}");
    let body_id = BodyId::mint(format!("{prefix}:body")).expect("identity grammar");
    let region_id = RegionId::mint(format!("{prefix}:region")).expect("identity grammar");
    let shell_id = ShellId::mint(format!("{prefix}:shell")).expect("identity grammar");
    let face_id = FaceId::mint(format!("{prefix}:face")).expect("identity grammar");
    let surface_id = SurfaceId::mint(format!("{prefix}:surface")).expect("identity grammar");
    let v_axis = normal.cross(u_axis);
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(origin, normal, u_axis)
                .map_err(cadmpeg_core::CodecError::malformed)?,
        ),
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
        .map(|entity| (entity.id().clone(), entity))
        .collect::<HashMap<_, _>>();
    let referenced = sketch
        .profiles
        .iter()
        .flatten()
        .map(|entity_use| entity_use.entity.clone())
        .collect::<HashSet<_>>();
    if let Some(entity) = ordered_entities.iter().find(|entity| {
        !referenced.contains(entity.id())
            && !matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            )
    }) {
        return Err(cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch writing cannot encode unprofiled curve {}",
            entity.id().as_str()
        )));
    }
    let profiles = sketch.profiles.clone();
    let mut face_loops = Vec::new();
    let mut vertex_by_position = HashMap::<(u64, u64), VertexId>::new();
    for (profile_index, profile) in profiles.iter().enumerate() {
        let endpoints = profile
            .iter()
            .map(|entity_use| {
                let entity = entities.get(&entity_use.entity).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "sketch {} references missing entity {}",
                        sketch.id.as_str(),
                        entity_use.entity.as_str()
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
        let loop_id =
            LoopId::mint(format!("{prefix}:loop:{profile_index}")).expect("identity grammar");
        face_loops.push(loop_id.clone());
        let mut ring: Option<cadmpeg_ir::topology::LoopRing> = None;
        for (use_index, entity_use) in profile.iter().enumerate() {
            let entity = entities.get(&entity_use.entity).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "sketch {} references missing entity {}",
                    sketch.id.as_str(),
                    entity_use.entity.as_str()
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
            if length == 0.0
                && matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
            {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "sketch entity {} has zero length",
                    entity.id().as_str()
                )));
            }
            let curve_id = CurveId::mint(format!("{prefix}:curve:{profile_index}:{use_index}"))
                .expect("identity grammar");
            let edge_id = EdgeId::mint(format!("{prefix}:edge:{profile_index}:{use_index}"))
                .expect("identity grammar");
            let coedge_id = CoedgeId::mint(format!("{prefix}:coedge:{profile_index}:{use_index}"))
                .expect("identity grammar");
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: generated.curve,
                source_object: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                    Some(curve_id),
                    Some(generated.param_range),
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,
                start: start_vertex,
                end: end_vertex,
                tolerance: None,
            });
            match &mut ring {
                Some(ring) => ring
                    .try_push(coedge_id.clone())
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                None => ring = Some(cadmpeg_ir::topology::LoopRing::single(coedge_id.clone())),
            }
            ir.model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                radial_next: coedge_id,
                sense: if entity_use.reversed {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                use_curve: None,
                pcurves: Vec::new(),
            });
        }
        ir.model.loops.push(Loop {
            id: loop_id,
            face: face_id.clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring.ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("sketch profile has no coedges")
            })?),
        });
    }
    for (ordinal, entity) in ordered_entities.iter().enumerate() {
        let SketchGeometryDefinition::Point { position } = *entity.geometry.definition() else {
            continue;
        };
        let point_id =
            PointId::mint(format!("{prefix}:free-point:{ordinal}")).expect("identity grammar");
        let vertex_id =
            VertexId::mint(format!("{prefix}:free-vertex:{ordinal}")).expect("identity grammar");
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
        let edge_id =
            EdgeId::mint(format!("{prefix}:point-edge:{ordinal}")).expect("identity grammar");
        let loop_id =
            LoopId::mint(format!("{prefix}:point-loop:{ordinal}")).expect("identity grammar");
        let coedge_id =
            CoedgeId::mint(format!("{prefix}:point-coedge:{ordinal}")).expect("identity grammar");
        ir.model.edges.push(Edge {
            id: edge_id.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
            start: vertex_id.clone(),
            end: vertex_id,
            tolerance: None,
        });
        ir.model.coedges.push(Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            radial_next: coedge_id.clone(),
            sense: Sense::Forward,
            use_curve: None,
            pcurves: Vec::new(),
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face: face_id.clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::single(coedge_id),
            ),
        });
        face_loops.push(loop_id);
    }
    if face_loops.is_empty() {
        return Err(cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch {} has no profiles",
            sketch.id.as_str()
        )));
    }
    ir.model.faces.push(Face {
        id: face_id.clone(),
        shell: shell_id.clone(),
        surface: surface_id,
        sense: Sense::Forward,
        loops: face_loops.into(),
        name: sketch.name.clone(),
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell::with_face(
        shell_id.clone(),
        region_id.clone(),
        face_id,
    ));
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
    param_range: [f64; 2],
}

fn generated_sketch_curve(
    geometry: &SketchGeometry,
    sketch: &Sketch,
    v_axis: Vector3,
) -> Result<GeneratedSketchCurve, cadmpeg_core::CodecError> {
    let (origin, normal, u_axis) = sketch.resolved_placement().ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch {} requires resolved model-space placement",
            sketch.id.as_str()
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
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
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
                curve: CurveGeometry::Line(cadmpeg_ir::geometry::LineCurve::try_new(origin, Vector3::new(
                        delta.x / length,
                        delta.y / length,
                        delta.z / length,
                    )).map_err(cadmpeg_core::CodecError::malformed)?),
                start: *start,
                end: *end,
                param_range: [0.0, length],
            })
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            let point = offset_point(*center, Point2::new(radius.get(), 0.0));
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Circle(cadmpeg_ir::geometry::CircleCurve::try_new(lift(*center), normal, u_axis, radius.get()).map_err(cadmpeg_core::CodecError::malformed)?),
                start: point,
                end: point,
                param_range: [0.0, std::f64::consts::TAU],
            })
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Ok(GeneratedSketchCurve {
            curve: CurveGeometry::Circle(cadmpeg_ir::geometry::CircleCurve::try_new(lift(*center), normal, u_axis, radius.get()).map_err(cadmpeg_core::CodecError::malformed)?),
            start: offset_point(*center, polar(radius.get(), start_angle.get())),
            end: offset_point(*center, polar(radius.get(), end_angle.get())),
            param_range: [start_angle.get(), end_angle.get()],
        }),
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            let point = |parameter: f64| {
                Point2::new(
                    center.u + major_angle.get().cos() * major_radius.get() * parameter.cos()
                        - major_angle.get().sin() * minor_radius.get() * parameter.sin(),
                    center.v
                        + major_angle.get().sin() * major_radius.get() * parameter.cos()
                        + major_angle.get().cos() * minor_radius.get() * parameter.sin(),
                )
            };
            let [start, end] = bounds
                .as_ref()
                .map_or([0.0, std::f64::consts::TAU], |[start, end]| {
                    [start.get(), end.get()]
                });
            let full = bounds.is_none();
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Ellipse(cadmpeg_ir::geometry::EllipseCurve::try_new(lift(*center), normal, vector(major_angle.get().cos(), major_angle.get().sin()), major_radius.get(), minor_radius.get()).map_err(cadmpeg_core::CodecError::malformed)?),
                start: point(start),
                end: if full { point(start) } else { point(end) },
                param_range: [start, end],
            })
        }
        SketchGeometryDefinition::Nurbs { curve } => {
            if curve.periodic() {
                return Err(cadmpeg_core::CodecError::NotImplemented(
                    "source-less SLDPRT sketch writing requires a non-periodic NURBS with at least two poles".into(),
                ));
            }
            let control_points = curve.control_points();
            let start = control_points[0];
            let end = control_points[control_points.len() - 1];
            let knots = curve.knots();
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Nurbs(curve.lift(lift).map_err(|error| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "source-less SLDPRT sketch NURBS lift is invalid: {error}"
                    ))
                })?),
                start,
                end,
                param_range: [knots[curve.degree() as usize], knots[control_points.len()]],
            })
        }
        SketchGeometryDefinition::Point { .. }
        | SketchGeometryDefinition::Text { .. }
        | SketchGeometryDefinition::ReferenceLine { .. }
        | SketchGeometryDefinition::Hyperbola { .. }
        | SketchGeometryDefinition::Parabola { .. }
        | SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. } => Err(
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
    let point_id = PointId::mint(format!("{prefix}:point:{ordinal}")).expect("identity grammar");
    let vertex_id = VertexId::mint(format!("{prefix}:vertex:{ordinal}")).expect("identity grammar");
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
                sketch.id.as_str()
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
                    entity.id().as_str()
                )));
            }
            match entity.geometry.definition() {
                SketchGeometryDefinition::Point { position } => {
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
                SketchGeometryDefinition::Line { start, end } => {
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
                _ => {
                    let geometry = &entity.geometry;
                    let patch_geometry = PatchCurve::try_from(geometry)?;
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
                        geometry: patch_geometry,
                        origin,
                        u_axis,
                        v_axis,
                    });
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
    match geometry.definition() {
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some([
            offset_point(*center, polar(radius.get(), start_angle.get())),
            offset_point(*center, polar(radius.get(), end_angle.get())),
        ]),
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds: Some([start, end]),
        } => {
            let point = |parameter: f64| {
                Point2::new(
                    center.u + major_angle.get().cos() * major_radius.get() * parameter.cos()
                        - major_angle.get().sin() * minor_radius.get() * parameter.sin(),
                    center.v
                        + major_angle.get().sin() * major_radius.get() * parameter.cos()
                        + major_angle.get().cos() * minor_radius.get() * parameter.sin(),
                )
            };
            Some([point(start.get()), point(end.get())])
        }
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let control_points = curve.control_points();
            Some([control_points[0], control_points[control_points.len() - 1]])
        }
        _ => None,
    }
}

enum PatchCurve {
    Circle {
        center: Point2,
        radius: f64,
    },
    Arc {
        center: Point2,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    Ellipse(PatchEllipse),
    Nurbs(cadmpeg_ir::geometry::PcurveNurbs),
}

struct PatchEllipse {
    center: Point2,
    major_angle: f64,
    major_radius: f64,
    minor_radius: f64,
    bounds: Option<[f64; 2]>,
}

impl TryFrom<&SketchGeometry> for PatchCurve {
    type Error = cadmpeg_core::CodecError;

    fn try_from(geometry: &SketchGeometry) -> Result<Self, Self::Error> {
        match geometry.definition() {
            SketchGeometryDefinition::Circle { center, radius } => Ok(Self::Circle {
                center: *center,
                radius: radius.get(),
            }),
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => Ok(Self::Arc {
                center: *center,
                radius: radius.get(),
                start_angle: start_angle.get(),
                end_angle: end_angle.get(),
            }),
            SketchGeometryDefinition::Ellipse {
                center,
                major_angle,
                major_radius,
                minor_radius,
                bounds,
            } => Ok(Self::Ellipse(PatchEllipse {
                center: *center,
                major_angle: major_angle.get(),
                major_radius: major_radius.get(),
                minor_radius: minor_radius.get(),
                bounds: bounds.map(|[start, end]| [start.get(), end.get()]),
            })),
            SketchGeometryDefinition::Nurbs { curve } => Ok(Self::Nurbs(curve.clone())),
            _ => Err(cadmpeg_core::CodecError::NotImplemented(
                "SLDPRT sketch write-back does not support this curve family".into(),
            )),
        }
    }
}

struct CurvePatch {
    lane_id: String,
    stream: usize,
    carrier_attr: u16,
    start_attr: u16,
    end_attr: u16,
    geometry: PatchCurve,
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
    let (center_2d, radius, angles) = match &request.geometry {
        PatchCurve::Circle { center, radius } => (*center, *radius, None),
        PatchCurve::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (*center, *radius, Some((*start_angle, *end_angle))),
        PatchCurve::Ellipse(ellipse) => return patch_direct_ellipse(body, request, ellipse),
        PatchCurve::Nurbs(curve) => return patch_direct_nurbs(body, request, curve),
    };
    let (axis, ref_direction) = match crate::brep::curve_by_attr(body, request.carrier_attr) {
        Some(CurveGeometry::Circle(circle_curve)) => {
            let (_, &axis, &ref_direction, _) = circle_curve.parts();
            (axis, ref_direction)
        }
        Some(CurveGeometry::Ellipse(_)) => {
            return Err(cadmpeg_core::CodecError::Malformed(
                "SLDPRT sketch carrier family changed".into(),
            ));
        }
        _ => {
            return Err(cadmpeg_core::CodecError::Malformed(
                "SLDPRT sketch analytic carrier is missing".into(),
            ));
        }
    };
    let center = lift_point(center_2d, request.origin, request.u_axis, request.v_axis);
    let curve = CurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(center, axis, ref_direction, radius)
            .map_err(cadmpeg_core::CodecError::malformed)?,
    );
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
    curve: &cadmpeg_ir::geometry::PcurveNurbs,
) -> Result<(), cadmpeg_core::CodecError> {
    let curve = curve
        .lift(|point| lift_point(point, request.origin, request.u_axis, request.v_axis))
        .map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "SLDPRT sketch NURBS lift is invalid: {error}"
            ))
        })?;
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
    ellipse: &PatchEllipse,
) -> Result<(), cadmpeg_core::CodecError> {
    let axis = match crate::brep::curve_by_attr(body, request.carrier_attr) {
        Some(CurveGeometry::Ellipse(ellipse_curve)) => {
            let (_, &axis, _, _, _) = ellipse_curve.parts();
            axis
        }
        Some(CurveGeometry::Circle(_)) => {
            return Err(cadmpeg_core::CodecError::Malformed(
                "SLDPRT sketch carrier family changed".into(),
            ));
        }
        _ => {
            return Err(cadmpeg_core::CodecError::Malformed(
                "SLDPRT sketch analytic carrier is missing".into(),
            ));
        }
    };
    let PatchEllipse {
        center,
        major_angle,
        major_radius,
        minor_radius,
        bounds,
    } = *ellipse;
    let center_3d = lift_point(center, request.origin, request.u_axis, request.v_axis);
    let major_direction = Vector3::new(
        request.u_axis.x * major_angle.cos() + request.v_axis.x * major_angle.sin(),
        request.u_axis.y * major_angle.cos() + request.v_axis.y * major_angle.sin(),
        request.u_axis.z * major_angle.cos() + request.v_axis.z * major_angle.sin(),
    );
    let curve = CurveGeometry::Ellipse(
        cadmpeg_ir::geometry::EllipseCurve::try_new(
            center_3d,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    );
    let (_, values) = crate::writer::curve_values(&curve, 0.001)?;
    if !crate::brep::patch_compact_values(body, request.carrier_attr, &values) {
        return Err(cadmpeg_core::CodecError::Malformed(
            "SLDPRT sketch ellipse carrier cannot be patched".into(),
        ));
    }
    let parameters = bounds.unwrap_or([0.0, 0.0]);
    for (attr, parameter) in [request.start_attr, request.end_attr]
        .into_iter()
        .zip(parameters)
    {
        let local = Point2::new(
            center.u + major_angle.cos() * major_radius * parameter.cos()
                - major_angle.sin() * minor_radius * parameter.sin(),
            center.v
                + major_angle.sin() * major_radius * parameter.cos()
                + major_angle.cos() * minor_radius * parameter.sin(),
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
