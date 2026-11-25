// SPDX-License-Identifier: Apache-2.0
//! Face tessellations over the IR example documents.

use cadmpeg_ir::math::{Point3, Vector3};

use crate::plane::{cross, dot, sub};
use crate::{FaceMesh, Options, Tessellator};

/// Agreement of an area with its closed form.
const AREA_TOLERANCE: f64 = 1e-9;

fn area(mesh: &FaceMesh) -> f64 {
    mesh.triangles
        .iter()
        .map(|[a, b, c]| {
            let (pa, pb, pc) = (mesh.positions[*a], mesh.positions[*b], mesh.positions[*c]);
            let normal = cross(sub(pb, pa), sub(pc, pa));
            dot(normal, normal).sqrt() / 2.0
        })
        .sum()
}

fn facing(mesh: &FaceMesh, triangle: [usize; 3]) -> Vector3 {
    let [a, b, c] = triangle;
    let points: [Point3; 3] = [mesh.positions[a], mesh.positions[b], mesh.positions[c]];
    cross(sub(points[1], points[0]), sub(points[2], points[0]))
}

#[test]
fn each_cube_face_is_two_outward_triangles_of_its_area() {
    let cube = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut tessellator = Tessellator::new(&cube, Options::default());
    let mut total = 0.0;
    for face in &cube.geometry.faces {
        let mesh = tessellator.face(&face.id).unwrap();
        assert_eq!(mesh.triangles.len(), 2, "{}", face.id);
        total += area(&mesh);
        for triangle in &mesh.triangles {
            let normal = facing(&mesh, *triangle);
            let corner = mesh.normals[triangle[0]];
            assert!(dot(normal, corner) > 0.0, "{}", face.id);
        }
        assert!(mesh.deviation <= AREA_TOLERANCE);
    }
    let side = cube
        .geometry
        .points
        .iter()
        .map(|point| point.position().get().x)
        .fold(0.0, f64::max)
        - cube
            .geometry
            .points
            .iter()
            .map(|point| point.position().get().x)
            .fold(f64::INFINITY, f64::min);
    assert!((total - 6.0 * side * side).abs() <= AREA_TOLERANCE * side * side);
}

/// The planar triangle `A(0,0,0) B(1,0,0) C(0,1,0)` as a bilinear NURBS
/// patch whose `u = 0` pole row collapses onto `A`, bounded by straight
/// edges `A→B`, `B→C`, and `C→A` taken from the loop's `first` corner on.
fn pole_triangle(first: usize) -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId) {
    use cadmpeg_ir::geometry::analytic::LineCurve;
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense,
        Vertex,
    };

    let corners = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#patch").unwrap();
    let face = FaceId::mint("test:model:face#triangle").unwrap();
    let loop_id = LoopId::mint("test:model:loop#triangle").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::new(
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsPoleGrid::from_lanes(
                    vec![vec![corners[0], corners[0]], vec![corners[1], corners[2]]],
                    None,
                )
                .unwrap(),
                false,
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    let vertex = |index: usize| VertexId::mint(format!("test:model:vertex#{index}")).unwrap();
    for (index, corner) in corners.iter().enumerate() {
        let point = PointId::mint(format!("test:model:point#{index}")).unwrap();
        ir.geometry.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(*corner).unwrap(),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex(index),
            point,
            tolerance: None,
            native_ref: None,
        });
    }
    let mut ring = Vec::new();
    for step in 0..3 {
        let start = (first + step) % 3;
        let end = (start + 1) % 3;
        let curve = CurveId::mint(format!("test:model:curve#{start}")).unwrap();
        let edge = EdgeId::mint(format!("test:model:edge#{start}")).unwrap();
        let coedge = CoedgeId::mint(format!("test:model:coedge#{start}")).unwrap();
        let chord = sub(corners[end], corners[start]);
        ir.geometry.curves.push(Curve {
            id: curve.clone(),
            source_object: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
                cadmpeg_ir::features::geometry::FinitePoint3::new(corners[start]).unwrap(),
                cadmpeg_ir::units::UnitVector3::normalized(chord).unwrap(),
            ))),
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::new(Some(curve), Some([0.0, chord.norm()])).unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex(start),
                vertex(end),
            )),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next: coedge.clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
        ring.push(coedge);
    }
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).unwrap()),
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:model:shell#triangle").unwrap(),
        surface,
        sense: Sense::Forward,
        loops: FaceLoops::classified(loop_id, Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    (ir, face)
}

#[test]
fn a_loop_through_a_collapsed_pole_row_meshes_its_whole_face() {
    for first in 0..3 {
        let (ir, face) = pole_triangle(first);
        let mut tessellator = Tessellator::new(&ir, Options::default());
        let mesh = tessellator.face(&face).unwrap();
        assert!(
            (area(&mesh) - 0.5).abs() <= AREA_TOLERANCE,
            "loop from corner {first}: area {}",
            area(&mesh)
        );
    }
}

