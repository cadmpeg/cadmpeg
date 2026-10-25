//! Sketch B-rep synthesis from the current neutral geometry.

use super::SKETCH_POINT_TOLERANCE;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
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

/// The namespace every identity a written sketch generates is composed under.
fn sketch_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("generated", "sldprt", "sketch")
}

pub(super) fn sketch_brep(
    source: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
) -> Result<cadmpeg_ir::CadIr, cadmpeg_core::CodecError> {
    let (origin, normal, u_axis) = sketch
        .resolved_placement()
        .map(|(origin, normal, u_axis)| (origin.get(), normal.get(), u_axis.get()))
        .ok_or_else(|| {
            cadmpeg_core::CodecError::UnsupportedContent(format!(
                "source-less SLDPRT sketch {} requires resolved model-space placement",
                sketch.id.as_str()
            ))
        })?;
    let mut ir = cadmpeg_ir::CadIr::empty();
    // The sketch identity is escaped into one key here, once, and every
    // generated id below is that key with a `:`-separated part appended.
    let prefix = cadmpeg_ir::ids::IdentityKey::try_new(
        sketch.id.as_str().replace('%', "%25").replace('#', "%23"),
    )
    .map_err(|error| {
        cadmpeg_core::CodecError::malformed(format_args!(
            "SLDPRT sketch identity is not identity key text: {error}"
        ))
    })?;
    let body_id = BodyId::compose(
        &sketch_namespace(),
        prefix.clone().colon(cadmpeg_ir::identity_key!("body")),
    );
    let region_id = RegionId::compose(
        &sketch_namespace(),
        prefix.clone().colon(cadmpeg_ir::identity_key!("region")),
    );
    let shell_id = ShellId::compose(
        &sketch_namespace(),
        prefix.clone().colon(cadmpeg_ir::identity_key!("shell")),
    );
    let face_id = FaceId::compose(
        &sketch_namespace(),
        prefix.clone().colon(cadmpeg_ir::identity_key!("face")),
    );
    let surface_id = SurfaceId::compose(
        &sketch_namespace(),
        prefix.clone().colon(cadmpeg_ir::identity_key!("surface")),
    );
    let v_axis = normal.cross(u_axis);
    ir.model.surfaces.push(Surface::new(
        surface_id.clone(),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(origin, normal, u_axis)
                .map_err(cadmpeg_core::CodecError::malformed)?,
        )),
        None,
    ));
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
        return Err(cadmpeg_core::CodecError::UnsupportedContent(format!(
            "source-less SLDPRT sketch writing cannot encode unprofiled curve {}",
            entity.id()
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
            return Err(cadmpeg_core::CodecError::UnsupportedContent(format!(
                "source-less SLDPRT sketch profile {profile_index} is not a closed endpoint chain"
            )));
        }
        let loop_id = LoopId::compose(
            &sketch_namespace(),
            prefix
                .clone()
                .colon(cadmpeg_ir::identity_key!("loop"))
                .colon(profile_index),
        );
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
            )?;
            let end_vertex = sketch_vertex(
                &mut ir,
                &mut vertex_by_position,
                &prefix,
                generated.end,
                origin,
                u_axis,
                v_axis,
            )?;
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
                    entity.id()
                )));
            }
            let curve_id = CurveId::compose(
                &sketch_namespace(),
                prefix
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("curve"))
                    .colon(profile_index)
                    .colon(use_index),
            );
            let edge_id = EdgeId::compose(
                &sketch_namespace(),
                prefix
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("edge"))
                    .colon(profile_index)
                    .colon(use_index),
            );
            let coedge_id = CoedgeId::compose(
                &sketch_namespace(),
                prefix
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("coedge"))
                    .colon(profile_index)
                    .colon(use_index),
            );
            ir.model.curves.push({
                let (parameter_range, id, geometry, source_object) =
                    (None, curve_id.clone(), generated.curve, None);
                Curve::new(id, geometry, parameter_range, source_object)
            });
            ir.model.edges.push(Edge::new(
                edge_id.clone(),
                cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id), Some(generated.param_range))
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                start_vertex,
                end_vertex,
                None,
            ));
            match &mut ring {
                Some(ring) => ring
                    .try_push(coedge_id.clone())
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                None => ring = Some(cadmpeg_ir::topology::LoopRing::single(coedge_id.clone())),
            }
            ir.model.coedges.push({
                let (id, owner_loop, edge, radial_next, sense, use_curve, pcurves) = (
                    coedge_id.clone(),
                    loop_id.clone(),
                    edge_id,
                    coedge_id,
                    if entity_use.reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    None,
                    Vec::new(),
                );
                Coedge::new(id, owner_loop, edge, radial_next, sense, pcurves, use_curve)
            });
        }
        ir.model.loops.push(Loop::new(
            loop_id,
            face_id.clone(),
            cadmpeg_ir::topology::LoopBoundary::Ring(ring.ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("sketch profile has no coedges")
            })?),
        ));
    }
    for (ordinal, entity) in ordered_entities.iter().enumerate() {
        let SketchGeometryDefinition::Point { position } = *entity.geometry.definition() else {
            continue;
        };
        let point_id = PointId::compose(
            &sketch_namespace(),
            prefix
                .clone()
                .colon(cadmpeg_ir::identity_key!("free-point"))
                .colon(ordinal),
        );
        let vertex_id = VertexId::compose(
            &sketch_namespace(),
            prefix
                .clone()
                .colon(cadmpeg_ir::identity_key!("free-vertex"))
                .colon(ordinal),
        );
        let finite_position = cadmpeg_ir::features::FinitePoint3::new(lift_point(
            position.get(),
            origin,
            u_axis,
            v_axis,
        ))
        .ok_or(Point::NON_FINITE_POSITION)
        .map_err(cadmpeg_core::CodecError::malformed)?;
        ir.model
            .points
            .push(Point::new(point_id.clone(), finite_position, None));
        ir.model
            .vertices
            .push(Vertex::new(vertex_id.clone(), point_id, None));
        let edge_id = EdgeId::compose(
            &sketch_namespace(),
            prefix
                .clone()
                .colon(cadmpeg_ir::identity_key!("point-edge"))
                .colon(ordinal),
        );
        let loop_id = LoopId::compose(
            &sketch_namespace(),
            prefix
                .clone()
                .colon(cadmpeg_ir::identity_key!("point-loop"))
                .colon(ordinal),
        );
        let coedge_id = CoedgeId::compose(
            &sketch_namespace(),
            prefix
                .clone()
                .colon(cadmpeg_ir::identity_key!("point-coedge"))
                .colon(ordinal),
        );
        ir.model.edges.push(Edge::new(
            edge_id.clone(),
            cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
            vertex_id.clone(),
            vertex_id,
            None,
        ));
        ir.model.coedges.push({
            let (id, owner_loop, edge, radial_next, sense, use_curve, pcurves) = (
                coedge_id.clone(),
                loop_id.clone(),
                edge_id,
                coedge_id.clone(),
                Sense::Forward,
                None,
                Vec::new(),
            );
            Coedge::new(id, owner_loop, edge, radial_next, sense, pcurves, use_curve)
        });
        ir.model.loops.push(Loop::new(
            loop_id.clone(),
            face_id.clone(),
            cadmpeg_ir::topology::LoopBoundary::Ring(cadmpeg_ir::topology::LoopRing::single(
                coedge_id,
            )),
        ));
        face_loops.push(loop_id);
    }
    if face_loops.is_empty() {
        return Err(cadmpeg_core::CodecError::UnsupportedContent(format!(
            "source-less SLDPRT sketch {} has no profiles",
            sketch.id.as_str()
        )));
    }
    ir.model.faces.push(
        Face::new(
            face_id.clone(),
            shell_id.clone(),
            surface_id,
            Sense::Forward,
            cadmpeg_ir::topology::FaceLoops::unspecified(face_loops),
        )
        .with_name(sketch.name.clone()),
    );
    ir.model.shells.push(Shell::with_face(
        shell_id.clone(),
        region_id.clone(),
        face_id,
    ));
    ir.model.regions.push(Region::new(
        region_id.clone(),
        body_id.clone(),
        vec![shell_id],
    ));
    ir.model.bodies.push(Body::new(
        body_id,
        BodyKind::Sheet,
        vec![region_id],
        None,
        sketch.name.clone(),
        None,
        None,
    ));
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
    let (origin, normal, u_axis) = sketch
        .resolved_placement()
        .map(|(origin, normal, u_axis)| (origin.get(), normal.get(), u_axis.get()))
        .ok_or_else(|| {
            cadmpeg_core::CodecError::UnsupportedContent(format!(
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
            let origin = lift(start.get());
            let target = lift(end.get());
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
                curve: CurveGeometry::Solved(SolvedCurveGeometry::Line(cadmpeg_ir::geometry::analytic::LineCurve::try_new(origin, Vector3::new(
                        delta.x / length,
                        delta.y / length,
                        delta.z / length,
                    )).map_err(cadmpeg_core::CodecError::malformed)?)),
                start: start.get(),
                end: end.get(),
                param_range: [0.0, length],
            })
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            let point = offset_point(center.get(), Point2::new(radius.get(), 0.0));
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Solved(
                    SolvedCurveGeometry::Circle(
                        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                            lift(center.get()), normal, u_axis, radius.get()
                        ).map_err(cadmpeg_core::CodecError::malformed)?
                    )
                ),
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
            curve: CurveGeometry::Solved(
                SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        lift(center.get()), normal, u_axis, radius.get()
                    ).map_err(cadmpeg_core::CodecError::malformed)?
                )
            ),
            start: offset_point(center.get(), polar(radius.get(), start_angle.get())),
            end: offset_point(center.get(), polar(radius.get(), end_angle.get())),
            param_range: [start_angle.get(), end_angle.get()],
        }),
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            let major_radius = radii.major();
            let minor_radius = radii.minor();
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
            let frame = cadmpeg_ir::units::OrthonormalFrame3::new(
                normal,
                vector(major_angle.get().cos(), major_angle.get().sin()),
            )
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("EllipseCurve.axis/major_direction must form an orthonormal frame"))?;
            let center_3d = cadmpeg_ir::features::FinitePoint3::new(lift(center.get()))
                .ok_or_else(|| cadmpeg_core::CodecError::malformed("EllipseCurve.center must be finite"))?;
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(cadmpeg_ir::geometry::analytic::EllipseCurve::new(center_3d, frame, *radii))),
                start: point(start),
                end: if full { point(start) } else { point(end) },
                param_range: [start, end],
            })
        }
        SketchGeometryDefinition::Nurbs { curve } => {
            if curve.periodic() {
                return Err(cadmpeg_core::CodecError::UnsupportedContent(
                    "source-less SLDPRT sketch writing requires a non-periodic NURBS with at least two poles".into(),
                ));
            }
            let control_points = curve.control_points();
            let start = control_points[0];
            let end = control_points[control_points.len() - 1];
            let knots = curve.knots();
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve.lift(lift).map_err(|error| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "source-less SLDPRT sketch NURBS lift is invalid: {error}"
                    ))
                })?)),
                start: start.get(),
                end: end.get(),
                param_range: [knots[cadmpeg_core::decode::index_from_u32(curve.degree())], knots[control_points.len()]],
            })
        }
        SketchGeometryDefinition::Point { .. }
        | SketchGeometryDefinition::Text { .. }
        | SketchGeometryDefinition::ReferenceLine { .. }
        | SketchGeometryDefinition::Hyperbola { .. }
        | SketchGeometryDefinition::Parabola { .. }
        | SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. } => Err(
            cadmpeg_core::CodecError::UnsupportedContent(
                "source-less SLDPRT sketch writing does not support point or native-only profile entities".into(),
            ),
        ),
    }
}

