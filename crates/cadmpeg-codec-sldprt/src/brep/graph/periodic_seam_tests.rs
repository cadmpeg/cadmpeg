// SPDX-License-Identifier: Apache-2.0
use super::*;
#[test]
fn degree_one_nurbs_cache_pcurve_continues_across_a_periodic_seam() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    // A closed ring in v whose seam at v = 0 lies inside the flat side x = 1.
    let ring = [(1.0, 0.0), (1.0, 1.0), (-1.0, 0.0), (1.0, -1.0), (1.0, 0.0)];
    let rows = [0.0, 1.0].map(|z| {
        ring.map(|(x, y)| cadmpeg_ir::math::Point3::new(x, y, z))
            .to_vec()
    });
    let surface = cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
            1,
            vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
            true,
        ),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(rows.to_vec(), None),
        false,
    )
    .expect("closed ring surface");
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.5),
            cadmpeg_ir::math::Point3::new(1.0, -0.5, 0.5),
        ],
        None,
        false,
    )
    .expect("seam curve");

    let (lanes, fit_tolerance) =
        super::nurbs_degree_one_cache_lanes(&ctx, &surface, &curve, [0.0, 1.0])
            .unwrap_or_else(|_| panic!("the seam curve projects"))
            .expect("the seam curve has lanes");

    assert!((lanes[0].v - 0.5).abs() < 1.0e-9);
    assert!((lanes[1].v + 0.5).abs() < 1.0e-9);
    assert!((lanes[0].u - 0.5).abs() < 1.0e-9);
    assert!((lanes[1].u - 0.5).abs() < 1.0e-9);
    assert!(fit_tolerance < 1.0e-9);
}

/// A cylinder of radius 5 about +z, reference direction +x, bounded by closed
/// circles at z = 0 and z = 10. Circle `k` closes at `vertices[k]`; where
/// `shared[k]` holds, a line edge along +y also ends at that vertex.
fn two_circle_cylinder(vertices: [Point3; 2], shared: [bool; 2]) -> Brep {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve};
    use cadmpeg_ir::math::Vector3;
    let id = |kind: &str, index: usize| format!("test:seam:{kind}#{index}");
    let surface_id = SurfaceId::mint(id("surface", 0)).expect("identity grammar");
    let face_id = FaceId::mint(id("face", 0)).expect("identity grammar");
    let mut brep = Brep {
        surfaces: vec![Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    5.0,
                )
                .expect("cylinder frame"),
            )),
            source_object: None,
            native_ref: None,
        }],
        ..Default::default()
    };
    let mut loops = Vec::new();
    let point = |brep: &mut Brep, name: String, position: Point3| {
        let point_id = PointId::mint(format!("test:seam:point#{name}")).expect("identity grammar");
        let vertex_id =
            VertexId::mint(format!("test:seam:vertex#{name}")).expect("identity grammar");
        brep.points.push(Point::new(
            point_id.clone(),
            FinitePoint3::new(position).expect("finite position"),
            None,
        ));
        brep.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: None,
            native_ref: None,
        });
        vertex_id
    };
    for (index, (position, shared)) in vertices.into_iter().zip(shared).enumerate() {
        let z = 10.0 * f64::from(u8::try_from(index).expect("two circles"));
        let curve_id = CurveId::mint(id("circle", index)).expect("identity grammar");
        let edge_id = EdgeId::mint(id("circle-edge", index)).expect("identity grammar");
        let loop_id = LoopId::mint(id("loop", index)).expect("identity grammar");
        let coedge_id = CoedgeId::mint(id("coedge", index)).expect("identity grammar");
        let vertex_id = point(&mut brep, format!("circle-{index}"), position);
        brep.curves.push(Curve {
            parameter_range: None,
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(0.0, 0.0, z),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    5.0,
                )
                .expect("circle frame"),
            )),
            source_object: None,
            native_ref: None,
        });
        brep.edges.push(Edge {
            id: edge_id.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex_id.clone(),
                vertex_id.clone(),
            )),
            tolerance: None,
            native_ref: None,
        });
        brep.coedges.push(Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            radial_next: coedge_id.clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
        brep.loops.push(Loop {
            id: loop_id.clone(),
            face: face_id.clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(vec![coedge_id], Vec::new())
                    .expect("valid loop ring"),
            ),
            native_ref: None,
        });
        loops.push(loop_id);
        if shared {
            let line_id = CurveId::mint(id("line", index)).expect("identity grammar");
            let far = point(
                &mut brep,
                format!("line-{index}"),
                position.translated(Vector3::new(0.0, 1.0, 0.0), 10.0),
            );
            brep.curves.push(Curve {
                parameter_range: None,
                id: line_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    LineCurve::try_new(position, Vector3::new(0.0, 1.0, 0.0))
                        .expect("line direction"),
                )),
                source_object: None,
                native_ref: None,
            });
            brep.edges.push(Edge {
                id: EdgeId::mint(id("line-edge", index)).expect("identity grammar"),
                carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(line_id)),
                ends: Some(cadmpeg_ir::topology::EdgeEnds::new(vertex_id, far)),
                tolerance: None,
                native_ref: None,
            });
        }
    }
    brep.faces.push(Face {
        id: face_id,
        shell: ShellId::mint(id("shell", 0)).expect("identity grammar"),
        surface: surface_id,
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    brep
}

/// Run the cylinder seam pass. Where `derived[k]` holds, circle `k`'s point is
/// annotated `derived`, as the decoder annotates a vertexless ring edge's
/// closure point; the other points are stored.
fn synthesize_seams(brep: &mut Brep, derived: [bool; 2]) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let mut annotations = AnnotationBuilder::new();
    for (index, derived) in derived.into_iter().enumerate() {
        if derived {
            annotations.exactness(
                format!("test:seam:point#circle-{index}"),
                Exactness::Derived,
            );
        }
    }
    let source_stream =
        cadmpeg_ir::annotations::StreamHandle::new(cadmpeg_ir::stream_name!("test"));
    synthesize_cylinder_seams(&ctx, brep, &mut annotations, &source_stream)
        .expect("seam synthesis");
}

