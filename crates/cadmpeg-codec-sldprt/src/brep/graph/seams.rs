// SPDX-License-Identifier: Apache-2.0
//! Repair cylinder and sphere periodic seam topology.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::Exactness;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::geometry::pcurve::Pcurve;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::ids::CoedgeId;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::ids::LoopId;
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::ids::VertexId;
use cadmpeg_ir::topology::Coedge;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::topology::Vertex;
use std::collections::HashMap;
use std::collections::HashSet;
use super::Brep;
use super::EPS_POINT_DISTANCE;
use super::admit_brep_entity;
use super::pcurve_namespace;


/// The position of each identity in `ids`; a repeated identity keeps its first
/// position.
fn first_positions<'a, K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    ids: impl IntoIterator<Item = &'a K>,
    operation: &'static str,
) -> Result<HashMap<&'a K, usize>, cadmpeg_core::CodecError> {
    let mut positions = HashMap::new();
    let mut ids = ids.into_iter().enumerate();
    while let Some((index, id)) = ctx.next_charged(&mut ids, operation)? {
        if let std::collections::hash_map::Entry::Vacant(slot) =
            ctx.entry_hash_map(&mut positions, id, operation)?
        {
            slot.insert(index);
        }
    }
    Ok(positions)
}

/// One cylinder face whose two circle loops become one ring through a
/// generated seam edge.
struct CylinderSeam {
    face: usize,
    loop_a: usize,
    loop_a_id: LoopId,
    loop_b_id: LoopId,
    circle_a: usize,
    circle_b: usize,
    vertex_a: VertexId,
    vertex_b: VertexId,
    point_a: Option<usize>,
    point_b: Option<usize>,
    seam_a: cadmpeg_ir::math::Point3,
    seam_b: cadmpeg_ir::math::Point3,
}