/// A face on the inner sheet of the torus of major radius 0.4 and tube
/// radius 0.5 about `z`, bounded by its circle at latitude `RING`, traced
/// with `u` rising.
fn spindle_strip() -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId) {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, TorusSurface};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense,
        Vertex,
    };

    let (major, minor) = (SPINDLE_MAJOR, SPINDLE_MINOR);
    let ring = major + minor * RING.cos();
    let height = minor * RING.sin();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#spindle").unwrap();
    let face = FaceId::mint("test:model:face#strip").unwrap();
    let loop_id = LoopId::mint("test:model:loop#ring").unwrap();
    let curve = CurveId::mint("test:model:curve#ring").unwrap();
    let edge = EdgeId::mint("test:model:edge#ring").unwrap();
    let coedge = CoedgeId::mint("test:model:coedge#ring").unwrap();
    let vertex = VertexId::mint("test:model:vertex#ring").unwrap();
    let point = PointId::mint("test:model:point#ring").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                major,
                minor,
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    // On the inner sheet `ring` is negative: `u` sits across the axis, and
    // the circle's parameter rises with `u`, starting at `u = π`.
    ir.geometry.curves.push(Curve {
        id: curve.clone(),
        source_object: None,
        parameter_range: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, height),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                ring.abs(),
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    ir.geometry.points.push(Point::new(
        point.clone(),
        cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(ring.abs(), 0.0, height))
            .unwrap(),
        None,
    ));
    ir.geometry.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.edges.push(Edge {
        id: edge.clone(),
        carrier: EdgeCarrier::new(Some(curve), Some([0.0, std::f64::consts::TAU])).unwrap(),
        ends: Some(cadmpeg_ir::topology::EdgeEnds::new(vertex.clone(), vertex)),
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.coedges.push(Coedge {
        id: coedge.clone(),
        owner_loop: loop_id.clone(),
        edge,
        radial_next: coedge.clone(),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
        native_ref: None,
    });
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Ring(LoopRing::new(vec![coedge], Vec::new()).unwrap()),
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:model:shell#strip").unwrap(),
        surface,
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(vec![loop_id]),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    (ir, face)
}

const SPINDLE_MAJOR: f64 = 0.4;
const SPINDLE_MINOR: f64 = 0.5;
const RING: f64 = 3.2;
/// Agreement of a faceted area with its integral, per unit of the integral.
const FACETED_AREA_RATIO: f64 = 1.0e-2;

/// The face lies left of its ring in the parameter plane, from `RING` up to
/// the pole circle where `0.4 + 0.5 cos v` vanishes: the area integrates
/// `|0.4 + 0.5 cos v| * 0.5 * 2π` over that band.
#[test]
fn a_ring_on_a_spindle_inner_sheet_bounds_the_strip_to_its_pole() {
    let (ir, face) = spindle_strip();
    let mut tessellator = Tessellator::new(&ir, Options::default());
    let mesh = tessellator.face(&face).unwrap();
    let pole = std::f64::consts::TAU - (-SPINDLE_MAJOR / SPINDLE_MINOR).acos();
    let steps = 100_000_u32;
    let step = (pole - RING) / f64::from(steps);
    let expected: f64 = (0..steps)
        .map(|index| {
            let v = RING + (f64::from(index) + 0.5) * step;
            (SPINDLE_MAJOR + SPINDLE_MINOR * v.cos()).abs() * SPINDLE_MINOR * step
        })
        .sum::<f64>()
        * std::f64::consts::TAU;
    let found = area(&mesh);
    assert!(
        (found - expected).abs() <= FACETED_AREA_RATIO * expected,
        "area {found}, expected {expected}"
    );
}

const CYLINDER_RADIUS: f64 = 20.0;
const CYLINDER_LENGTH: f64 = 2000.0;

/// The quarter of the cylinder of radius `CYLINDER_RADIUS` about `z` with
/// `0 <= u <= π/2` and `0 <= z <= CYLINDER_LENGTH`, bounded by its two
/// quarter circles and two rulings.
fn long_quarter_cylinder() -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId) {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense,
        Vertex,
    };
    use std::f64::consts::FRAC_PI_2;

    let (r, l) = (CYLINDER_RADIUS, CYLINDER_LENGTH);
    let corners = [
        Point3::new(r, 0.0, 0.0),
        Point3::new(0.0, r, 0.0),
        Point3::new(0.0, r, l),
        Point3::new(r, 0.0, l),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#cylinder").unwrap();
    let face = FaceId::mint("test:model:face#quarter").unwrap();
    let loop_id = LoopId::mint("test:model:loop#quarter").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                r,
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    let vertex = |index: usize| VertexId::mint(format!("test:model:vertex#{index}")).unwrap();
    for (index, corner) in corners.iter().enumerate() {
        let point = PointId::mint(format!("test:model:point#{index}")).unwrap();
        ir.geometry.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(*corner).unwrap(),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex(index),
            point,
            tolerance: None,
            native_ref: None,
        });
    }
    // Each side runs from corner `index` to the next, counterclockwise
    // about the outward normal.
    let circle = |height: f64| {
        SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, height),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                r,
            )
            .unwrap(),
        )
    };
    let ruling = |at: Point3| {
        SolvedCurveGeometry::Line(LineCurve::try_new(at, Vector3::new(0.0, 0.0, 1.0)).unwrap())
    };
    let sides = [
        (circle(0.0), [0.0, FRAC_PI_2], Sense::Forward),
        (ruling(corners[1]), [0.0, l], Sense::Forward),
        (circle(l), [0.0, FRAC_PI_2], Sense::Reversed),
        (ruling(corners[0]), [0.0, l], Sense::Reversed),
    ];
    let mut ring = Vec::new();
    for (index, (geometry, range, sense)) in sides.into_iter().enumerate() {
        let curve = CurveId::mint(format!("test:model:curve#{index}")).unwrap();
        let edge = EdgeId::mint(format!("test:model:edge#{index}")).unwrap();
        let coedge = CoedgeId::mint(format!("test:model:coedge#{index}")).unwrap();
        let (from, to) = (index, (index + 1) % 4);
        let (start, end) = if sense == Sense::Forward {
            (from, to)
        } else {
            (to, from)
        };
        ir.geometry.curves.push(Curve {
            id: curve.clone(),
            source_object: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(geometry),
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::new(Some(curve), Some(range)).unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex(start),
                vertex(end),
            )),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next: coedge.clone(),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
        ring.push(coedge);
    }
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).unwrap()),
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:model:shell#quarter").unwrap(),
        surface,
        sense: Sense::Forward,
        loops: FaceLoops::classified(loop_id, Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    (ir, face)
}

/// A cylinder bends only around its axis, so the rulings need no interior
/// rows: the mesh holds a few triangles per chord step of the quarter
/// circle, and its area stays within the chord limit of `r π/2 l`.
#[test]
fn a_long_cylinder_keeps_its_triangles_long_along_the_rulings() {
    let (ir, face) = long_quarter_cylinder();
    let options = Options::default();
    let mut tessellator = Tessellator::new(&ir, options);
    let mesh = tessellator.face(&face).unwrap();
    let steps =
        std::f64::consts::FRAC_PI_2 / (2.0 * (1.0 - options.chord / CYLINDER_RADIUS).acos());
    assert!(
        cadmpeg_core::convert::f64_from_index(mesh.triangles.len()).unwrap() < 8.0 * steps,
        "{} triangles for {steps} chord steps",
        mesh.triangles.len()
    );
    let exact = CYLINDER_RADIUS * std::f64::consts::FRAC_PI_2 * CYLINDER_LENGTH;
    let chord_area = options.chord * std::f64::consts::FRAC_PI_2 * CYLINDER_LENGTH;
    assert!(
        (area(&mesh) - exact).abs() <= chord_area,
        "area {} against {exact}",
        area(&mesh)
    );
}

