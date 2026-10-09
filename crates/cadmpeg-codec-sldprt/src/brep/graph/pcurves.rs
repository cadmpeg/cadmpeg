// SPDX-License-Identifier: Apache-2.0
//! Resolve support charts and derive coedge pcurves.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::Exactness;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::eval::analytic_surface_parameters;
use cadmpeg_ir::eval::nurbs_curve_parameter_domain;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis;
use cadmpeg_ir::geometry::pcurve::Pcurve;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::geometry::pcurve::PolarPcurveNurbs;
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::ids::VertexId;
use cadmpeg_ir::topology::Coedge;
use cadmpeg_ir::topology::Edge;
use crate::brep::LEN_TO_MM;
use crate::brep::evaluation::nurbs_surface_parameter_segment_chord_bound;
use crate::brep::evaluation::nurbs_surface_parameter_within_tolerance;
use crate::brep::graph::cylindrical_radial::CylindricalRadialControls;
use std::collections::HashMap;
use super::Brep;
use super::EPS_AXIS_ALIGNMENT;
use super::EPS_CIRCLE_RADIUS_MATCH;
use super::EPS_GEOMETRY_RESIDUAL;
use super::EPS_NORMAL_NONZERO;
use super::EPS_PLANAR_DISTANCE;
use super::EPS_RADIUS_ABSOLUTE;
use super::EPS_RADIUS_RELATIVE;
use super::inverse::InverseResolution;
use super::nurbs_pcurves::NurbsPcurveFailure;
use super::nurbs_pcurves::NurbsPcurveResolution;
use super::Stats;
use super::admit_brep_entity;
use super::nurbs_pcurves::derive_nurbs_edge_pcurve;
use super::inverse::inverse_coordinate_tolerance;
use super::nurbs_pcurves::nurbs_edge_parameter_range;
use super::inverse::nurbs_parameter_at_point;
use super::pcurve_namespace;
use super::nurbs_pcurves::resolve_axis_candidates;
use super::inverse::ruled_surface_line_pcurve;

/// The support surface, edge and edge curve of one coedge.
struct CoedgeSupport<'a> {
    surface: &'a Surface,
    edge: &'a Edge,
    curve: &'a Curve,
}

/// The arenas the pcurve passes read, resolved once for all of them. No pass
/// changes a loop, face, surface, edge, curve, vertex or point.
struct PcurveSupports<'a, 'ctx> {
    /// The support of each coedge, in coedge order; `None` when its loop,
    /// face, surface, edge or curve does not resolve.
    coedges: Vec<Option<CoedgeSupport<'a>>>,
    /// The position of each vertex whose point resolves.
    vertex_positions: HashMap<&'a VertexId, cadmpeg_ir::math::Point3>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

/// Read-only arenas whose references remain valid across all pcurve passes.
struct PcurveArenas<'a> {
    loops: &'a [cadmpeg_ir::topology::Loop],
    faces: &'a [cadmpeg_ir::topology::Face],
    surfaces: &'a [Surface],
    edges: &'a [Edge],
    curves: &'a [Curve],
    vertices: &'a [cadmpeg_ir::topology::Vertex],
    points: &'a [cadmpeg_ir::topology::Point],
}

impl<'a, 'ctx> PcurveSupports<'a, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        arenas: PcurveArenas<'a>,
        coedges: &[Coedge],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        const LOOK_UP: &str = "look up Parasolid pcurve supports";
        let PcurveArenas { loops, faces, surfaces, edges, curves, vertices, points } = arenas;
        let (supports, workspace) = ctx.with_scoped_storage(
            "index Parasolid pcurve supports", || {
                let surfaces = ctx.collect_hash_map(
                    surfaces.iter().map(|surface| (&surface.id, surface)),
                    "index Parasolid pcurve surfaces",
                )?;
                let faces = ctx.collect_hash_map(
                    faces.iter().map(|face| (&face.id, face)),
                    "index Parasolid pcurve faces",
                )?;
                let mut loop_surfaces = HashMap::new();
                for lp in ctx.admit_iter(loops, "index Parasolid pcurve loop surfaces")? {
                    let Some(face) = ctx.get_hash_map(&faces, &lp.face, LOOK_UP)? else {
                        continue;
                    };
                    let Some(&surface) = ctx.get_hash_map(&surfaces, &face.surface, LOOK_UP)? else {
                        continue;
                    };
                    ctx.insert_hash_map(
                        &mut loop_surfaces,
                        &lp.id,
                        surface,
                        "index Parasolid pcurve loop surfaces",
                    )?;
                }
                let edges = ctx.collect_hash_map(
                    edges.iter().map(|edge| (&edge.id, edge)),
                    "index Parasolid pcurve edges",
                )?;
                let curves = ctx.collect_hash_map(
                    curves.iter().map(|curve| (&curve.id, curve)),
                    "index Parasolid pcurve curves",
                )?;
                let (coedge_supports, storage) = ctx.with_scoped_storage(
                    "hold Parasolid pcurve supports", || ctx.try_collect_vec(
                    coedges.iter().map(|coedge| {
                        let Some(&surface) =
                            ctx.get_hash_map(&loop_surfaces, &coedge.owner_loop, LOOK_UP)?
                        else {
                            return Ok(None);
                        };
                        let Some(&edge) = ctx.get_hash_map(&edges, &coedge.edge, LOOK_UP)? else {
                            return Ok(None);
                        };
                        let Some(curve_id) = edge.curve() else {
                            return Ok(None);
                        };
                        let Some(&curve) = ctx.get_hash_map(&curves, curve_id, LOOK_UP)? else {
                            return Ok(None);
                        };
                        Ok::<_, cadmpeg_core::CodecError>(Some(CoedgeSupport {
                            surface,
                            edge,
                            curve,
                        }))
                    }),
                    "resolve Parasolid pcurve supports",
                ))?;
                let mut supports = Self {
                    coedges: coedge_supports,
                    vertex_positions: HashMap::new(),
                    _storage: storage,
                };
                let points = ctx.collect_hash_map(
                    points
                        .iter()
                        .map(|point| (&point.id, point.position().get())),
                    "index Parasolid pcurve points",
                )?;
                for vertex in ctx.admit_iter(vertices, "index Parasolid pcurve vertex points")? {
                    if let Some(&position) = ctx.get_hash_map(&points, &vertex.point, LOOK_UP)? {
                        supports._storage.with_storage(|| ctx.insert_hash_map(
                            &mut supports.vertex_positions,
                            &vertex.id,
                            position,
                            "index Parasolid pcurve vertex points",
                        ))?;
                    }
                }
                Ok::<_, cadmpeg_core::CodecError>(supports)
            },
        )?;
        drop(workspace);
        Ok(supports)
    }

    fn position(
        &self,
        ctx: &DecodeContext<'_>,
        vertex: &VertexId,
    ) -> Result<Option<cadmpeg_ir::math::Point3>, cadmpeg_core::CodecError> {
        Ok(ctx
            .get_hash_map(&self.vertex_positions, vertex, "look up SLDPRT hash key")?
            .copied())
    }
}

/// The arenas the pcurve passes write.
struct PcurveSink<'o> {
    coedges: &'o mut [Coedge],
    pcurves: &'o mut Vec<Pcurve>,
    losses: &'o mut Vec<cadmpeg_ir::report::loss::LossNote>,
    stats: &'o mut Stats,
}