fn circle_vertex_position(brep: &Brep, index: usize) -> Point3 {
    let vertex = format!("test:seam:vertex#circle-{index}");
    let point = &brep
        .vertices
        .iter()
        .find(|candidate| candidate.id.as_str() == vertex)
        .expect("circle vertex")
        .point;
    brep.points
        .iter()
        .find(|candidate| candidate.id == *point)
        .expect("circle vertex point")
        .position()
        .get()
}

fn seam_edge(brep: &Brep) -> Option<&Edge> {
    brep.edges
        .iter()
        .find(|edge| edge.id.as_str().contains("#seam:"))
}

#[test]
fn seam_runs_through_a_circle_vertex_another_edge_ends_at() {
    let tangent = Point3::new(5.0, 0.0, 0.0);
    let mut brep = two_circle_cylinder([tangent, Point3::new(0.0, 5.0, 10.0)], [true, false]);

    synthesize_seams(&mut brep, [false, true]);

    assert_eq!(circle_vertex_position(&brep, 0), tangent);
    assert!(circle_vertex_position(&brep, 1).distance(Point3::new(5.0, 0.0, 10.0)) <= 1.0e-12);
    let seam = seam_edge(&brep).expect("a seam edge");
    let [start, end] = seam
        .param_range()
        .map(cadmpeg_ir::units::FiniteVector::get)
        .expect("a seam range");
    assert!(start.abs() <= 1.0e-12 && (end - 10.0).abs() <= 1.0e-12);
    let curve = brep
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == seam.curve())
        .expect("seam curve");
    let Some(SolvedCurveGeometry::Line(line)) = curve.geometry.solved() else {
        panic!("the seam is a line");
    };
    assert!(line.origin().get().distance(tangent) <= 1.0e-12);
    let direction = *line.direction().as_raw();
    assert!((direction.z - 1.0).abs() <= 1.0e-12);
    assert_eq!(brep.faces[0].loops.len(), 1);
}