const LUNE_RADIUS: f64 = 2.0;
const LUNE_GAP: f64 = 0.05;

/// The sphere of radius `LUNE_RADIUS` about the origin less the thin lune
/// between the meridians at `u = -LUNE_GAP` and `u = 0`: its one loop runs
/// up the first meridian and down the second, with both junctions on poles.
fn sphere_less_a_lune() -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId) {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, SphereSurface};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense,
        Vertex,
    };
    use std::f64::consts::{FRAC_PI_2, PI};

    let r = LUNE_RADIUS;
    let mut ir = cadmpeg_ir::CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#sphere").unwrap();
    let face = FaceId::mint("test:model:face#lune").unwrap();
    let loop_id = LoopId::mint("test:model:loop#lune").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                r,
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    let poles = [Point3::new(0.0, 0.0, -r), Point3::new(0.0, 0.0, r)];
    let vertex = |index: usize| VertexId::mint(format!("test:model:vertex#{index}")).unwrap();
    for (index, pole) in poles.iter().enumerate() {
        let point = PointId::mint(format!("test:model:point#{index}")).unwrap();
        ir.geometry.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(*pole).unwrap(),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex(index),
            point,
            tolerance: None,
            native_ref: None,
        });
    }
    // A meridian at longitude `u` is the circle in the plane through `z`
    // at that longitude, whose parameter runs from the south pole at
    // `3π/2` to the north pole at `5π/2`.
    let meridian = |u: f64| {
        SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(u.sin(), -u.cos(), 0.0),
                Vector3::new(u.cos(), u.sin(), 0.0),
                r,
            )
            .unwrap(),
        )
    };
    let sides = [
        (meridian(-LUNE_GAP), Sense::Forward),
        (meridian(0.0), Sense::Reversed),
    ];
    let mut ring = Vec::new();
    for (index, (geometry, sense)) in sides.into_iter().enumerate() {
        let curve = CurveId::mint(format!("test:model:curve#{index}")).unwrap();
        let edge = EdgeId::mint(format!("test:model:edge#{index}")).unwrap();
        let coedge = CoedgeId::mint(format!("test:model:coedge#{index}")).unwrap();
        ir.geometry.curves.push(Curve {
            id: curve.clone(),
            source_object: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(geometry),
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::new(Some(curve), Some([PI + FRAC_PI_2, 2.0 * PI + FRAC_PI_2]))
                .unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(vertex(0), vertex(1))),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next: coedge.clone(),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
        ring.push(coedge);
    }
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).unwrap()),
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:model:shell#lune").unwrap(),
        surface,
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(vec![loop_id]),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    (ir, face)
}

/// Going up the meridian at `-LUNE_GAP` and down the one at zero keeps
/// the wide side on the left: the face is the lune of angle
/// `2π - LUNE_GAP`, of area `2 r² (2π - LUNE_GAP)`.
#[test]
fn a_loop_between_two_meridians_bounds_the_side_its_direction_selects() {
    let (ir, face) = sphere_less_a_lune();
    let mut tessellator = Tessellator::new(&ir, Options::default());
    let mesh = tessellator.face(&face).unwrap();
    let expected = 2.0 * LUNE_RADIUS * LUNE_RADIUS * (std::f64::consts::TAU - LUNE_GAP);
    assert!(
        (area(&mesh) - expected).abs() <= FACETED_AREA_RATIO * expected,
        "area {}, expected {expected}",
        area(&mesh)
    );
}

const TUBE_MAJOR: f64 = 40.0;
const TUBE_MINOR: f64 = 6.0;
const TUBE_SWEEP: [f64; 2] = [std::f64::consts::FRAC_PI_2, std::f64::consts::PI];

/// The upper half of the tube of the torus of radii `TUBE_MAJOR` and
/// `TUBE_MINOR` about `z`, `0 <= v <= π`, over the longitudes `TUBE_SWEEP`:
/// along the outer equator, up a meridian, back along the inner equator,
/// and down the other meridian.
fn half_tube() -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId) {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, TorusSurface};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense,
        Vertex,
    };
    use std::f64::consts::PI;

    let (big, small) = (TUBE_MAJOR, TUBE_MINOR);
    let [first, last] = TUBE_SWEEP;
    let on = |u: f64, v: f64| {
        let ring = big + small * v.cos();
        Point3::new(ring * u.cos(), ring * u.sin(), small * v.sin())
    };
    let corners = [on(first, 0.0), on(last, 0.0), on(last, PI), on(first, PI)];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#torus").unwrap();
    let face = FaceId::mint("test:model:face#half-tube").unwrap();
    let loop_id = LoopId::mint("test:model:loop#half-tube").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                big,
                small,
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    let vertex = |index: usize| VertexId::mint(format!("test:model:vertex#{index}")).unwrap();
    for (index, corner) in corners.iter().enumerate() {
        let point = PointId::mint(format!("test:model:point#{index}")).unwrap();
        ir.geometry.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(*corner).unwrap(),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex(index),
            point,
            tolerance: None,
            native_ref: None,
        });
    }
    let equator = |radius: f64| {
        SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .unwrap(),
        )
    };
    // The meridian at longitude `u` has its parameter equal to `v`.
    let meridian = |u: f64| {
        SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(big * u.cos(), big * u.sin(), 0.0),
                Vector3::new(u.sin(), -u.cos(), 0.0),
                Vector3::new(u.cos(), u.sin(), 0.0),
                small,
            )
            .unwrap(),
        )
    };
    let sides = [
        (equator(big + small), [first, last], 0, 1, Sense::Forward),
        (meridian(last), [0.0, PI], 1, 2, Sense::Forward),
        (equator(big - small), [first, last], 3, 2, Sense::Reversed),
        (meridian(first), [0.0, PI], 0, 3, Sense::Reversed),
    ];
    let mut ring = Vec::new();
    for (index, (geometry, range, start, end, sense)) in sides.into_iter().enumerate() {
        let curve = CurveId::mint(format!("test:model:curve#{index}")).unwrap();
        let edge = EdgeId::mint(format!("test:model:edge#{index}")).unwrap();
        let coedge = CoedgeId::mint(format!("test:model:coedge#{index}")).unwrap();
        ir.geometry.curves.push(Curve {
            id: curve.clone(),
            source_object: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(geometry),
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::new(Some(curve), Some(range)).unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex(start),
                vertex(end),
            )),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next: coedge.clone(),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
        ring.push(coedge);
    }
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).unwrap()),
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:model:shell#half-tube").unwrap(),
        surface,
        sense: Sense::Forward,
        loops: FaceLoops::classified(loop_id, Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    (ir, face)
}