pub(super) fn synthesize_cylinder_seams(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    const COPY: &str = "SLDPRT decoded identity copy";
    const LOOK_UP: &str = "look up Parasolid cylinder seam topology";
    let mut scratch = ctx.reserve_scoped(0, "select Parasolid cylinder seams")?;
    let candidates = {
        let surfaces = scratch.with_storage(|| {
            ctx.collect_hash_map(
                out.surfaces.iter().map(|surface| (&surface.id, surface)),
                "index Parasolid seam surfaces",
            )
        })?;
        let loops = scratch.with_storage(|| {
            first_positions(
                ctx,
                out.loops.iter().map(|lp| &lp.id),
                "index Parasolid seam loops",
            )
        })?;
        let coedges = scratch.with_storage(|| {
            first_positions(
                ctx,
                out.coedges.iter().map(|coedge| &coedge.id),
                "index Parasolid seam coedges",
            )
        })?;
        let edges = scratch.with_storage(|| {
            ctx.collect_hash_map(
                out.edges.iter().map(|edge| (&edge.id, edge)),
                "index Parasolid seam edges",
            )
        })?;
        let curves = scratch.with_storage(|| {
            ctx.collect_hash_map(
                out.curves.iter().map(|curve| (&curve.id, curve)),
                "index Parasolid seam curves",
            )
        })?;
        let vertices = scratch.with_storage(|| {
            first_positions(
                ctx,
                out.vertices.iter().map(|vertex| &vertex.id),
                "index Parasolid seam vertices",
            )
        })?;
        let points = scratch.with_storage(|| {
            first_positions(
                ctx,
                out.points.iter().map(|point| &point.id),
                "index Parasolid seam points",
            )
        })?;
        let point_of = |vertex: &VertexId| -> Result<Option<usize>, cadmpeg_core::CodecError> {
            let Some(&vertex) = ctx.get_hash_map(&vertices, vertex, LOOK_UP)? else {
                return Ok(None);
            };
            Ok(ctx
                .get_hash_map(&points, &out.vertices[vertex].point, LOOK_UP)?
                .copied())
        };
        let mut candidates = Vec::new();
        for (face_index, face) in ctx
            .admit_iter(&out.faces, "select Parasolid cylinder seams")?
            .enumerate()
        {
            let Some(surface) = ctx.get_hash_map(&surfaces, &face.surface, LOOK_UP)? else {
                continue;
            };
            let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
            else {
                continue;
            };
            let ref_direction = *cylinder_surface.frame().reference().as_raw();
            if face.loops.len() != 2 {
                continue;
            }
            let mut members = face.loops.iter();
            let (Some(&a_index), Some(&b_index)) = (
                members
                    .next()
                    .map(|id| ctx.get_hash_map(&loops, id, LOOK_UP))
                    .transpose()?
                    .flatten(),
                members
                    .next()
                    .map(|id| ctx.get_hash_map(&loops, id, LOOK_UP))
                    .transpose()?
                    .flatten(),
            ) else {
                continue;
            };
            let (a, b) = (&out.loops[a_index], &out.loops[b_index]);
            let ([a_coedge], [b_coedge]) = (a.coedges(), b.coedges()) else {
                continue;
            };
            let Some(&ca) = ctx.get_hash_map(&coedges, a_coedge, LOOK_UP)? else {
                continue;
            };
            let Some(&cb) = ctx.get_hash_map(&coedges, b_coedge, LOOK_UP)? else {
                continue;
            };
            let Some(ea) = ctx.get_hash_map(&edges, &out.coedges[ca].edge, LOOK_UP)? else {
                continue;
            };
            let Some(eb) = ctx.get_hash_map(&edges, &out.coedges[cb].edge, LOOK_UP)? else {
                continue;
            };
            let seam_point = |edge: &Edge| -> Result<_, cadmpeg_core::CodecError> {
                if !ctx.equal(&edge.start, &edge.end, "compare Parasolid seam vertices")? {
                    return Ok(None);
                }
                let Some(curve) = edge.curve() else {
                    return Ok(None);
                };
                let Some(curve) = ctx.get_hash_map(&curves, curve, LOOK_UP)? else {
                    return Ok(None);
                };
                let Some(SolvedCurveGeometry::Circle(circle_curve)) = curve.geometry.solved()
                else {
                    return Ok(None);
                };
                let center = circle_curve.center().get();
                let radius = circle_curve.radius().get();
                Ok(Some(cadmpeg_ir::math::Point3::new(
                    center.x - ref_direction.x * radius,
                    center.y - ref_direction.y * radius,
                    center.z - ref_direction.z * radius,
                )))
            };
            let (Some(seam_a), Some(seam_b)) = (seam_point(ea)?, seam_point(eb)?) else {
                continue;
            };
            // The replaced loop identity only steers the removal below; the
            // other identities move into the emitted seam.
            let loop_b_id = scratch
                .with_storage(|| b.id.try_clone_for_decode(ctx, "SLDPRT seam loop identity"))?;
            let candidate = CylinderSeam {
                face: face_index,
                loop_a: a_index,
                loop_a_id: a.id.try_clone_for_decode(ctx, COPY)?,
                loop_b_id,
                circle_a: ca,
                circle_b: cb,
                vertex_a: ea.start.try_clone_for_decode(ctx, COPY)?,
                vertex_b: eb.start.try_clone_for_decode(ctx, COPY)?,
                point_a: point_of(&ea.start)?,
                point_b: point_of(&eb.start)?,
                seam_a,
                seam_b,
            };
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut candidates,
                    candidate,
                    "collect Parasolid cylinder seam candidates",
                )
            })?;
        }
        candidates
    };

    let mut removed = HashSet::new();
    for candidate in ctx.admit_iter(candidates, "emit Parasolid cylinder seams")? {
        let CylinderSeam {
            face,
            loop_a,
            loop_a_id,
            loop_b_id,
            circle_a,
            circle_b,
            vertex_a,
            vertex_b,
            point_a,
            point_b,
            seam_a: pa,
            seam_b: pb,
        } = candidate;
        for (point, position) in [(point_a, pa), (point_b, pb)] {
            if let Some(point) = point.and_then(|point| out.points.get_mut(point)) {
                point.set_position(
                    cadmpeg_ir::features::FinitePoint3::new(position).ok_or_else(|| {
                        cadmpeg_core::CodecError::Malformed(Point::NON_FINITE_POSITION.into())
                    })?,
                );
            }
        }
        let direction = cadmpeg_ir::math::Vector3::new(pb.x - pa.x, pb.y - pa.y, pb.z - pa.z);
        let norm = direction.norm();
        if norm == 0.0 {
            continue;
        }
        let direction = cadmpeg_ir::math::Vector3::new(
            direction.x / norm,
            direction.y / norm,
            direction.z / norm,
        );
        // Seam identities extend the face key, which is one u16 attr.
        let suffix = cadmpeg_ir::identity_key!("seam:").then(out.faces[face].id.key());
        let curve_id = CurveId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "curve"),
            &suffix,
        );
        let edge_id = EdgeId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "edge"),
            &suffix,
        );
        let coedge_namespace = cadmpeg_ir::identity_namespace!("sldprt", "brep", "coedge");
        let seam_a = CoedgeId::compose(&coedge_namespace, suffix.clone().colon(0_u16));
        let seam_b = CoedgeId::compose(&coedge_namespace, suffix.colon(1_u16));
        for id in [
            curve_id.as_str(),
            edge_id.as_str(),
            seam_a.as_str(),
            seam_b.as_str(),
        ] {
            annotations.note(ctx, id, source_stream, 0, Some("derived_periodic_seam"))?;
            annotations.exactness(ctx, id, Exactness::Derived)?;
        }
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.curves, 1, "collect Parasolid cylinder seam curves")?;
        out.curves.push(Curve {
            id: curve_id.try_clone_for_decode(ctx, COPY)?,
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                match cadmpeg_ir::geometry::analytic::LineCurve::try_new(pa, direction) {
                    Ok(payload) => payload,
                    Err(_) => continue,
                },
            )),
        });
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.edges, 1, "collect Parasolid cylinder seam edges")?;
        out.edges.push(Edge {
            id: edge_id.try_clone_for_decode(ctx, COPY)?,
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id), Some([0.0, norm]))
                .map_err(cadmpeg_core::CodecError::malformed)?,
            start: vertex_a,
            end: vertex_b,
            tolerance: None,
        });
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(
            &mut out.coedges,
            2,
            "collect Parasolid cylinder seam coedges",
        )?;
        out.coedges.push(Coedge {
            id: seam_a.try_clone_for_decode(ctx, COPY)?,
            owner_loop: loop_a_id.try_clone_for_decode(ctx, COPY)?,
            edge: edge_id.try_clone_for_decode(ctx, COPY)?,
            radial_next: seam_b.try_clone_for_decode(ctx, COPY)?,
            sense: Sense::Forward,
            use_curve: None,
            pcurves: Vec::new(),
        });
        admit_brep_entity(ctx)?;
        out.coedges.push(Coedge {
            id: seam_b.try_clone_for_decode(ctx, COPY)?,
            owner_loop: loop_a_id.try_clone_for_decode(ctx, COPY)?,
            edge: edge_id,
            radial_next: seam_a.try_clone_for_decode(ctx, COPY)?,
            sense: Sense::Reversed,
            use_curve: None,
            pcurves: Vec::new(),
        });
        let ring = cadmpeg_ir::topology::LoopRing::new(
            ctx,
            ctx.collect_retained_vec(
                [
                    out.coedges[circle_a].id.try_clone_for_decode(ctx, COPY)?,
                    seam_a,
                    out.coedges[circle_b].id.try_clone_for_decode(ctx, COPY)?,
                    seam_b,
                ],
                "build Parasolid cylinder seam ring",
            )?,
            Vec::new(),
        )
        .map_err(cadmpeg_core::CodecError::from)?
        .map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "generated periodic seam ring is invalid: {error}"
            ))
        })?;
        for circle in [circle_a, circle_b] {
            out.coedges[circle].owner_loop = loop_a_id.try_clone_for_decode(ctx, COPY)?;
        }
        out.loops[loop_a].boundary = cadmpeg_ir::topology::LoopBoundary::Ring(ring);
        out.faces[face].loops = cadmpeg_ir::topology::FaceLoops::unspecified(
            ctx.collect_retained_vec([loop_a_id], "bind Parasolid cylinder seam loop")?,
        );
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut removed,
                loop_b_id,
                "track replaced Parasolid seam loops",
            )
        })?;
    }
    ctx.retain_vec(
        &mut out.loops,
        |lp| Ok(!ctx.contains_hash_set(&removed, &lp.id, "test SLDPRT hashed identity")?),
        "remove replaced Parasolid seam loops",
    )?;
    Ok(())
}