fn sketch_vertex(
    ir: &mut cadmpeg_ir::CadIr,
    vertices: &mut HashMap<(u64, u64), VertexId>,
    prefix: &cadmpeg_ir::ids::IdentityKey,
    position: Point2,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
) -> Result<VertexId, cadmpeg_core::CodecError> {
    if let Some((_, id)) = vertices.iter().find(|((u, v), _)| {
        same_sketch_point(
            Point2::new(f64::from_bits(*u), f64::from_bits(*v)),
            position,
        )
    }) {
        return Ok(id.clone());
    }
    let key = (position.u.to_bits(), position.v.to_bits());
    let ordinal = vertices.len();
    let point_id = PointId::compose(
        &sketch_namespace(),
        prefix
            .clone()
            .colon(cadmpeg_ir::identity_key!("point"))
            .colon(ordinal),
    );
    let vertex_id = VertexId::compose(
        &sketch_namespace(),
        prefix
            .clone()
            .colon(cadmpeg_ir::identity_key!("vertex"))
            .colon(ordinal),
    );
    let finite_position =
        cadmpeg_ir::features::FinitePoint3::new(lift_point(position, origin, u_axis, v_axis))
            .ok_or(Point::NON_FINITE_POSITION)
            .map_err(cadmpeg_core::CodecError::malformed)?;
    ir.model
        .points
        .push(Point::new(point_id.clone(), finite_position, None));
    ir.model
        .vertices
        .push(Vertex::new(vertex_id.clone(), point_id, None));
    vertices.insert(key, vertex_id.clone());
    Ok(vertex_id)
}

pub(super) fn same_sketch_point(left: Point2, right: Point2) -> bool {
    (left.u - right.u).abs() <= SKETCH_POINT_TOLERANCE
        && (left.v - right.v).abs() <= SKETCH_POINT_TOLERANCE
}

fn lift_point(point: Point2, origin: Point3, u_axis: Vector3, v_axis: Vector3) -> Point3 {
    Point3::new(
        origin.x + point.u * u_axis.x + point.v * v_axis.x,
        origin.y + point.u * u_axis.y + point.v * v_axis.y,
        origin.z + point.u * u_axis.z + point.v * v_axis.z,
    )
}

fn polar(radius: f64, angle: f64) -> Point2 {
    Point2::new(radius * angle.cos(), radius * angle.sin())
}

fn offset_point(origin: Point2, delta: Point2) -> Point2 {
    Point2::new(origin.u + delta.u, origin.v + delta.v)
}