/// The tube's longitude does not bend at the patch centre, `v = π/2`, but
/// bends at both equators: the mesh stays on the patch, whose area is
/// `(u1 - u0) r R π`.
#[test]
fn a_patch_whose_bending_varies_meshes_its_area() {
    let (ir, face) = half_tube();
    let mut tessellator = Tessellator::new(&ir, Options::default());
    let mesh = tessellator.face(&face).unwrap();
    let expected = (TUBE_SWEEP[1] - TUBE_SWEEP[0]) * TUBE_MINOR * TUBE_MAJOR * std::f64::consts::PI;
    assert!(
        (area(&mesh) - expected).abs() <= FACETED_AREA_RATIO * expected,
        "area {}, expected {expected}",
        area(&mesh)
    );
}

#[test]
fn a_history_body_is_invisible_to_the_tessellator() {
    let nested = cadmpeg_ir::examples::unit_cube_with_history_body().expect("fixture");
    let mut plain = nested.clone();
    plain.model.history_bodies.clear();
    let mut with = Tessellator::new(&nested, Options::default());
    let mut without = Tessellator::new(&plain, Options::default());
    for face in &plain.geometry.faces {
        let found = with.face(&face.id).expect("model face");
        let expected = without.face(&face.id).expect("model face");
        assert_eq!(found.positions, expected.positions);
        assert_eq!(found.triangles, expected.triangles);
    }
    for face in &nested.model.history_bodies[0].topology.faces {
        assert!(
            with.face(&face.id).is_err(),
            "nest face {} is not a model face",
            face.id
        );
    }
}

#[test]
fn a_face_whose_sense_contradicts_its_loop_takes_the_loop_orientation() {
    use cadmpeg_ir::topology::Sense;

    let cube = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut flipped = cube.clone();
    let face = &mut flipped.geometry.faces[0];
    face.sense = match face.sense {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    };
    let id = face.id.clone();
    let expected = Tessellator::new(&cube, Options::default())
        .face(&id)
        .unwrap();
    let mesh = Tessellator::new(&flipped, Options::default())
        .face(&id)
        .unwrap();
    assert!((area(&mesh) - area(&expected)).abs() <= AREA_TOLERANCE);
    for triangle in &mesh.triangles {
        assert!(dot(facing(&mesh, *triangle), expected.normals[0]) > 0.0);
        assert!(dot(mesh.normals[triangle[0]], expected.normals[0]) > 0.0);
    }
}

const BAND_MAJOR: f64 = 5.315_956_317_980_74;
const BAND_MINOR: f64 = 0.95;

/// The band of the torus of radii `BAND_MAJOR` and `BAND_MINOR` about `z`
/// between the meridians at `u = -π/2` and `u = π/2`, across the seam at
/// `u = 0`: each loop is one meridian circle, whose parameter is `v`.
fn tube_band() -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId) {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, TorusSurface};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense,
        Vertex,
    };
    use std::f64::consts::{FRAC_PI_2, TAU};

    let (big, small) = (BAND_MAJOR, BAND_MINOR);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#torus").unwrap();
    let face = FaceId::mint("test:model:face#band").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                big,
                small,
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    let mut loops = Vec::new();
    for (index, (u, sense)) in [(FRAC_PI_2, Sense::Forward), (-FRAC_PI_2, Sense::Reversed)]
        .into_iter()
        .enumerate()
    {
        let point = PointId::mint(format!("test:model:point#{index}")).unwrap();
        let vertex = VertexId::mint(format!("test:model:vertex#{index}")).unwrap();
        let curve = CurveId::mint(format!("test:model:curve#{index}")).unwrap();
        let edge = EdgeId::mint(format!("test:model:edge#{index}")).unwrap();
        let coedge = CoedgeId::mint(format!("test:model:coedge#{index}")).unwrap();
        let loop_id = LoopId::mint(format!("test:model:loop#{index}")).unwrap();
        ir.geometry.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(
                (big + small) * u.cos(),
                (big + small) * u.sin(),
                0.0,
            ))
            .unwrap(),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex.clone(),
            point,
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.curves.push(Curve {
            id: curve.clone(),
            source_object: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(big * u.cos(), big * u.sin(), 0.0),
                    Vector3::new(u.sin(), -u.cos(), 0.0),
                    Vector3::new(u.cos(), u.sin(), 0.0),
                    small,
                )
                .unwrap(),
            )),
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::new(Some(curve), Some([0.0, TAU])).unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(vertex.clone(), vertex)),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next: coedge.clone(),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
        ir.geometry.loops.push(Loop {
            id: loop_id.clone(),
            face: face.clone(),
            boundary: LoopBoundary::Ring(LoopRing::new(vec![coedge], Vec::new()).unwrap()),
            native_ref: None,
        });
        loops.push(loop_id);
    }
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:model:shell#band").unwrap(),
        surface,
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(loops),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    (ir, face)
}