impl PcurveSink<'_> {
    /// Make `pcurve` the one pcurve use of the coedge at `index`.
    fn bind(
        &mut self,
        ctx: &DecodeContext<'_>,
        index: usize,
        pcurve: PcurveId,
        parameter_range: Option<cadmpeg_ir::geometry::DirectedParameterRange>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if let Some(coedge) = self.coedges.get_mut(index) {
            coedge.pcurves = ctx.collect_retained_vec(
                [cadmpeg_ir::topology::PcurveUse {
                    pcurve,
                    isoparametric: None,
                    parameter_range,
                }],
                "bind derived Parasolid pcurve",
            )?;
        }
        Ok(())
    }
}

/// Derive planar, cylindrical, revolved-circle, spherical and NURBS pcurves,
/// in that order. A later pass skips a coedge an earlier pass bound.
pub(super) fn derive_pcurves(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let Brep {
        loops,
        faces,
        surfaces,
        edges,
        curves,
        vertices,
        points,
        coedges,
        pcurves,
        losses,
        stats,
        ..
    } = out;
    let (loops, faces, surfaces, edges, curves, vertices, points) = (
        &**loops,
        &**faces,
        &**surfaces,
        &**edges,
        &**curves,
        &**vertices,
        &**points,
    );
    let supports = PcurveSupports::new(
        ctx,
        PcurveArenas { loops, faces, surfaces, edges, curves, vertices, points },
        coedges,
    )?;
    let mut sink = PcurveSink {
        coedges,
        pcurves,
        losses,
        stats,
    };
    derive_planar_pcurves(ctx, &supports, &mut sink, annotations, source_stream)?;
    derive_cylindrical_pcurves(ctx, &supports, &mut sink, annotations, source_stream)?;
    derive_revolved_circle_pcurves(ctx, &supports, &mut sink, annotations, source_stream)?;
    derive_spherical_pcurves(ctx, &supports, &mut sink, annotations, source_stream)?;
    derive_nurbs_isoparametric_pcurves(ctx, &supports, &mut sink, annotations, source_stream)
}

fn derive_planar_pcurves(
    ctx: &DecodeContext<'_>,
    supports: &PcurveSupports<'_, '_>,
    sink: &mut PcurveSink<'_>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut derived = Vec::new();
    for (index, (coedge, support)) in ctx
        .admit_iter(&*sink.coedges, "derive Parasolid planar pcurves")?
        .zip(&supports.coedges)
        .enumerate()
    {
        let Some(CoedgeSupport { surface, curve, .. }) = support else {
            continue;
        };
        if !matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
        ) {
            continue;
        }
        let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved() else {
            continue;
        };
        let origin = plane_surface.origin().get();
        let normal = *plane_surface.frame().axis().as_raw();
        let u_reference = *plane_surface.frame().reference().as_raw();
        let v_reference = cadmpeg_ir::math::Vector3::new(
            normal.y * u_reference.z - normal.z * u_reference.y,
            normal.z * u_reference.x - normal.x * u_reference.z,
            normal.x * u_reference.y - normal.y * u_reference.x,
        );
        let uv = |point: cadmpeg_ir::math::Point3| {
            let d = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
            cadmpeg_ir::math::Point2::new(
                d[0] * u_reference.x + d[1] * u_reference.y + d[2] * u_reference.z,
                d[0] * v_reference.x + d[1] * v_reference.y + d[2] * v_reference.z,
            )
        };
        let project_direction = |direction: cadmpeg_ir::math::Vector3| {
            let projected = cadmpeg_ir::math::Point2::new(
                direction.x * u_reference.x
                    + direction.y * u_reference.y
                    + direction.z * u_reference.z,
                direction.x * v_reference.x
                    + direction.y * v_reference.y
                    + direction.z * v_reference.z,
            );
            let norm = (projected.u * projected.u + projected.v * projected.v).sqrt();
            (norm > EPS_NORMAL_NONZERO)
                .then(|| cadmpeg_ir::math::Point2::new(projected.u / norm, projected.v / norm))
        };
        let plane_distance = |point: cadmpeg_ir::math::Point3| {
            (point.x - origin.x) * normal.x
                + (point.y - origin.y) * normal.y
                + (point.z - origin.z) * normal.z
        };
        let geometry = match &curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                let curve_origin = line_curve.origin().get();
                let direction = *line_curve.direction().as_raw();
                if plane_distance(curve_origin).abs() > EPS_PLANAR_DISTANCE
                    || (direction.x * normal.x + direction.y * normal.y + direction.z * normal.z)
                        .abs()
                        > EPS_GEOMETRY_RESIDUAL
                {
                    continue;
                }
                let Some(direction) = project_direction(direction) else {
                    continue;
                };
                PcurveGeometry::Line(
                    match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                        uv(curve_origin),
                        direction,
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                )
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center().get();
                let axis = circle_curve.frame().axis().as_raw();
                let ref_direction = circle_curve.frame().reference().as_raw();
                let radius = circle_curve.radius().get();
                let axis_dot = axis.x * normal.x + axis.y * normal.y + axis.z * normal.z;
                if axis_dot.abs() < 1.0 - EPS_AXIS_ALIGNMENT
                    || plane_distance(center).abs() > EPS_PLANAR_DISTANCE
                {
                    continue;
                }
                let Some(ref_direction) = project_direction(*ref_direction) else {
                    continue;
                };
                PcurveGeometry::Circle(
                    match cadmpeg_ir::geometry::pcurve::CirclePcurve::try_new(
                        uv(center),
                        ref_direction,
                        if axis_dot < 0.0 {
                            cadmpeg_ir::math::Point2::new(ref_direction.v, -ref_direction.u)
                        } else {
                            cadmpeg_ir::math::Point2::new(-ref_direction.v, ref_direction.u)
                        },
                        radius,
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                )
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                let center = ellipse_curve.center().get();
                let axis = ellipse_curve.frame().axis().as_raw();
                let major_direction = ellipse_curve.frame().reference().as_raw();
                let major_radius = ellipse_curve.major_radius().get();
                let minor_radius = ellipse_curve.minor_radius().get();
                let axis_dot = axis.x * normal.x + axis.y * normal.y + axis.z * normal.z;
                if axis_dot.abs() < 1.0 - EPS_AXIS_ALIGNMENT
                    || plane_distance(center).abs() > EPS_PLANAR_DISTANCE
                {
                    continue;
                }
                let Some(major_direction) = project_direction(*major_direction) else {
                    continue;
                };
                PcurveGeometry::Ellipse(match cadmpeg_ir::geometry::pcurve::EllipsePcurve::try_new(
                    uv(center),
                    major_direction,
                    if axis_dot < 0.0 {
                        cadmpeg_ir::math::Point2::new(major_direction.v, -major_direction.u)
                    } else {
                        cadmpeg_ir::math::Point2::new(-major_direction.v, major_direction.u)
                    },
                    major_radius,
                    minor_radius,
                ) {
                    Ok(payload) => payload,
                    Err(_) => continue,
                })
            }
            _ => continue,
        };
        let id = PcurveId::compose(&pcurve_namespace(), coedge.id.key());
        let pcurve = Pcurve {
            id: id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
            geometry,
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
        };
        ctx.push_vec(
            &mut derived,
            (index, id, pcurve),
            "collect derived Parasolid pcurves",
        )?;
    }
    for (index, id, pcurve) in ctx.admit_iter(derived, "bind derived Parasolid pcurves")? {
        annotations.note(
            ctx,
            id.as_str(),
            source_stream,
            0,
            Some("derived_planar_pcurve"),
        )?;
        annotations.exactness(ctx, id.as_str(), Exactness::Derived)?;
        sink.bind(ctx, index, id, None)?;
        admit_brep_entity(ctx)?;
        ctx.push_vec(sink.pcurves, pcurve, "collect derived Parasolid pcurves")?;
    }
    Ok(())
}