/// An existing curve-less seam edge of a sphere face, which receives a
/// degenerate pole curve.
struct SphereSeamRepair {
    edge: usize,
    pole: cadmpeg_ir::math::Point3,
    /// The points of the edge's start and end vertices.
    points: [Option<usize>; 2],
}

/// A sphere face bounded by three circles, whose loop gains a generated pole
/// seam.
struct SphereSeam {
    face: usize,
    loop_index: usize,
    loop_id: LoopId,
    ring: Vec<CoedgeId>,
    pole: cadmpeg_ir::math::Point3,
    pole_vertex: Option<VertexId>,
}

pub(super) fn synthesize_sphere_seams(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<(), cadmpeg_core::CodecError> {
    const COPY: &str = "SLDPRT decoded identity copy";
    const LOOK_UP: &str = "look up Parasolid sphere seam topology";
    let mut scratch = ctx.reserve_scoped(0, "select Parasolid sphere seams")?;
    let (repairs, seams) = {
        let surfaces = scratch.with_storage(|| {
            ctx.collect_hash_map(
                out.surfaces
                    .iter()
                    .map(|surface| (&surface.id, &surface.geometry)),
                "index Parasolid sphere geometry",
            )
        })?;
        let loops = scratch.with_storage(|| {
            first_positions(
                ctx,
                out.loops.iter().map(|lp| &lp.id),
                "index Parasolid sphere loops",
            )
        })?;
        let coedge_edges = scratch.with_storage(|| {
            ctx.collect_hash_map(
                out.coedges.iter().map(|coedge| (&coedge.id, &coedge.edge)),
                "index Parasolid sphere coedge edges",
            )
        })?;
        let edge_indices = scratch.with_storage(|| {
            ctx.collect_hash_map(
                out.edges
                    .iter()
                    .enumerate()
                    .map(|(index, edge)| (&edge.id, index)),
                "index Parasolid sphere edges",
            )
        })?;
        let curve_geometry = scratch.with_storage(|| {
            ctx.collect_hash_map(
                out.curves.iter().map(|curve| (&curve.id, &curve.geometry)),
                "index Parasolid sphere curves",
            )
        })?;
        let vertices = scratch.with_storage(|| {
            first_positions(
                ctx,
                out.vertices.iter().map(|vertex| &vertex.id),
                "index Parasolid sphere vertices",
            )
        })?;
        let points = scratch.with_storage(|| {
            first_positions(
                ctx,
                out.points.iter().map(|point| &point.id),
                "index Parasolid sphere points",
            )
        })?;
        let vertex_points = scratch.with_storage(|| {
            let mut vertex_points = HashMap::new();
            for vertex in ctx.admit_iter(&out.vertices, "index Parasolid sphere vertex points")? {
                if let Some(&point) = ctx.get_hash_map(&points, &vertex.point, LOOK_UP)? {
                    ctx.insert_hash_map(
                        &mut vertex_points,
                        &vertex.id,
                        out.points[point].position().get(),
                        "index Parasolid sphere vertex points",
                    )?;
                }
            }
            Ok::<_, cadmpeg_core::CodecError>(vertex_points)
        })?;
        let point_of = |vertex: &VertexId| -> Result<Option<usize>, cadmpeg_core::CodecError> {
            let Some(&vertex) = ctx.get_hash_map(&vertices, vertex, LOOK_UP)? else {
                return Ok(None);
            };
            Ok(ctx
                .get_hash_map(&points, &out.vertices[vertex].point, LOOK_UP)?
                .copied())
        };
        let edge_of = |coedge: &CoedgeId| -> Result<Option<usize>, cadmpeg_core::CodecError> {
            let Some(edge) = ctx.get_hash_map(&coedge_edges, coedge, LOOK_UP)? else {
                return Ok(None);
            };
            Ok(ctx.get_hash_map(&edge_indices, *edge, LOOK_UP)?.copied())
        };
        let is_circle = |edge: usize| -> Result<bool, cadmpeg_core::CodecError> {
            let Some(curve) = out.edges[edge].curve() else {
                return Ok(false);
            };
            Ok(ctx
                .get_hash_map(&curve_geometry, curve, LOOK_UP)?
                .is_some_and(|geometry| {
                    matches!(
                        geometry,
                        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))
                    )
                }))
        };
        let mut repairs = Vec::new();
        let mut seams = Vec::new();
        for (face_index, face) in ctx
            .admit_iter(&out.faces, "select Parasolid sphere seams")?
            .enumerate()
        {
            let Some(&geometry) = ctx.get_hash_map(&surfaces, &face.surface, LOOK_UP)? else {
                continue;
            };
            let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) = geometry.solved() else {
                continue;
            };
            let center = sphere_surface.center().get();
            let axis = *sphere_surface.frame().axis().as_raw();
            let radius = sphere_surface.radius().get();
            // The analytic sphere axis fixes the seam pole.
            let pole = cadmpeg_ir::math::Point3::new(
                center.x + radius * axis.x,
                center.y + radius * axis.y,
                center.z + radius * axis.z,
            );
            let mut face_loops = face.loops.iter();
            let (Some(loop_id), None) = (face_loops.next(), face_loops.next()) else {
                continue;
            };
            let Some(&loop_index) = ctx.get_hash_map(&loops, loop_id, LOOK_UP)? else {
                continue;
            };
            let lp = &out.loops[loop_index];
            match lp.coedges().len() {
                // Three circles and one curve-less seam: the seam edge
                // receives a degenerate pole curve. An existing endpoint is a
                // topology carrier, not a pole selector.
                4 if matches!(geometry, SurfaceGeometry::Solved(_)) => {
                    let mut seam_edges = 0_usize;
                    let mut seam_edge = None;
                    let mut circles = 0_usize;
                    for coedge in
                        ctx.admit_iter(lp.coedges(), "select Parasolid sphere seam edges")?
                    {
                        let Some(edge) = edge_of(coedge)? else {
                            continue;
                        };
                        if out.edges[edge].curve().is_none() {
                            seam_edges += 1;
                            seam_edge = Some(edge);
                        } else if is_circle(edge)? {
                            circles += 1;
                        }
                    }
                    let (1, Some(edge), 3) = (seam_edges, seam_edge, circles) else {
                        continue;
                    };
                    let points = [
                        point_of(&out.edges[edge].start)?,
                        point_of(&out.edges[edge].end)?,
                    ];
                    scratch.with_storage(|| {
                        ctx.push_vec(
                            &mut repairs,
                            SphereSeamRepair { edge, pole, points },
                            "collect Parasolid sphere seam repairs",
                        )
                    })?;
                }
                // Three circles and no seam: the loop gains a generated seam.
                3 => {
                    if !ctx.all_by(
                        lp.coedges(),
                        |coedge| {
                            Ok(edge_of(coedge)?
                                .map(is_circle)
                                .transpose()?
                                .unwrap_or(false))
                        },
                        "check Parasolid sphere circle loops",
                    )? {
                        continue;
                    }
                    let mut pole_vertex: Option<&VertexId> = None;
                    for coedge in
                        ctx.admit_iter(lp.coedges(), "scan Parasolid sphere pole coedges")?
                    {
                        let Some(edge) = edge_of(coedge)? else {
                            continue;
                        };
                        let edge = &out.edges[edge];
                        for vertex in [&edge.start, &edge.end] {
                            let Some(point) = ctx.get_hash_map(&vertex_points, vertex, LOOK_UP)?
                            else {
                                continue;
                            };
                            let dx = point.x - pole.x;
                            let dy = point.y - pole.y;
                            let dz = point.z - pole.z;
                            if dx * dx + dy * dy + dz * dz > EPS_POINT_DISTANCE {
                                continue;
                            }
                            // The least pole vertex identity carries the seam.
                            pole_vertex = match pole_vertex {
                                Some(known)
                                    if ctx.compare(
                                        known.as_str(),
                                        vertex.as_str(),
                                        "order Parasolid sphere pole vertices",
                                    )? != std::cmp::Ordering::Greater =>
                                {
                                    Some(known)
                                }
                                _ => Some(vertex),
                            };
                        }
                    }
                    let seam = SphereSeam {
                        face: face_index,
                        loop_index,
                        loop_id: lp.id.try_clone_for_decode(ctx, COPY)?,
                        ring: ctx.try_collect_vec(
                            lp.coedges().iter().map(|id| {
                                id.try_clone_for_decode(ctx, "SLDPRT sphere ring identity")
                            }),
                            "copy Parasolid sphere seam ring",
                        )?,
                        pole,
                        pole_vertex: pole_vertex
                            .map(|vertex| {
                                vertex.try_clone_for_decode(ctx, "SLDPRT sphere pole identity")
                            })
                            .transpose()?,
                    };
                    scratch.with_storage(|| {
                        ctx.push_vec(&mut seams, seam, "collect Parasolid sphere seam candidates")
                    })?;
                }
                _ => {}
            }
        }
        (repairs, seams)
    };

    for SphereSeamRepair { edge, pole, points } in
        ctx.admit_iter(repairs, "repair Parasolid sphere seams")?
    {
        let Ok(degenerate) = cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(pole) else {
            continue;
        };
        for point in points.into_iter().flatten() {
            if let Some(vertex_point) = out.points.get_mut(point) {
                vertex_point.set_position(degenerate.point());
            }
        }
        let curve_id = CurveId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "curve"),
            cadmpeg_ir::identity_key!("sphere-seam:").then(out.edges[edge].id.key()),
        );
        annotations.note(
            ctx,
            curve_id.as_str(),
            source_stream,
            0,
            Some("derived_sphere_seam"),
        )?;
        annotations.exactness(ctx, curve_id.as_str(), Exactness::Derived)?;
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(
            &mut out.curves,
            1,
            "collect repaired Parasolid sphere seam curves",
        )?;
        out.curves.push(Curve {
            id: curve_id.try_clone_for_decode(ctx, COPY)?,
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate)),
        });
        out.edges[edge]
            .set_curve(Some(curve_id))
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }

    for seam in ctx.admit_iter(seams, "emit Parasolid sphere seams")? {
        let SphereSeam {
            face: face_index,
            loop_index,
            loop_id,
            mut ring,
            pole,
            pole_vertex,
        } = seam;
        let Ok(degenerate) = cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(pole) else {
            continue;
        };
        let Ok(pcurve) = cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            cadmpeg_ir::math::Point2::new(0.0, std::f64::consts::FRAC_PI_2),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
        ) else {
            continue;
        };

        // Seam identities name the face by its position, one usize.
        let seam_face_key = cadmpeg_ir::identity_key!("sphere-seam-face:").then(face_index);
        let curve_id = CurveId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "curve"),
            &seam_face_key,
        );
        let edge_id = EdgeId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "edge"),
            &seam_face_key,
        );
        let coedge_id = CoedgeId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "brep", "coedge"),
            &seam_face_key,
        );
        let pcurve_id = PcurveId::compose(&pcurve_namespace(), &seam_face_key);
        let pole_vertex = match pole_vertex {
            Some(vertex) => vertex,
            None => {
                let point_id = PointId::compose(
                    &cadmpeg_ir::identity_namespace!("sldprt", "brep", "point"),
                    &seam_face_key,
                );
                let vertex_id = VertexId::compose(
                    &cadmpeg_ir::identity_namespace!("sldprt", "brep", "vertex"),
                    &seam_face_key,
                );
                for id in [point_id.as_str(), vertex_id.as_str()] {
                    annotations.note(ctx, id, source_stream, 0, Some("derived_sphere_seam"))?;
                    annotations.exactness(ctx, id, Exactness::Derived)?;
                }
                admit_brep_entity(ctx)?;
                ctx.reserve_vec(&mut out.points, 1, "collect Parasolid sphere seam points")?;
                out.points.push(Point::new(
                    point_id.try_clone_for_decode(ctx, COPY)?,
                    degenerate.point(),
                    None,
                ));
                admit_brep_entity(ctx)?;
                ctx.reserve_vec(
                    &mut out.vertices,
                    1,
                    "collect Parasolid sphere seam vertices",
                )?;
                out.vertices.push(Vertex {
                    id: vertex_id.try_clone_for_decode(ctx, COPY)?,
                    point: point_id,
                    tolerance: None,
                });
                vertex_id
            }
        };
        for id in [
            curve_id.as_str(),
            edge_id.as_str(),
            coedge_id.as_str(),
            pcurve_id.as_str(),
        ] {
            annotations.note(ctx, id, source_stream, 0, Some("derived_sphere_seam"))?;
            annotations.exactness(ctx, id, Exactness::Derived)?;
        }
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.curves, 1, "collect Parasolid sphere seam curves")?;
        out.curves.push(Curve {
            id: curve_id.try_clone_for_decode(ctx, COPY)?,
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate)),
        });
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.edges, 1, "collect Parasolid sphere seam edges")?;
        out.edges.push(Edge {
            id: edge_id.try_clone_for_decode(ctx, COPY)?,
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
            start: pole_vertex.try_clone_for_decode(ctx, COPY)?,
            end: pole_vertex,
            tolerance: None,
        });
        let Some(full_turn) = cadmpeg_ir::units::FiniteVector::new([0.0, std::f64::consts::TAU])
        else {
            continue;
        };
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.pcurves, 1, "collect Parasolid sphere seam pcurves")?;
        out.pcurves.push(Pcurve {
            id: pcurve_id.try_clone_for_decode(ctx, COPY)?,
            geometry: PcurveGeometry::Line(pcurve),
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                None,
                Some(full_turn),
                None,
            ),
        });
        ctx.push_vec(
            &mut ring,
            coedge_id.try_clone_for_decode(ctx, COPY)?,
            "extend Parasolid sphere seam ring",
        )?;
        let pcurve_uses = ctx.collect_retained_vec(
            [cadmpeg_ir::topology::PcurveUse {
                pcurve: pcurve_id,
                isoparametric: None,
                parameter_range: Some(
                    cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, std::f64::consts::TAU])
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            }],
            "bind Parasolid sphere seam pcurve",
        )?;
        admit_brep_entity(ctx)?;
        ctx.reserve_vec(&mut out.coedges, 1, "collect Parasolid sphere seam coedges")?;
        out.coedges.push(Coedge {
            id: coedge_id.try_clone_for_decode(ctx, COPY)?,
            owner_loop: loop_id,
            edge: edge_id,
            radial_next: coedge_id,
            sense: Sense::Forward,
            use_curve: None,
            pcurves: pcurve_uses,
        });
        let ring = cadmpeg_ir::topology::LoopRing::new(ctx, ring, Vec::new())
            .map_err(cadmpeg_core::CodecError::from)?
            .map_err(|error| {
                cadmpeg_core::CodecError::malformed(format_args!(
                    "generated sphere seam ring is invalid: {error}"
                ))
            })?;
        if let Some(lp) = out.loops.get_mut(loop_index) {
            lp.boundary = cadmpeg_ir::topology::LoopBoundary::Ring(ring);
        }
    }
    Ok(())
}