/// The band's area is `π · 2π r R`.
#[test]
fn a_torus_band_across_its_seam_meshes_its_area() {
    let (ir, face) = tube_band();
    let mut tessellator = Tessellator::new(&ir, Options::default());
    let mesh = tessellator.face(&face).unwrap();
    let expected = 2.0 * std::f64::consts::PI.powi(2) * BAND_MAJOR * BAND_MINOR;
    assert!(
        (area(&mesh) - expected).abs() <= FACETED_AREA_RATIO * expected,
        "area {}, expected {expected}",
        area(&mesh)
    );
}

/// The square `[0.5, 1.5]²` at `z = 0`, traced counterclockwise about `+z`
/// as the declared outer loop of a face on the bilinear NURBS patch
/// `[0, 2]²`, whose normal is `+z`, with the face sense `sense`.
fn square_on_patch(
    sense: cadmpeg_ir::topology::Sense,
) -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId) {
    let corner = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let square = [
        corner(0.5, 0.5),
        corner(1.5, 0.5),
        corner(1.5, 1.5),
        corner(0.5, 1.5),
    ];
    quad_on_patch(square, sense, true)
}

/// The quadrilateral `corners` at `z = 0`, traced in corner order as the
/// one loop of a face on the bilinear NURBS patch `[0, 2]²`, whose normal
/// is `+z`, with the face sense `sense`. The loop is the face's declared
/// outer loop when `declared` holds.
fn quad_on_patch(
    corners: [Point3; 4],
    sense: cadmpeg_ir::topology::Sense,
    declared: bool,
) -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId) {
    use cadmpeg_ir::geometry::analytic::LineCurve;
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense,
        Vertex,
    };

    let mut ir = cadmpeg_ir::CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#patch").unwrap();
    let face = FaceId::mint("test:model:face#square").unwrap();
    let loop_id = LoopId::mint("test:model:loop#square").unwrap();
    let pole = |x: f64, y: f64| Point3::new(x, y, 0.0);
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::new(
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsPoleGrid::from_lanes(
                    vec![
                        vec![pole(0.0, 0.0), pole(0.0, 2.0)],
                        vec![pole(2.0, 0.0), pole(2.0, 2.0)],
                    ],
                    None,
                )
                .unwrap(),
                false,
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    let vertex = |index: usize| VertexId::mint(format!("test:model:vertex#{index}")).unwrap();
    for (index, corner) in corners.iter().enumerate() {
        let point = PointId::mint(format!("test:model:point#{index}")).unwrap();
        ir.geometry.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(*corner).unwrap(),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex(index),
            point,
            tolerance: None,
            native_ref: None,
        });
    }
    let mut ring = Vec::new();
    for start in 0..4 {
        let end = (start + 1) % 4;
        let curve = CurveId::mint(format!("test:model:curve#{start}")).unwrap();
        let edge = EdgeId::mint(format!("test:model:edge#{start}")).unwrap();
        let coedge = CoedgeId::mint(format!("test:model:coedge#{start}")).unwrap();
        let chord = sub(corners[end], corners[start]);
        ir.geometry.curves.push(Curve {
            id: curve.clone(),
            source_object: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
                cadmpeg_ir::features::geometry::FinitePoint3::new(corners[start]).unwrap(),
                cadmpeg_ir::units::UnitVector3::normalized(chord).unwrap(),
            ))),
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::new(Some(curve), Some([0.0, chord.norm()])).unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex(start),
                vertex(end),
            )),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next: coedge.clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
        ring.push(coedge);
    }
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).unwrap()),
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:model:shell#square").unwrap(),
        surface,
        sense,
        loops: if declared {
            FaceLoops::classified(loop_id, Vec::new())
        } else {
            FaceLoops::unspecified(vec![loop_id])
        },
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    (ir, face)
}

#[test]
fn a_declared_outer_loop_against_the_sense_of_a_patch_bounds_its_face() {
    use cadmpeg_ir::topology::Sense;

    for sense in [Sense::Forward, Sense::Reversed] {
        let (ir, face) = square_on_patch(sense);
        let mesh = Tessellator::new(&ir, Options::default())
            .face(&face)
            .unwrap();
        assert!(
            (area(&mesh) - 1.0).abs() <= AREA_TOLERANCE,
            "{sense:?}: area {}",
            area(&mesh)
        );
        for triangle in &mesh.triangles {
            assert!(facing(&mesh, *triangle).z > 0.0, "{sense:?}");
        }
    }
}

/// A face whose one loop runs clockwise about its oriented normal bounds
/// the patch less the loop. When the loop runs within a thousandth of the
/// patch's sides, that remainder splits apart, so the face is the region
/// the loop encloses.
#[test]
fn a_loop_against_the_face_across_its_patch_bounds_the_region_it_encloses() {
    use cadmpeg_ir::topology::Sense;

    let corner = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let strip = [
        corner(0.001, 0.001),
        corner(0.5, 0.001),
        corner(0.5, 1.999),
        corner(0.001, 1.999),
    ];
    let (ir, face) = quad_on_patch(strip, Sense::Reversed, false);
    let mesh = Tessellator::new(&ir, Options::default())
        .face(&face)
        .unwrap();
    assert!(
        (area(&mesh) - 0.499 * 1.998).abs() <= AREA_TOLERANCE,
        "area {}",
        area(&mesh)
    );
    for triangle in &mesh.triangles {
        assert!(facing(&mesh, *triangle).z > 0.0);
    }
}

/// A loop inside a patch that runs clockwise about the face's oriented
/// normal is a hole in the whole patch.
#[test]
fn a_loop_against_the_face_inside_its_patch_is_a_hole() {
    use cadmpeg_ir::topology::Sense;

    let corner = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let square = [
        corner(0.5, 0.5),
        corner(1.5, 0.5),
        corner(1.5, 1.5),
        corner(0.5, 1.5),
    ];
    let (ir, face) = quad_on_patch(square, Sense::Reversed, false);
    let mesh = Tessellator::new(&ir, Options::default())
        .face(&face)
        .unwrap();
    assert!(
        (area(&mesh) - 3.0).abs() <= AREA_TOLERANCE,
        "area {}",
        area(&mesh)
    );
}

const NOTCH_RADIUS: f64 = 11.43;
const NOTCH_HEIGHT: f64 = 0.348;
const NOTCH_OFFSET: f64 = 10.3124;
const NOTCH_HALF_ANGLE: f64 = 75.0 * std::f64::consts::PI / 180.0;