fn derive_cylindrical_pcurves(
    ctx: &DecodeContext<'_>,
    supports: &PcurveSupports<'_, '_>,
    sink: &mut PcurveSink<'_>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut refusals = Vec::new();
    let position = |vertex_id: &VertexId| supports.position(ctx, vertex_id);
    let mut derived = Vec::new();
    for (index, (coedge, support)) in ctx
        .admit_iter(&*sink.coedges, "derive Parasolid cylindrical pcurves")?
        .zip(&supports.coedges)
        .enumerate()
    {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(CoedgeSupport {
            surface,
            edge,
            curve,
        }) = support
        else {
            continue;
        };
        let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
        else {
            continue;
        };
        let origin = cylinder_surface.origin().get();
        let axis = cylinder_surface.frame().axis().as_raw();
        let u_reference = cylinder_surface.frame().reference().as_raw();
        let radius = cylinder_surface.radius().get();
        let cross = cadmpeg_ir::math::Vector3::new(
            axis.y * u_reference.z - axis.z * u_reference.y,
            axis.z * u_reference.x - axis.x * u_reference.z,
            axis.x * u_reference.y - axis.y * u_reference.x,
        );
        let dot = |a: [f64; 3], b: cadmpeg_ir::math::Vector3| a[0] * b.x + a[1] * b.y + a[2] * b.z;
        let mut parameter_range = None;
        let geometry = match &curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))
                if {
                    let circle_axis = circle_curve.frame().axis().as_raw();
                    let circle_radius = circle_curve.radius().get();
                    (circle_radius.abs() - radius.abs()).abs() < EPS_CIRCLE_RADIUS_MATCH
                        && (circle_axis.x * axis.x
                            + circle_axis.y * axis.y
                            + circle_axis.z * axis.z)
                            .abs()
                            > 1.0 - EPS_AXIS_ALIGNMENT
                } =>
            {
                let center = circle_curve.center().get();
                let circle_axis = circle_curve.frame().axis().as_raw();
                let circle_reference = circle_curve.frame().reference().as_raw();
                let d = [
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                ];
                let axial = dot(d, *axis);
                let radial = [
                    d[0] - axial * axis.x,
                    d[1] - axial * axis.y,
                    d[2] - axial * axis.z,
                ];
                if dot(
                    radial,
                    cadmpeg_ir::math::Vector3::new(radial[0], radial[1], radial[2]),
                )
                .sqrt()
                    > EPS_PLANAR_DISTANCE
                {
                    continue;
                }
                let Some((phase, sense)) =
                    circle_azimuth_parameter(*axis, *u_reference, *circle_axis, *circle_reference)
                else {
                    continue;
                };
                PcurveGeometry::Line(
                    match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                        cadmpeg_ir::math::Point2::new(phase, axial),
                        cadmpeg_ir::math::Point2::new(sense, 0.0),
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                )
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve))
                if {
                    let direction = *line_curve.direction().as_raw();
                    (direction.x * axis.x + direction.y * axis.y + direction.z * axis.z).abs()
                        > 1.0 - EPS_AXIS_ALIGNMENT
                } =>
            {
                let direction = *line_curve.direction().as_raw();
                let Some(start) = position(&edge.start)? else {
                    continue;
                };
                let d = [start.x - origin.x, start.y - origin.y, start.z - origin.z];
                let v = dot(d, *axis);
                let radial = [d[0] - v * axis.x, d[1] - v * axis.y, d[2] - v * axis.z];
                let u = dot(radial, cross).atan2(dot(radial, *u_reference));
                PcurveGeometry::Line(
                    match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                        cadmpeg_ir::math::Point2::new(u, v),
                        cadmpeg_ir::math::Point2::new(
                            0.0,
                            if dot([direction.x, direction.y, direction.z], *axis) >= 0.0 {
                                1.0
                            } else {
                                -1.0
                            },
                        ),
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                )
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                let center = ellipse_curve.center().get();
                let ellipse_axis = ellipse_curve.frame().axis().as_raw();
                let major_direction = ellipse_curve.frame().reference().as_raw();
                let major_radius = ellipse_curve.major_radius().get();
                let minor_radius = ellipse_curve.minor_radius().get();
                let minor_direction = cadmpeg_ir::math::Vector3::new(
                    ellipse_axis.y * major_direction.z - ellipse_axis.z * major_direction.y,
                    ellipse_axis.z * major_direction.x - ellipse_axis.x * major_direction.z,
                    ellipse_axis.x * major_direction.y - ellipse_axis.y * major_direction.x,
                );
                let relative = [
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                ];
                let major = [
                    major_radius * major_direction.x,
                    major_radius * major_direction.y,
                    major_radius * major_direction.z,
                ];
                let minor = [
                    minor_radius * minor_direction.x,
                    minor_radius * minor_direction.y,
                    minor_radius * minor_direction.z,
                ];
                let radial_center = cadmpeg_ir::math::Point2::new(
                    dot(relative, *u_reference),
                    dot(relative, cross),
                );
                let radial_cos =
                    cadmpeg_ir::math::Point2::new(dot(major, *u_reference), dot(major, cross));
                let radial_sin =
                    cadmpeg_ir::math::Point2::new(dot(minor, *u_reference), dot(minor, cross));
                let norm = |value: cadmpeg_ir::math::Point2| value.u.hypot(value.v);
                let product = |a: cadmpeg_ir::math::Point2, b: cadmpeg_ir::math::Point2| {
                    a.u * b.u + a.v * b.v
                };
                let tolerance = EPS_RADIUS_ABSOLUTE.max(radius.abs() * EPS_RADIUS_RELATIVE);
                if norm(radial_center) > tolerance
                    || (norm(radial_cos) - radius.abs()).abs() > tolerance
                    || (norm(radial_sin) - radius.abs()).abs() > tolerance
                    || product(radial_cos, radial_sin).abs() > tolerance * radius.abs()
                {
                    continue;
                }
                PcurveGeometry::PolarHarmonic(
                    match cadmpeg_ir::geometry::pcurve::PolarHarmonicPcurve::try_new(
                        radial_center,
                        radial_cos,
                        radial_sin,
                        dot(relative, *axis),
                        dot(major, *axis),
                        dot(minor, *axis),
                    ) {
                        Ok(payload) => payload,
                        Err(_) => continue,
                    },
                )
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                let project = |point: cadmpeg_ir::math::Point3| {
                    let relative = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
                    cadmpeg_ir::math::Point2::new(dot(relative, *u_reference), dot(relative, cross))
                };
                let radial_control_points = CylindricalRadialControls::new(ctx, nurbs, project)?;
                if !radial_control_points.has_constant_radius(cylinder_surface.radius())? {
                    continue;
                }
                parameter_range = if let Some(range) = edge.param_range() {
                    Some(range.get())
                } else {
                    let (Some(start), Some(end)) = (position(&edge.start)?, position(&edge.end)?)
                    else {
                        continue;
                    };
                    match (
                        nurbs_parameter_at_point(ctx, nurbs, start)?,
                        nurbs_parameter_at_point(ctx, nurbs, end)?,
                    ) {
                        (InverseResolution::Unique(start), InverseResolution::Unique(end)) => {
                            Some([start.min(end), start.max(end)])
                        }
                        (InverseResolution::Ambiguous, _) | (_, InverseResolution::Ambiguous) => {
                            sink.stats.ambiguous_pcurve_parameters += 1;
                            continue;
                        }
                        _ => continue,
                    }
                };
                // One pole row carries its radial and axial halves together,
                // so the two projections are built into one list.
                let mut poles = Vec::new();
                ctx.reserve_capacity(
                    &mut poles,
                    nurbs.pole_count(),
                    "collect cylindrical polar poles",
                )?;
                for (index, radial) in ctx
                    .admit_iter(
                        radial_control_points.values(),
                        "scan SLDPRT derive_cylindrical_pcurves values",
                    )?
                    .enumerate()
                {
                    let Some(point) = nurbs.pole_rows().point_at(index) else {
                        continue;
                    };
                    ctx.push_vec(
                        &mut (poles),
                        cadmpeg_ir::geometry::pcurve::PolarNurbsPole {
                            radial: *radial,
                            axial: dot(
                                [point.x - origin.x, point.y - origin.y, point.z - origin.z],
                                *axis,
                            ),
                        },
                        "collect cylindrical polar poles",
                    )?;
                }
                drop(radial_control_points);
                let knots = nurbs
                    .knots()
                    .try_clone_for_decode(ctx, "copy cylindrical polar knots")?;
                let weights = if let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } =
                    nurbs.pole_rows()
                {
                    Some(ctx.collect_vec(
                        points.iter().map(|pole| pole.weight),
                        "copy cylindrical polar weights",
                    )?)
                } else {
                    None
                };
                let polar = match PolarPcurveNurbs::from_checked_lanes(
                    ctx,
                    nurbs.degree(),
                    knots,
                    poles,
                    weights,
                    nurbs.periodic(),
                )? {
                    Ok(polar) => polar,
                    Err(error) => {
                        let note = crate::loss::spline_lane_refusal(
                            ctx,
                            format_args!("cylindrical pcurve for edge {}: {error}", edge.id),
                        )?;
                        ctx.push_vec(&mut refusals, note, "collect cylindrical pcurve losses")?;
                        continue;
                    }
                };
                PcurveGeometry::PolarNurbs { nurbs: polar }
            }
            _ => continue,
        };
        let id = PcurveId::compose(
            &pcurve_namespace(),
            cadmpeg_ir::identity_key!("cylinder:").then(coedge.id.key()),
        );
        let parameter_range = match parameter_range.map(cadmpeg_ir::units::FiniteVector::new) {
            Some(None) => continue,
            Some(Some(range)) => Some(range),
            None => None,
        };
        let pcurve = Pcurve {
            id: id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
            geometry,
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                None,
                parameter_range,
                None,
            ),
        };
        ctx.push_vec(
            &mut derived,
            (index, id, pcurve),
            "collect derived Parasolid pcurves",
        )?;
    }
    for (index, id, pcurve) in ctx.admit_iter(derived, "bind derived Parasolid pcurves")? {
        annotations.note(
            ctx,
            id.as_str(),
            source_stream,
            0,
            Some("derived_cylindrical_pcurve"),
        )?;
        annotations.exactness(ctx, id.as_str(), Exactness::Derived)?;
        sink.bind(ctx, index, id, None)?;
        admit_brep_entity(ctx)?;
        ctx.push_vec(sink.pcurves, pcurve, "collect derived Parasolid pcurves")?;
    }
    ctx.append_vec(sink.losses, &mut refusals, "move cylindrical pcurve losses")?;
    Ok(())
}