#[test]
fn seam_moves_derived_circle_vertices_to_the_reference_ruling() {
    let mut brep = two_circle_cylinder(
        [Point3::new(5.0, 0.0, 0.0), Point3::new(0.0, 5.0, 10.0)],
        [false, false],
    );

    synthesize_seams(&mut brep, [true, true]);

    assert_eq!(
        circle_vertex_position(&brep, 0),
        Point3::new(-5.0, 0.0, 0.0)
    );
    assert_eq!(
        circle_vertex_position(&brep, 1),
        Point3::new(-5.0, 0.0, 10.0)
    );
    assert!(seam_edge(&brep).is_some());
    assert_eq!(brep.faces[0].loops.len(), 1);
}

#[test]
fn shared_circle_vertices_on_different_rulings_get_no_seam() {
    let vertices = [Point3::new(5.0, 0.0, 0.0), Point3::new(0.0, 5.0, 10.0)];
    let mut brep = two_circle_cylinder(vertices, [true, true]);

    synthesize_seams(&mut brep, [false, false]);

    assert_eq!(circle_vertex_position(&brep, 0), vertices[0]);
    assert_eq!(circle_vertex_position(&brep, 1), vertices[1]);
    assert!(seam_edge(&brep).is_none());
    assert_eq!(brep.faces[0].loops.len(), 2);
}

#[test]
fn seam_runs_through_a_stored_circle_vertex_no_other_edge_ends_at() {
    let stored = Point3::new(5.0, 0.0, 0.0);
    let mut brep = two_circle_cylinder([stored, Point3::new(0.0, 5.0, 10.0)], [false, false]);

    synthesize_seams(&mut brep, [false, true]);

    assert_eq!(circle_vertex_position(&brep, 0), stored);
    assert!(circle_vertex_position(&brep, 1).distance(Point3::new(5.0, 0.0, 10.0)) <= 1.0e-12);
    let seam = seam_edge(&brep).expect("a seam edge");
    let curve = brep
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == seam.curve())
        .expect("seam curve");
    let Some(SolvedCurveGeometry::Line(line)) = curve.geometry.solved() else {
        panic!("the seam is a line");
    };
    assert!(line.origin().get().distance(stored) <= 1.0e-12);
    assert!((line.direction().as_raw().z - 1.0).abs() <= 1.0e-12);
    assert_eq!(brep.faces[0].loops.len(), 1);
}

#[test]
fn stored_circle_vertices_on_different_rulings_get_no_seam() {
    let vertices = [Point3::new(5.0, 0.0, 0.0), Point3::new(0.0, 5.0, 10.0)];
    let mut brep = two_circle_cylinder(vertices, [false, false]);

    synthesize_seams(&mut brep, [false, false]);

    assert_eq!(circle_vertex_position(&brep, 0), vertices[0]);
    assert_eq!(circle_vertex_position(&brep, 1), vertices[1]);
    assert!(seam_edge(&brep).is_none());
    assert_eq!(brep.faces[0].loops.len(), 2);
}

#[test]
fn a_stored_circle_vertex_within_resolution_of_the_reference_point_keeps_its_position() {
    let stored = Point3::new(-5.0, 1.0e-6, 0.0);
    let mut brep = two_circle_cylinder([stored, Point3::new(0.0, 5.0, 10.0)], [false, false]);

    synthesize_seams(&mut brep, [false, true]);

    assert_eq!(circle_vertex_position(&brep, 0), stored);
    assert_eq!(
        circle_vertex_position(&brep, 1),
        Point3::new(-5.0, 0.0, 10.0)
    );
    assert!(seam_edge(&brep).is_some());
    assert_eq!(brep.faces[0].loops.len(), 1);
}