/// The half band `y ≤ 0` of the cone of half angle `NOTCH_HALF_ANGLE` that
/// narrows up `+z` from radius `NOTCH_RADIUS` at `z = 0`, between its base
/// circle and its circle at `z = NOTCH_HEIGHT`, less the part beyond the
/// plane `y = -NOTCH_OFFSET`. That plane cuts the band's outer side in a
/// hyperbola and misses the inner circle. The declared outer loop runs
/// along the base arcs and the hyperbola, up the generator at `x > 0`, back
/// along the inner arc, and down the generator at `x < 0`.
fn notched_band(
    sense: cadmpeg_ir::topology::Sense,
) -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId) {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, ConeSurface, HyperbolaCurve, LineCurve};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense,
        Vertex,
    };
    use std::f64::consts::{PI, TAU};

    let (radius, height, offset) = (NOTCH_RADIUS, NOTCH_HEIGHT, NOTCH_OFFSET);
    let slope = NOTCH_HALF_ANGLE.tan();
    let inner = radius - height * slope;
    let along = (radius * radius - offset * offset).sqrt();
    let corners = [
        Point3::new(-radius, 0.0, 0.0),
        Point3::new(-along, -offset, 0.0),
        Point3::new(along, -offset, 0.0),
        Point3::new(radius, 0.0, 0.0),
        Point3::new(inner, 0.0, height),
        Point3::new(-inner, 0.0, height),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#cone").unwrap();
    let face = FaceId::mint("test:model:face#notch").unwrap();
    let loop_id = LoopId::mint("test:model:loop#notch").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            ConeSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
                1.0,
                NOTCH_HALF_ANGLE,
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    let vertex = |index: usize| VertexId::mint(format!("test:model:vertex#{index}")).unwrap();
    for (index, corner) in corners.iter().enumerate() {
        let point = PointId::mint(format!("test:model:point#{index}")).unwrap();
        ir.geometry.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(*corner).unwrap(),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex(index),
            point,
            tolerance: None,
            native_ref: None,
        });
    }
    let circle = |z: f64, r: f64| {
        SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                r,
            )
            .unwrap(),
        )
    };
    let line = |from: Point3, to: Point3| {
        let chord = sub(to, from);
        (
            SolvedCurveGeometry::Line(LineCurve::new(
                cadmpeg_ir::features::geometry::FinitePoint3::new(from).unwrap(),
                cadmpeg_ir::units::UnitVector3::normalized(chord).unwrap(),
            )),
            [0.0, chord.norm()],
        )
    };
    // The plane meets the base circle at angles `π + θ` and `2π - θ`; the
    // hyperbola `c + (offset / tan α) cosh t (-z) + offset sinh t x` meets
    // it at `cosh t = radius / offset`.
    let low = (offset / radius).asin();
    let reach = (radius / offset).acosh();
    let rise = line(corners[3], corners[4]);
    let fall = line(corners[5], corners[0]);
    let edges = [
        (circle(0.0, radius), [PI, PI + low], (0, 1), Sense::Forward),
        (
            SolvedCurveGeometry::Hyperbola(
                HyperbolaCurve::try_new(
                    Point3::new(0.0, -offset, radius / slope),
                    Vector3::new(0.0, -1.0, 0.0),
                    Vector3::new(0.0, 0.0, -1.0),
                    offset / slope,
                    offset,
                )
                .unwrap(),
            ),
            [-reach, reach],
            (1, 2),
            Sense::Forward,
        ),
        (
            circle(0.0, radius),
            [TAU - low, TAU],
            (2, 3),
            Sense::Forward,
        ),
        (rise.0, rise.1, (3, 4), Sense::Forward),
        (circle(height, inner), [PI, TAU], (5, 4), Sense::Reversed),
        (fall.0, fall.1, (5, 0), Sense::Forward),
    ];
    let mut ring = Vec::new();
    for (index, (geometry, range, (start, end), coedge_sense)) in edges.into_iter().enumerate() {
        let curve = CurveId::mint(format!("test:model:curve#{index}")).unwrap();
        let edge = EdgeId::mint(format!("test:model:edge#{index}")).unwrap();
        let coedge = CoedgeId::mint(format!("test:model:coedge#{index}")).unwrap();
        ir.geometry.curves.push(Curve {
            id: curve.clone(),
            source_object: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(geometry),
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::new(Some(curve), Some(range)).unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex(start),
                vertex(end),
            )),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next: coedge.clone(),
            sense: coedge_sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
        ring.push(coedge);
    }
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).unwrap()),
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:model:shell#notch").unwrap(),
        surface,
        sense,
        loops: FaceLoops::classified(loop_id, Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    (ir, face)
}

/// The face projects along the axis onto the half annulus between the two
/// circles less the base circle's segment beyond the plane, and a cone of
/// half angle `α` scales projected area by `1 / sin α`. Refinement keeps
/// every triangle off the hyperbola's chords, which the cone bows past.
#[test]
fn a_cone_band_notched_along_its_axis_meshes_its_area_unfolded() {
    use cadmpeg_ir::topology::Sense;

    let (radius, offset) = (NOTCH_RADIUS, NOTCH_OFFSET);
    let inner = radius - NOTCH_HEIGHT * NOTCH_HALF_ANGLE.tan();
    let band = std::f64::consts::FRAC_PI_2 * (radius * radius - inner * inner);
    let segment = radius * radius * (offset / radius).acos()
        - offset * (radius * radius - offset * offset).sqrt();
    let expected = (band - segment) / NOTCH_HALF_ANGLE.sin();
    for sense in [Sense::Forward, Sense::Reversed] {
        let (ir, face) = notched_band(sense);
        let mesh = Tessellator::new(&ir, Options::default())
            .face(&face)
            .unwrap();
        let found = area(&mesh);
        assert!(
            (found - expected).abs() <= FACETED_AREA_RATIO * expected,
            "{sense:?}: area {found}, expected {expected}"
        );
        let normal = facing(&mesh, mesh.triangles[0]);
        for triangle in &mesh.triangles {
            assert!(dot(facing(&mesh, *triangle), normal) > 0.0, "{sense:?}");
        }
    }
}