pub(super) fn quadratic_nurbs_has_constant_radius(
    ctx: &DecodeContext<'_>,
    radial_control_points: &[cadmpeg_ir::math::Point2],
    weights: Option<&[f64]>,
    knots: &[f64],
    radius: cadmpeg_ir::scalar::PositiveLength,
) -> Result<bool, cadmpeg_core::CodecError> {
    if radial_control_points.len() < 3
        || radial_control_points.len().is_multiple_of(2)
        || knots.len() != radial_control_points.len() + 3
        || weights.is_some_and(|weights| weights.len() != radial_control_points.len())
    {
        return Ok(false);
    }
    let radius = radius.get();
    let mut runs = 0usize;
    let mut previous = 0.0;
    let mut count = 0usize;
    for knot in ctx.admit_iter(knots, "scan Parasolid constant radius knot runs")? {
        if !knot.is_finite() {
            return Ok(false);
        }
        if runs == 0 {
            previous = *knot;
            count = 1;
            runs = 1;
        } else if *knot == previous {
            count += 1;
        } else {
            if *knot <= previous || count != if runs == 1 { 3 } else { 2 } {
                return Ok(false);
            }
            previous = *knot;
            count = 1;
            runs += 1;
        }
    }
    if runs < 2 || count != 3 || runs - 1 != (radial_control_points.len() - 1) / 2 {
        return Ok(false);
    }
    let weight = |index: usize| weights.map_or(1.0, |weights| weights[index]);
    let choose_2 = [1.0, 2.0, 1.0];
    let choose_4 = [1.0, 4.0, 6.0, 4.0, 1.0];
    let tolerance = EPS_RADIUS_ABSOLUTE.max(radius * radius * EPS_RADIUS_RELATIVE);
    // Each quadratic segment checks a constant number of coefficients.
    for start in ctx
        .admit_iter(
            0..radial_control_points.len() - 1,
            "check Parasolid constant radius segments",
        )?
        .step_by(2)
    {
        let homogeneous = [0, 1, 2].map(|offset| {
            let weight = weight(start + offset);
            let point = radial_control_points[start + offset];
            (point.u * weight, point.v * weight, weight)
        });
        for (degree, &denominator) in choose_4.iter().enumerate() {
            let mut identity = 0.0_f64;
            for i in 0usize..=2 {
                let Some(j) = degree.checked_sub(i) else {
                    continue;
                };
                if j > 2 {
                    continue;
                }
                let factor = choose_2[i] * choose_2[j] / denominator;
                identity += factor
                    * (homogeneous[i].0 * homogeneous[j].0 + homogeneous[i].1 * homogeneous[j].1
                        - radius * radius * homogeneous[i].2 * homogeneous[j].2);
            }
            if identity.abs() > tolerance {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn circle_azimuth_parameter(
    surface_axis: cadmpeg_ir::math::Vector3,
    surface_reference: cadmpeg_ir::math::Vector3,
    circle_axis: cadmpeg_ir::math::Vector3,
    circle_reference: cadmpeg_ir::math::Vector3,
) -> Option<(f64, f64)> {
    let axis_dot = surface_axis.x * circle_axis.x
        + surface_axis.y * circle_axis.y
        + surface_axis.z * circle_axis.z;
    if axis_dot.abs() < 1.0 - EPS_AXIS_ALIGNMENT {
        return None;
    }
    let surface_tangent = cadmpeg_ir::math::Vector3::new(
        surface_axis.y * surface_reference.z - surface_axis.z * surface_reference.y,
        surface_axis.z * surface_reference.x - surface_axis.x * surface_reference.z,
        surface_axis.x * surface_reference.y - surface_axis.y * surface_reference.x,
    );
    let phase = (circle_reference.x * surface_tangent.x
        + circle_reference.y * surface_tangent.y
        + circle_reference.z * surface_tangent.z)
        .atan2(
            circle_reference.x * surface_reference.x
                + circle_reference.y * surface_reference.y
                + circle_reference.z * surface_reference.z,
        );
    Some((phase, axis_dot.signum()))
}

fn derive_revolved_circle_pcurves(
    ctx: &DecodeContext<'_>,
    supports: &PcurveSupports<'_, '_>,
    sink: &mut PcurveSink<'_>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let dot = |a: [f64; 3], b: cadmpeg_ir::math::Vector3| a[0] * b.x + a[1] * b.y + a[2] * b.z;
    let mut derived = Vec::new();
    for (index, (coedge, support)) in ctx
        .admit_iter(&*sink.coedges, "derive Parasolid revolved circle pcurves")?
        .zip(&supports.coedges)
        .enumerate()
    {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(CoedgeSupport { surface, curve, .. }) = support else {
            continue;
        };
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = &curve.geometry
        else {
            continue;
        };
        let circle_center = circle_curve.center().get();
        let circle_axis = circle_curve.frame().axis().as_raw();
        let circle_reference = circle_curve.frame().reference().as_raw();
        let circle_radius = circle_curve.radius().get();
        let (surface_axis, surface_reference, v) = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))
                if {
                    let ratio = cone_surface.ratio().get();
                    (ratio - 1.0).abs() < EPS_NORMAL_NONZERO
                } =>
            {
                let origin = cone_surface.origin().get();
                let axis = cone_surface.frame().axis().as_raw();
                let ref_direction = cone_surface.frame().reference().as_raw();
                let radius = cone_surface.radius().get();
                let half_angle = cone_surface.half_angle().get();
                let d = [
                    circle_center.x - origin.x,
                    circle_center.y - origin.y,
                    circle_center.z - origin.z,
                ];
                let v = dot(d, *axis);
                let radial = [d[0] - v * axis.x, d[1] - v * axis.y, d[2] - v * axis.z];
                let expected_radius = radius + v * half_angle.tan();
                if dot(
                    radial,
                    cadmpeg_ir::math::Vector3::new(radial[0], radial[1], radial[2]),
                )
                .sqrt()
                    > EPS_PLANAR_DISTANCE
                    || (circle_radius.abs() - expected_radius.abs()).abs() > EPS_CIRCLE_RADIUS_MATCH
                {
                    continue;
                }
                (*axis, *ref_direction, v)
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                let center = torus_surface.center().get();
                let axis = torus_surface.frame().axis().as_raw();
                let ref_direction = torus_surface.frame().reference().as_raw();
                let major_radius = torus_surface.major_radius().get();
                let minor_radius = torus_surface.minor_radius().get();
                let d = [
                    circle_center.x - center.x,
                    circle_center.y - center.y,
                    circle_center.z - center.z,
                ];
                let height = dot(d, *axis);
                let radial = [
                    d[0] - height * axis.x,
                    d[1] - height * axis.y,
                    d[2] - height * axis.z,
                ];
                if dot(
                    radial,
                    cadmpeg_ir::math::Vector3::new(radial[0], radial[1], radial[2]),
                )
                .sqrt()
                    > EPS_PLANAR_DISTANCE
                    || ((circle_radius.abs() - major_radius).hypot(height) - minor_radius.abs())
                        .abs()
                        > EPS_RADIUS_ABSOLUTE.max(minor_radius.abs() * EPS_RADIUS_RELATIVE)
                {
                    continue;
                }
                (
                    *axis,
                    *ref_direction,
                    height.atan2(circle_radius.abs() - major_radius),
                )
            }
            _ => continue,
        };
        let Some((phase, sense)) = circle_azimuth_parameter(
            surface_axis,
            surface_reference,
            *circle_axis,
            *circle_reference,
        ) else {
            continue;
        };
        let id = PcurveId::compose(
            &pcurve_namespace(),
            cadmpeg_ir::identity_key!("revolved-circle:").then(coedge.id.key()),
        );
        let geometry = PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(phase, v),
                cadmpeg_ir::math::Point2::new(sense, 0.0),
            ) {
                Ok(payload) => payload,
                Err(_) => continue,
            },
        );
        let pcurve = Pcurve {
            id: id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
            geometry,
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
        };
        ctx.push_vec(
            &mut derived,
            (index, id, pcurve),
            "collect derived Parasolid pcurves",
        )?;
    }
    for (index, id, pcurve) in ctx.admit_iter(derived, "bind derived Parasolid pcurves")? {
        annotations.note(
            ctx,
            id.as_str(),
            source_stream,
            0,
            Some("derived_revolved_circle_pcurve"),
        )?;
        annotations.exactness(ctx, id.as_str(), Exactness::Derived)?;
        sink.bind(ctx, index, id, None)?;
        admit_brep_entity(ctx)?;
        ctx.push_vec(sink.pcurves, pcurve, "collect derived Parasolid pcurves")?;
    }
    Ok(())
}