/// Work units that trace the lune's loop and run out during refinement.
const SHORT_WORK: usize = 10_000;

/// A face whose work runs out keeps the mesh refinement reached while that
/// mesh stays on its surface: every triangle of the sphere faces outward.
#[test]
fn a_face_whose_work_runs_out_keeps_an_unfolded_mesh() {
    let (ir, face) = sphere_less_a_lune();
    let mut tessellator = Tessellator::new(&ir, Options::default());
    let mesh = crate::face::tessellate_within(&mut tessellator, &face, SHORT_WORK).unwrap();
    for triangle in &mesh.triangles {
        let corner = mesh.positions[triangle[0]];
        let outward = Vector3::new(corner.x, corner.y, corner.z);
        assert!(dot(facing(&mesh, *triangle), outward) > 0.0, "{triangle:?}");
    }
}

/// The strip of a quadratic accordion profile folded `folds` times across
/// `x ∈ [0, 1]` between `y = ±1`, swept from `z = 0` to `z = 1`, bounded
/// between the knots `first` and `last` of the profile: along the profile
/// at `z = 0`, up a ruling, back along the profile at `z = 1`, and down.
fn accordion_strip(
    folds: u16,
    first: u16,
    last: u16,
) -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId, Vec<Point3>) {
    use cadmpeg_ir::geometry::analytic::LineCurve;
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, EdgeEnds, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point,
        Sense, Vertex,
    };

    let spans = folds;
    let poles: Vec<Point3> = (0..=spans + 1)
        .map(|pole| {
            let x = f64::from(pole) / f64::from(spans + 1);
            let y = if pole % 2 == 0 { 1.0 } else { -1.0 };
            Point3::new(x, y, 0.0)
        })
        .collect();
    let mut knots = vec![0.0, 0.0];
    knots.extend((0..=spans).map(|step| f64::from(step) / f64::from(spans)));
    knots.extend([1.0, 1.0]);
    let lifted = |point: Point3, z: f64| Point3::new(point.x, point.y, z);
    // A quadratic B-spline with these knots passes through the midpoint of
    // poles `k` and `k + 1` at the knot `k / spans`.
    let at_knot = |k: u16, z: f64| {
        let (a, b) = (poles[usize::from(k)], poles[usize::from(k) + 1]);
        Point3::new(f64::midpoint(a.x, b.x), f64::midpoint(a.y, b.y), z)
    };
    let corners = [
        at_knot(first, 0.0),
        at_knot(last, 0.0),
        at_knot(last, 1.0),
        at_knot(first, 1.0),
    ];
    let range = [
        f64::from(first) / f64::from(spans),
        f64::from(last) / f64::from(spans),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#accordion").unwrap();
    let face = FaceId::mint("test:model:face#accordion").unwrap();
    let loop_id = LoopId::mint("test:model:loop#accordion").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::new(
                NurbsSurfaceAxis::new(2, knots.clone(), false),
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsPoleGrid::from_lanes(
                    poles
                        .iter()
                        .map(|pole| vec![lifted(*pole, 0.0), lifted(*pole, 1.0)])
                        .collect(),
                    None,
                )
                .unwrap(),
                false,
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    let vertex = |index: usize| VertexId::mint(format!("test:model:vertex#{index}")).unwrap();
    for (index, corner) in corners.iter().enumerate() {
        let point = PointId::mint(format!("test:model:point#{index}")).unwrap();
        ir.geometry.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(*corner).unwrap(),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex(index),
            point,
            tolerance: None,
            native_ref: None,
        });
    }
    let profile = |z: f64| {
        SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                2,
                knots.clone(),
                poles.iter().map(|pole| lifted(*pole, z)).collect(),
                None,
                false,
            )
            .unwrap(),
        )
    };
    let ruling = |at: Point3| {
        SolvedCurveGeometry::Line(LineCurve::new(
            cadmpeg_ir::features::geometry::FinitePoint3::new(at).unwrap(),
            cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        ))
    };
    let edges = [
        (profile(0.0), range, (0, 1), Sense::Forward),
        (ruling(corners[1]), [0.0, 1.0], (1, 2), Sense::Forward),
        (profile(1.0), range, (3, 2), Sense::Reversed),
        (ruling(corners[0]), [0.0, 1.0], (0, 3), Sense::Reversed),
    ];
    let mut ring = Vec::new();
    for (index, (geometry, range, (start, end), sense)) in edges.into_iter().enumerate() {
        let curve = CurveId::mint(format!("test:model:curve#{index}")).unwrap();
        let edge = EdgeId::mint(format!("test:model:edge#{index}")).unwrap();
        let coedge = CoedgeId::mint(format!("test:model:coedge#{index}")).unwrap();
        ir.geometry.curves.push(Curve {
            id: curve.clone(),
            source_object: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(geometry),
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::new(Some(curve), Some(range)).unwrap(),
            ends: Some(EdgeEnds::new(vertex(start), vertex(end))),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next: coedge.clone(),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
        ring.push(coedge);
    }
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).unwrap()),
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:model:shell#accordion").unwrap(),
        surface,
        sense: Sense::Forward,
        loops: FaceLoops::classified(loop_id, Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    (ir, face, poles)
}

/// Samples per span of the arc-length sum.
const ARC_SAMPLES: u16 = 2000;

/// The length of the quadratic accordion profile `poles` between the knots
/// `first` and `last`: span `k` is the Bézier arc from the midpoint of poles
/// `k` and `k + 1` through pole `k + 1` to the midpoint of poles `k + 1` and
/// `k + 2`.
fn profile_length(poles: &[Point3], first: u16, last: u16) -> f64 {
    let mid =
        |a: Point3, b: Point3| Point3::new(f64::midpoint(a.x, b.x), f64::midpoint(a.y, b.y), 0.0);
    let mut length = 0.0;
    for span in first..last {
        let pole = usize::from(span);
        let (start, control, end) = (
            mid(poles[pole], poles[pole + 1]),
            poles[pole + 1],
            mid(poles[pole + 1], poles[pole + 2]),
        );
        let at = |t: f64| {
            let (before, between, after) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
            Point3::new(
                before * start.x + between * control.x + after * end.x,
                before * start.y + between * control.y + after * end.y,
                0.0,
            )
        };
        let mut previous = at(0.0);
        for step in 1..=ARC_SAMPLES {
            let next = at(f64::from(step) / f64::from(ARC_SAMPLES));
            length += next.distance(previous);
            previous = next;
        }
    }
    length
}