/// Answer the sphere latitude of a circle whose plane is normal to the sphere
/// axis, or refuse a plane the sphere does not carry.
///
/// `height` is the signed axial distance from the sphere center to the circle
/// plane, so a circle on the sphere has `height.abs() <= radius.abs()` and the
/// latitude is `asin(height / radius)`. The radius match that precedes this
/// compares the circle radius against `sqrt(radius^2 - height^2)`, which
/// saturates at zero, so a small circle passes that match at any height beyond
/// the pole. The height is therefore stated here against the same distance
/// tolerance, and only the excess that tolerance admits is mapped onto the
/// pole. A larger height states a circle on another sphere.
pub(super) fn sphere_latitude(height: f64, radius: f64) -> Option<f64> {
    let pole = radius.abs() + EPS_CIRCLE_RADIUS_MATCH;
    if !(-pole..=pole).contains(&height) {
        return None;
    }
    let sine = height / radius;
    if sine < -1.0 {
        return Some(-std::f64::consts::FRAC_PI_2);
    }
    if sine > 1.0 {
        return Some(std::f64::consts::FRAC_PI_2);
    }
    Some(sine.asin())
}

fn derive_spherical_pcurves(
    ctx: &DecodeContext<'_>,
    supports: &PcurveSupports<'_, '_>,
    sink: &mut PcurveSink<'_>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut derived = Vec::new();
    for (index, (coedge, support)) in ctx
        .admit_iter(&*sink.coedges, "derive Parasolid spherical pcurves")?
        .zip(&supports.coedges)
        .enumerate()
    {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(CoedgeSupport { surface, curve, .. }) = support else {
            continue;
        };
        let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) = surface.geometry.solved() else {
            continue;
        };
        let sphere_center = sphere_surface.center().get();
        let v_reference = *sphere_surface.frame().axis().as_raw();
        let u_reference = *sphere_surface.frame().reference().as_raw();
        let radius = sphere_surface.radius().get();
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = &curve.geometry
        else {
            continue;
        };
        let center = circle_curve.center().get();
        let axis = *circle_curve.frame().axis().as_raw();
        let circle_radius = circle_curve.radius().get();
        let axis_dot = axis.dot(v_reference);
        let reference = *circle_curve.frame().reference().as_raw();
        let tangent = v_reference.cross(u_reference);
        let offset = center.vector_from(sphere_center);
        // Allow rounding of the frame projections as well as the existing
        // absolute carrier fit tolerance; never form squared radii.
        let fit_tolerance = EPS_CIRCLE_RADIUS_MATCH + 64.0 * f64::EPSILON * radius;
        let (origin, direction) = if axis_dot.abs() > 1.0 - EPS_AXIS_ALIGNMENT {
            let height = offset.dot(v_reference);
            let transverse = cadmpeg_ir::math::Vector3::new(
                offset.x - height * v_reference.x,
                offset.y - height * v_reference.y,
                offset.z - height * v_reference.z,
            );
            let Some(latitude) = sphere_latitude(height, radius) else {
                continue;
            };
            let sine = (height / radius).clamp(-1.0, 1.0);
            let section_radius = radius * ((1.0 - sine.abs()) * (1.0 + sine.abs())).sqrt();
            if transverse.norm() > fit_tolerance
                || (section_radius - circle_radius).abs() > fit_tolerance
            {
                continue;
            }
            let phase = reference.dot(tangent).atan2(reference.dot(u_reference));
            (
                cadmpeg_ir::math::Point2::new(phase, latitude),
                cadmpeg_ir::math::Point2::new(axis_dot.signum(), 0.0),
            )
        } else if axis_dot.abs() < EPS_AXIS_ALIGNMENT
            && (circle_radius - radius).abs() <= fit_tolerance
            && offset.norm() <= fit_tolerance
        {
            // With equator = sphere_axis × circle_axis, increasing latitude
            // follows the circle's positive orientation. Its reference fixes
            // the initial latitude, including a reference at either pole.
            let Some(equator) = sphere_surface
                .frame()
                .axis()
                .finite_cross(*circle_curve.frame().axis())
                .unit_nonzero()
            else {
                continue;
            };
            let longitude = equator.dot(tangent).atan2(equator.dot(u_reference));
            let phase = reference.dot(v_reference).atan2(reference.dot(equator));
            (
                cadmpeg_ir::math::Point2::new(longitude, phase),
                cadmpeg_ir::math::Point2::new(0.0, 1.0),
            )
        } else {
            continue;
        };
        let Ok(line) = cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(origin, direction) else {
            continue;
        };
        let geometry = PcurveGeometry::Line(line);
        // Verify both frame axes and their opposite points before assigning a
        // derived support relation. Near-aligned frames still need a physical fit.
        let mut fits = true;
        for parameter in [
            0.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            -std::f64::consts::FRAC_PI_2,
        ] {
            let Some(uv) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::pcurve_uv(ctx, &geometry, parameter),
                )?)?
            else {
                fits = false;
                break;
            };
            let Some(lifted) =
                crate::brep::evaluation::surface_point(ctx, &surface.geometry, uv.u, uv.v)?
            else {
                fits = false;
                break;
            };
            let Some(curve_point) = cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
                    ctx,
                    &CurveGeometry::Solved(SolvedCurveGeometry::Circle(*circle_curve)),
                    parameter,
                ))?,
            )?
            else {
                fits = false;
                break;
            };
            if lifted.distance(curve_point.get()) > fit_tolerance {
                fits = false;
                break;
            }
        }
        if !fits {
            continue;
        }
        let id = PcurveId::compose(
            &pcurve_namespace(),
            cadmpeg_ir::identity_key!("sphere:").then(coedge.id.key()),
        );
        let pcurve = Pcurve {
            id: id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
            geometry,
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
        };
        ctx.push_vec(
            &mut derived,
            (index, id, pcurve),
            "collect derived Parasolid pcurves",
        )?;
    }
    for (index, id, pcurve) in ctx.admit_iter(derived, "bind derived Parasolid pcurves")? {
        annotations.note(
            ctx,
            id.as_str(),
            source_stream,
            0,
            Some("derived_spherical_pcurve"),
        )?;
        annotations.exactness(ctx, id.as_str(), Exactness::Derived)?;
        sink.bind(ctx, index, id, None)?;
        admit_brep_entity(ctx)?;
        ctx.push_vec(sink.pcurves, pcurve, "collect derived Parasolid pcurves")?;
    }
    Ok(())
}