/// A strip of an accordion folded more often than the dense start grid
/// steps: the loop's first sample, which has no seed, can invert onto a
/// neighbouring fold. The loop's end seeds it again, and the strip meshes
/// its profile length times its unit height.
#[test]
fn a_loop_start_on_a_closely_folded_surface_inverts_from_the_loop_end() {
    for (folds, first, last) in [(150_u16, 5_u16, 15_u16), (600, 588, 598)] {
        let (ir, face, poles) = accordion_strip(folds, first, last);
        let mesh = Tessellator::new(&ir, Options::default())
            .face(&face)
            .unwrap();
        let expected = profile_length(&poles, first, last);
        let found = area(&mesh);
        assert!(
            (found - expected).abs() <= FACETED_AREA_RATIO * expected,
            "{folds} folds: area {found}, expected {expected}"
        );
    }
}

const SEGMENT_RADIUS: f64 = 30.0;
const SEGMENT_HALF_ANGLE: f64 = 0.015;

/// The circular segment of `z = 0` between the arc of radius
/// `SEGMENT_RADIUS` about the origin from angle `-SEGMENT_HALF_ANGLE` to
/// `SEGMENT_HALF_ANGLE` and its chord: one loop of an arc and a line.
fn circular_segment() -> (cadmpeg_ir::CadIr, cadmpeg_ir::ids::FaceId) {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, LineCurve, PlaneSurface};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense,
        Vertex,
    };

    let (r, a) = (SEGMENT_RADIUS, SEGMENT_HALF_ANGLE);
    let ends = [
        Point3::new(r * a.cos(), -r * a.sin(), 0.0),
        Point3::new(r * a.cos(), r * a.sin(), 0.0),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let surface = SurfaceId::mint("test:model:surface#plane").unwrap();
    let face = FaceId::mint("test:model:face#segment").unwrap();
    let loop_id = LoopId::mint("test:model:loop#segment").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        source_object: None,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        native_ref: None,
    });
    let vertex = |index: usize| VertexId::mint(format!("test:model:vertex#{index}")).unwrap();
    for (index, end) in ends.iter().enumerate() {
        let point = PointId::mint(format!("test:model:point#{index}")).unwrap();
        ir.geometry.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::geometry::FinitePoint3::new(*end).unwrap(),
            None,
        ));
        ir.geometry.vertices.push(Vertex {
            id: vertex(index),
            point,
            tolerance: None,
            native_ref: None,
        });
    }
    // The arc rises from end 0 to end 1 and the chord falls back, so the
    // loop runs counterclockwise about `+z`.
    let sides = [
        (
            SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new((-a).cos(), (-a).sin(), 0.0),
                    r,
                )
                .unwrap(),
            ),
            [0.0, 2.0 * a],
            [0, 1],
        ),
        (
            SolvedCurveGeometry::Line(
                LineCurve::try_new(ends[1], Vector3::new(0.0, -1.0, 0.0)).unwrap(),
            ),
            [0.0, 2.0 * r * a.sin()],
            [1, 0],
        ),
    ];
    let mut ring = Vec::new();
    for (index, (geometry, range, [start, end])) in sides.into_iter().enumerate() {
        let curve = CurveId::mint(format!("test:model:curve#{index}")).unwrap();
        let edge = EdgeId::mint(format!("test:model:edge#{index}")).unwrap();
        let coedge = CoedgeId::mint(format!("test:model:coedge#{index}")).unwrap();
        ir.geometry.curves.push(Curve {
            id: curve.clone(),
            source_object: None,
            parameter_range: None,
            geometry: CurveGeometry::Solved(geometry),
            native_ref: None,
        });
        ir.geometry.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::new(Some(curve), Some(range)).unwrap(),
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(
                vertex(start),
                vertex(end),
            )),
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next: coedge.clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
        ring.push(coedge);
    }
    ir.geometry.loops.push(Loop {
        id: loop_id.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).unwrap()),
        native_ref: None,
    });
    ir.geometry.faces.push(Face {
        id: face.clone(),
        shell: ShellId::mint("test:model:shell#segment").unwrap(),
        surface,
        sense: Sense::Forward,
        loops: FaceLoops::classified(loop_id, Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    (ir, face)
}

/// The arc's sagitta is below the chord tolerance, so its one interior
/// sample is its midpoint: the mesh is the triangle on the chord with that
/// apex, of area `chord · sagitta / 2`.
#[test]
fn a_circular_segment_flatter_than_the_chord_tolerance_meshes_its_triangle() {
    let (ir, face) = circular_segment();
    let options = Options::default();
    let (r, a) = (SEGMENT_RADIUS, SEGMENT_HALF_ANGLE);
    let sagitta = r * (1.0 - a.cos());
    assert!(sagitta < options.chord);
    let mesh = Tessellator::new(&ir, options).face(&face).unwrap();
    let triangle = r * a.sin() * sagitta;
    assert!(
        (area(&mesh) - triangle).abs() <= AREA_TOLERANCE * triangle,
        "area {} against {triangle}",
        area(&mesh)
    );
}

#[test]
fn a_partial_face_has_no_mesh() {
    let mut cube = cadmpeg_ir::examples::unit_cube().unwrap();
    let stated = cube.geometry.faces[0]
        .loops
        .stated()
        .cloned()
        .collect::<Vec<_>>();
    cube.geometry.faces[0].loops =
        cadmpeg_ir::topology::FaceLoops::PartialUnspecified { loops: stated };
    let face = cube.geometry.faces[0].id.clone();
    let mut tessellator = Tessellator::new(&cube, Options::default());
    assert!(tessellator.face(&face).is_err());
    let other = cube.geometry.faces[1].id.clone();
    assert!(tessellator.face(&other).is_ok());
}