fn derive_nurbs_isoparametric_pcurves(
    ctx: &DecodeContext<'_>,
    supports: &PcurveSupports<'_, '_>,
    sink: &mut PcurveSink<'_>,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut lane_refusals = crate::lane_refusal::LaneRefusals::new();
    let mut derived = Vec::new();
    for (index, (coedge, support)) in ctx
        .admit_iter(&*sink.coedges, "derive Parasolid NURBS pcurves")?
        .zip(&supports.coedges)
        .enumerate()
    {
        if !coedge.pcurves.is_empty() {
            continue;
        }
        let Some(CoedgeSupport {
            surface,
            edge,
            curve,
        }) = support
        else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) = &surface.geometry
        else {
            continue;
        };
        let curve = &curve.geometry;
        let endpoints = match (
            supports.position(ctx, &edge.start)?,
            supports.position(ctx, &edge.end)?,
        ) {
            (Some(start), Some(end)) => Some([start, end]),
            _ => None,
        };
        let (geometry, parameter_range, fit_tolerance, cache) = match curve {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) => {
                let Some(parameter_range) =
                    nurbs_edge_parameter_range(ctx, edge, curve, endpoints)?
                else {
                    continue;
                };
                let resolution =
                    match derive_nurbs_edge_pcurve(ctx, surface, curve, parameter_range) {
                        Ok(resolution) => resolution,
                        Err(NurbsPcurveFailure::Carrier(error)) => {
                            lane_refusals.note(
                                ctx,
                                format_args!("isoparametric pcurve for edge {}", edge.id.as_str()),
                                &error,
                            )?;
                            continue;
                        }
                        Err(NurbsPcurveFailure::Resource(limit)) => return Err(limit),
                    };
                match resolution {
                    NurbsPcurveResolution::Exact(geometry) => {
                        (geometry, Some(parameter_range), None, false)
                    }
                    NurbsPcurveResolution::Cache {
                        geometry,
                        fit_tolerance,
                    } => (geometry, Some(parameter_range), Some(fit_tolerance), true),
                    NurbsPcurveResolution::OffSurface => {
                        sink.stats.off_surface_nurbs_pcurves += 1;
                        continue;
                    }
                    NurbsPcurveResolution::Ambiguous => {
                        sink.stats.ambiguous_pcurve_parameters += 1;
                        continue;
                    }
                    NurbsPcurveResolution::NoMatch => continue,
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                let origin = line_curve.origin().get();
                let direction = *line_curve.direction().as_raw();
                let resolution = resolve_axis_candidates([
                    ruled_surface_line_pcurve(
                        ctx,
                        surface,
                        SurfaceParameterAxis::U,
                        origin,
                        direction,
                    )?,
                    ruled_surface_line_pcurve(
                        ctx,
                        surface,
                        SurfaceParameterAxis::V,
                        origin,
                        direction,
                    )?,
                ]);
                match resolution {
                    InverseResolution::Unique(geometry) => (geometry, None, None, false),
                    InverseResolution::Ambiguous => {
                        sink.stats.ambiguous_pcurve_parameters += 1;
                        continue;
                    }
                    InverseResolution::NoMatch => continue,
                }
            }
            _ => continue,
        };
        let id = PcurveId::compose(
            &pcurve_namespace(),
            if cache {
                cadmpeg_ir::identity_key!("nurbs-surface-cache:")
            } else {
                cadmpeg_ir::identity_key!("nurbs-isoparametric:")
            }
            .then(coedge.id.key()),
        );
        let parameter_range = match parameter_range.map(cadmpeg_ir::units::FiniteVector::new) {
            Some(None) => continue,
            Some(Some(range)) => Some(range),
            None => None,
        };
        let Ok(fit_tolerance) = fit_tolerance
            .map(cadmpeg_ir::geometry::FitTolerance::try_new)
            .transpose()
        else {
            continue;
        };
        let pcurve = Pcurve {
            id: id.try_clone_for_decode(ctx, "SLDPRT decoded identity copy")?,
            geometry,
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                None,
                parameter_range,
                fit_tolerance,
            ),
        };
        ctx.push_vec(
            &mut derived,
            (index, id, pcurve, cache),
            "collect derived Parasolid pcurves",
        )?;
    }
    // The sink is drained before the `?` below: an error on that route must
    // not drop a refusal the walk above already pushed.
    for record in ctx.admit_iter(
        lane_refusals.take_records(),
        "collect isoparametric pcurve losses",
    )? {
        let note = crate::loss::spline_lane_refusal(ctx, &record)?;
        ctx.push_vec(sink.losses, note, "collect isoparametric pcurve losses")?;
    }
    for (index, id, pcurve, cache) in ctx.admit_iter(derived, "bind derived Parasolid pcurves")? {
        let parameter_range = pcurve
            .parameter_range()
            .map(|range| cadmpeg_ir::geometry::DirectedParameterRange::new(range.get()))
            .transpose()
            .map_err(cadmpeg_core::CodecError::malformed)?;
        annotations.note(
            ctx,
            id.as_str(),
            source_stream,
            0,
            Some(if cache {
                "derived_nurbs_surface_cache_pcurve"
            } else {
                "derived_nurbs_isoparametric_pcurve"
            }),
        )?;
        annotations.exactness(ctx, id.as_str(), Exactness::Derived)?;
        sink.bind(ctx, index, id, parameter_range)?;
        admit_brep_entity(ctx)?;
        ctx.push_vec(sink.pcurves, pcurve, "collect derived Parasolid pcurves")?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IntersectionPcurveSource {
    StoredCache,
    AnalyticInverse,
    NurbsInverse,
}

/// Conservative maximum separation between one analytic surface image of a
/// linear UV segment and the straight chord through its endpoints.
fn analytic_pcurve_chord_bound(
    surface: &SurfaceGeometry,
    start: cadmpeg_ir::math::Point2,
    end: cadmpeg_ir::math::Point2,
) -> Option<f64> {
    let du = (end.u - start.u).abs();
    let dv = (end.v - start.v).abs();
    let second_derivative_bound = match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => 0.0,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let radius = cylinder_surface.radius().get();
            radius.abs() * du * du
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            let slope = half_angle.tan().abs();
            let radial_scale = 1.0f64.max(ratio.abs());
            let max_radius = (radius + start.v * half_angle.tan())
                .abs()
                .max((radius + end.v * half_angle.tan()).abs());
            radial_scale * (max_radius * du * du + 2.0 * slope * du * dv)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let radius = sphere_surface.radius().get();
            radius.abs() * (du + dv).powi(2)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            (major_radius.abs() + minor_radius.abs()) * du * du
                + 2.0 * minor_radius.abs() * du * dv
                + minor_radius.abs() * dv * dv
        }
        _ => return None,
    };
    let bound = second_derivative_bound / 8.0;
    bound.is_finite().then_some(bound)
}

pub(super) fn intersection_support_pcurve(
    ctx: &DecodeContext<'_>,
    support_data: &crate::brep::intersection::IntersectionSupportData,
    chart: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    surface_attr: u16,
    surface: &SurfaceGeometry,
    edge_endpoints: [cadmpeg_ir::math::Point3; 2],
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<(PcurveGeometry, [f64; 2], IntersectionPcurveSource)>, cadmpeg_core::CodecError>
{
    macro_rules! some_or_none {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    (|| -> Result<Option<(PcurveGeometry, [f64; 2], IntersectionPcurveSource)>, cadmpeg_core::CodecError> {
        if chart.degree() != 1
            || matches!(chart.pole_rows(), cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { .. })
            || chart.periodic()
            || !support_data.fit_tolerance_mm.is_finite()
            || support_data.fit_tolerance_mm <= 0.0
        {
            return Ok(None);
        }
        let parameter_range = some_or_none!(nurbs_curve_parameter_domain(chart)).endpoints();
        let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points: chart_points } = chart.pole_rows() else { return Ok(None) };
        let support_index = match support_data.supports.map(|support| support == surface_attr) {
            [true, false] => 0,
            [false, true] => 1,
            _ => return Ok(None),
        };
        let squared_distance = |left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3| {
            (left.x - right.x).powi(2) + (left.y - right.y).powi(2) + (left.z - right.z).powi(2)
        };
        let model_endpoints = [
            *some_or_none!(chart_points.first()),
            *some_or_none!(chart_points.last()),
        ];
        let direct_error = squared_distance(model_endpoints[0].get(), edge_endpoints[0])
            + squared_distance(model_endpoints[1].get(), edge_endpoints[1]);
        let reverse_error = squared_distance(model_endpoints[0].get(), edge_endpoints[1])
            + squared_distance(model_endpoints[1].get(), edge_endpoints[0]);
        let targets = if direct_error <= reverse_error {
            edge_endpoints
        } else {
            [edge_endpoints[1], edge_endpoints[0]]
        };
        let (mut control_points, source) = if let Some(support_uv) = &support_data.support_uv {
            let source_points = &support_uv[support_index];
            let mut control_points = ctx.copy_slice(source_points, "copy intersection support UV controls")?;
            match surface {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {
                    for point in ctx.admit_iter(&mut control_points, "scale intersection support UV controls")? {
                        point.u *= LEN_TO_MM;
                        point.v *= LEN_TO_MM;
                    }
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
                    for point in ctx.admit_iter(&mut control_points, "scale intersection support UV controls")? {
                        point.v *= LEN_TO_MM;
                    }
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
                    for point in ctx.admit_iter(&mut control_points, "scale intersection support UV controls")? {
                        point.v *= LEN_TO_MM;
                    }
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => {}
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => {}
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => {}
                _ => return Ok(None),
            }
            (control_points, IntersectionPcurveSource::StoredCache)
        } else {
            match surface {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) => {
                    let mut control_points = Vec::new();
                    ctx.reserve_capacity(&mut control_points, chart_points.len(), "solve intersection support UV controls")?;
                    for point in ctx.admit_iter(chart_points, "scan SLDPRT chart_points values")? {
                        let parameters = match nurbs_surface_parameter_within_tolerance(
                            ctx,
                            surface,
                            point.get(),
                            control_points.last().copied(),
                            support_data.fit_tolerance_mm,
                        ) {
                            Ok(Some(parameters)) => parameters,
                            Ok(None) => return Ok(None),
                            Err(limit) => return Err(limit),
                        };
                        ctx.push_vec(&mut (control_points), parameters.get(), "solve intersection support UV controls")?;
                    }
                    (control_points, IntersectionPcurveSource::NurbsInverse)
                }
                _ => {
                    let mut control_points = Vec::new();
                    ctx.reserve_capacity(&mut control_points, chart_points.len(), "project intersection analytic controls")?;
                    for point in ctx.admit_iter(chart_points, "scan SLDPRT chart_points values")? {
                        let parameters = some_or_none!(analytic_surface_parameters(surface, point.get()));
                        ctx.push_vec(&mut (control_points), cadmpeg_ir::math::Point2::from(parameters), "project intersection analytic controls")?;
                    }
                    for index in ctx.admit_iter(1..control_points.len(), "unwrap intersection analytic controls")? {
                        let previous = control_points[index - 1];
                        match surface {
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
                                control_points[index].u += ((previous.u - control_points[index].u)
                                    / std::f64::consts::TAU)
                                    .round()
                                    * std::f64::consts::TAU;
                            }
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
                                control_points[index].u += ((previous.u - control_points[index].u)
                                    / std::f64::consts::TAU)
                                    .round()
                                    * std::f64::consts::TAU;
                            }
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => {
                                control_points[index].u += ((previous.u - control_points[index].u)
                                    / std::f64::consts::TAU)
                                    .round()
                                    * std::f64::consts::TAU;
                            }
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => {
                                control_points[index].u += ((previous.u - control_points[index].u)
                                    / std::f64::consts::TAU)
                                    .round()
                                    * std::f64::consts::TAU;
                            }
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {}
                            _ => return Ok(None),
                        }
                        if matches!(
                            surface,
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_))
                        ) {
                            control_points[index].v += ((previous.v - control_points[index].v)
                                / std::f64::consts::TAU)
                                .round()
                                * std::f64::consts::TAU;
                        }
                    }
                    (control_points, IntersectionPcurveSource::AnalyticInverse)
                }
            }
        };
        if control_points.len() != chart_points.len() {
            return Ok(None);
        }
        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) = surface {
            let tolerance = inverse_coordinate_tolerance(ctx, edge_endpoints)?;
            let last = control_points.len() - 1;
            for (index, target) in [(0, targets[0]), (last, targets[1])] {
                control_points[index] = match nurbs_surface_parameter_within_tolerance(
                            ctx,
                    surface,
                    target,
                    Some(control_points[index]),
                    tolerance,
                ) {
                    Ok(Some(parameters)) => parameters.get(),
                    Ok(None) => return Ok(None),
                    Err(limit) => return Err(limit),
                };
            }
        } else {
            let adjust_periodic = |parameter: f64, reference: f64| {
                parameter
                    + ((reference - parameter) / std::f64::consts::TAU).round()
                        * std::f64::consts::TAU
            };
            let last = control_points.len() - 1;
            for (index, target) in [(0, targets[0]), (last, targets[1])] {
                let reference = control_points[index];
                let mut parameters =
                    cadmpeg_ir::math::Point2::from(some_or_none!(analytic_surface_parameters(surface, target)));
                match surface {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
                        parameters.u = adjust_periodic(parameters.u, reference.u);
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
                        parameters.u = adjust_periodic(parameters.u, reference.u);
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => {
                        parameters.u = adjust_periodic(parameters.u, reference.u);
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => {
                        parameters.u = adjust_periodic(parameters.u, reference.u);
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {}
                    _ => return Ok(None),
                }
                if matches!(
                    surface,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_))
                ) {
                    parameters.v = adjust_periodic(parameters.v, reference.v);
                }
                control_points[index] = parameters;
            }
        }
        let tolerance = inverse_coordinate_tolerance(ctx, edge_endpoints)?;
        for (parameters, target) in [some_or_none!(control_points.first()), some_or_none!(control_points.last())]
            .into_iter()
            .zip(targets)
        {
            let Some(point) = crate::brep::evaluation::surface_point(ctx, surface, parameters.u, parameters.v)? else { return Ok(None); };
            let point = point.get();
            if squared_distance(point, target) > tolerance * tolerance {
                return Ok(None);
            }
        }
        let mut mapped_points_storage = ctx.reserve_scoped(0, "SLDPRT temporary vector storage")?;
        let mut mapped_points = Vec::new();
        mapped_points_storage.with_storage(|| ctx.reserve_capacity(&mut mapped_points, control_points.len(), "map intersection support controls"))?;
        for parameters in ctx.admit_iter(&control_points, "scan SLDPRT intersection_support_pcurve values")? {
            let Some(point) = crate::brep::evaluation::surface_point(ctx, surface, parameters.u, parameters.v)? else { return Ok(None); };
            let point = point.get();
            mapped_points_storage.with_storage(|| ctx.push_vec(&mut (mapped_points), point, "map intersection support controls"))?;
        }
        let control_errors = mapped_points_storage.with_storage(|| {
            ctx.collect_vec(
                mapped_points.iter().zip(chart_points).map(|(point, target)| {
                    squared_distance(*point, target.get()).sqrt()
                }),
                "check intersection support control errors",
            )
        })?;
        if ctx.any_by(
            &control_errors,
            |error| Ok(!error.is_finite() || *error > support_data.fit_tolerance_mm),
            "check intersection support control errors",
        )? {
            return Ok(None);
        }
        for ((parameters, chord), endpoint_errors) in ctx.admit_iter(&control_points, "scan Parasolid intersection parameter segments")?
            .windows(const { crate::nonzero(2) })
            .zip(chart_points.windows(2))
            .zip(control_errors.windows(2))
        {
            let exceeds = match surface {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) => {
                    let error = nurbs_surface_parameter_segment_chord_bound(
                        ctx,
                        surface,
                        [parameters[0], parameters[1]],
                        [chord[0].get(), chord[1].get()],
                    )?;
                    error.is_none_or(|error| error > support_data.fit_tolerance_mm)
                }
                _ => analytic_pcurve_chord_bound(surface, parameters[0], parameters[1]).is_none_or(
                    |curvature_error| {
                        curvature_error + endpoint_errors[0].max(endpoint_errors[1])
                            > support_data.fit_tolerance_mm
                    },
                ),
            };
            if exceeds {
                return Ok(None);
            }
        }
        let knots = ctx.copy_slice(chart.knots().as_slice(), "copy intersection support pcurve knots")?;
        let nurbs =
            match PcurveNurbs::from_lanes(ctx, 1, knots, control_points, None, false)? {
                Ok(nurbs) => nurbs,
                Err(error) => {
                    refusal.note(
                        ctx,
                        format_args!(
                            "sldprt intersection support pcurve for surface attr {surface_attr}"
                        ),
                        &error,
                    )?;
                    return Ok(None);
                }
            };
        Ok(Some((
            PcurveGeometry::Nurbs { nurbs },
            parameter_range,
            source,
        )))
    })()
}

#[cfg(test)]
mod tests;
