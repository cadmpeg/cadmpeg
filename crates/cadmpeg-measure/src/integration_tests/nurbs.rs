// SPDX-License-Identifier: Apache-2.0
//! NURBS faces: a whole bilinear patch, a rational quarter cylinder bounded
//! by its 3D edges, and unspecified loops whose region rests on the declared
//! orientation, with expectations from geometry.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::{EdgeId, SurfaceId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::reason::{CaveatCode, UnavailableReason};
use crate::report::{FaceMeasures, MeasureReport, RegionRule, WitnessKind};
use crate::select::Selection;
use crate::test_support::builders::{
    assert_measured, assert_range, measure, Builder, Circle, Frame,
};
use crate::value::{BoxKind, Quantity, Reading};

fn faces() -> Selection {
    Selection {
        all_faces: true,
        ..Selection::default()
    }
}

fn only_face(report: &MeasureReport) -> &FaceMeasures {
    let [face] = report.faces().as_slice() else {
        panic!("one face row");
    };
    face
}

/// The bilinear patch `(x, y) = (2u, 2v)` on `[0, 1]²` at `z = 0`.
fn square_patch(builder: &mut Builder) -> SurfaceId {
    builder.nurbs(
        (1, vec![0.0, 0.0, 1.0, 1.0]),
        (1, vec![0.0, 0.0, 1.0, 1.0]),
        &[
            vec![([0.0, 0.0, 0.0], 1.0), ([0.0, 2.0, 0.0], 1.0)],
            vec![([2.0, 0.0, 0.0], 1.0), ([2.0, 2.0, 0.0], 1.0)],
        ],
    )
}

/// A counterclockwise square ring of line edges through `corners`.
fn square_ring(builder: &mut Builder, corners: [[f64; 2]; 4]) -> Vec<(EdgeId, Sense)> {
    let vertices: Vec<VertexId> = corners
        .iter()
        .map(|[x, y]| builder.vertex([*x, *y, 0.0]))
        .collect();
    (0..4)
        .map(|index| {
            (
                builder.line(&vertices[index], &vertices[(index + 1) % 4]),
                Sense::Forward,
            )
        })
        .collect()
}

#[test]
fn a_face_without_loops_covers_its_whole_patch() {
    let mut builder = Builder::new();
    let surface = square_patch(&mut builder);
    builder.face(&surface, Sense::Forward, &[], false);
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    let measures = face.measures();
    assert_measured(measures.area(), 4.0, 4.0);
    assert_eq!(*measures.region(), Some(RegionRule::WholeSurface));
    let bbox = measures.bbox().measured().expect("box");
    assert_eq!(bbox.value().kind, BoxKind::Conservative);
    assert_eq!(bbox.value().max.get().x, 2.0);
    assert_eq!(bbox.value().max.get().y, 2.0);
    assert_range(measures.u_range().as_ref(), [0.0, 1.0], 1.0);
    assert_range(measures.v_range().as_ref(), [0.0, 1.0], 1.0);
}

/// An unspecified loop inside the patch, listed counterclockwise.
fn patch_with_loop(sense: Sense, corners: [[f64; 2]; 4]) -> CadIr {
    let mut builder = Builder::new();
    let surface = square_patch(&mut builder);
    let ring = square_ring(&mut builder, corners);
    builder.face(&surface, sense, &[ring], false);
    builder.finish(BodyKind::Sheet)
}

const INNER: [[f64; 2]; 4] = [[0.5, 0.5], [1.5, 0.5], [1.5, 1.5], [0.5, 1.5]];

#[test]
fn an_unspecified_loop_reads_its_declared_side_with_the_complement() {
    for (sense, declared, complement) in [(Sense::Forward, 1.0, 3.0), (Sense::Reversed, 3.0, 1.0)] {
        let ir = patch_with_loop(sense, INNER);
        let report = measure(&ir, &faces());
        let face = only_face(&report);
        let measures = face.measures();
        assert_eq!(*measures.region(), Some(RegionRule::DeclaredOrientation));
        assert_measured(measures.area(), declared, 4.0);
        let area = measures.area().measured().expect("area");
        assert_eq!(area.reading(), Some(Reading::Declared));
        let [candidate] = area.candidates() else {
            panic!("one candidate");
        };
        assert_eq!(candidate.reading(), Reading::Complement);
        let numeric = candidate.numeric_error().get();
        assert!((candidate.value().value.get() - complement).abs() <= numeric + 1.0e-9 * 4.0);
        assert_eq!(*face.orientation().consistent(), None);
        assert!(face
            .caveats()
            .iter()
            .any(|caveat| caveat.code() == CaveatCode::OrientationUnconfirmed));
    }
}

#[test]
fn a_loop_on_the_domain_boundary_is_confirmed_by_the_empty_complement() {
    let ir = patch_with_loop(
        Sense::Forward,
        [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]],
    );
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    assert_measured(face.measures().area(), 4.0, 4.0);
    let area = face.measures().area().measured().expect("area");
    assert_eq!(area.reading(), None);
    let witness = face.orientation().witness().as_ref().expect("witness");
    assert_eq!(*witness.kind(), WitnessKind::EmptyReading);
    assert!(*witness.agrees());
    assert_eq!(*face.orientation().consistent(), Some(true));
    assert!(face.caveats().is_empty());
}

/// The quarter cylinder `r = 10`, `z ∈ [0, 30]`, `u` from `+x` to `+y`, as
/// a rational quadratic by linear patch bounded by its 3D edges.
#[test]
fn a_rational_quarter_cylinder_inverts_its_edges() {
    let radius = 10.0;
    let height = 30.0;
    let mut builder = Builder::new();
    let column =
        |x: f64, y: f64, weight: f64| vec![([x, y, 0.0], weight), ([x, y, height], weight)];
    let surface = builder.nurbs(
        (2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
        (1, vec![0.0, 0.0, 1.0, 1.0]),
        &[
            column(radius, 0.0, 1.0),
            column(radius, radius, FRAC_1_SQRT_2),
            column(0.0, radius, 1.0),
        ],
    );
    let a = builder.vertex([radius, 0.0, 0.0]);
    let b = builder.vertex([0.0, radius, 0.0]);
    let c = builder.vertex([0.0, radius, height]);
    let d = builder.vertex([radius, 0.0, height]);
    let arc = |z: f64| Circle {
        frame: Frame::z_up([0.0, 0.0, z]),
        radius,
    };
    let bottom = builder.arc(arc(0.0), [0.0, PI / 2.0], &a, &b);
    let right = builder.line(&b, &c);
    let top = builder.arc(arc(height), [0.0, PI / 2.0], &d, &c);
    let left = builder.line(&a, &d);
    builder.face(
        &surface,
        Sense::Forward,
        &[vec![
            (bottom, Sense::Forward),
            (right, Sense::Forward),
            (top, Sense::Reversed),
            (left, Sense::Reversed),
        ]],
        true,
    );
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let face = only_face(&report);
    let measures = face.measures();
    assert_eq!(*measures.region(), Some(RegionRule::Nesting));
    assert_measured(measures.area(), 150.0 * PI, radius * height);
    assert_measured(measures.boundary_length(), 10.0 * PI + 60.0, height);
    assert_eq!(*face.orientation().consistent(), Some(true));
    let Quantity::Measured(bbox) = measures.bbox() else {
        panic!("a measured box");
    };
    assert_eq!(bbox.value().kind, BoxKind::Conservative);
}

/// The triangle `(0,0,0)`, `(1,0,0)`, `(0,1,0)` as a bilinear patch whose
/// `v = 1` side collapses to the origin, bounded by its three straight sides
/// from each corner in turn: the patch is the whole face, of area 1/2. At
/// the collapsed side `u` is free, so each side meets it at the `u` of its
/// own trace.
#[test]
fn a_patch_with_a_collapsed_side_measures_its_whole_area() {
    let (apex, first_corner, second_corner) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    for start in 0..3 {
        let mut builder = Builder::new();
        let surface = builder.nurbs(
            (1, vec![0.0, 0.0, 1.0, 1.0]),
            (1, vec![0.0, 0.0, 1.0, 1.0]),
            &[
                vec![(first_corner, 1.0), (apex, 1.0)],
                vec![(second_corner, 1.0), (apex, 1.0)],
            ],
        );
        let vertices: Vec<_> = [first_corner, second_corner, apex]
            .iter()
            .map(|corner| builder.vertex(*corner))
            .collect();
        let ring: Vec<_> = (0..3)
            .map(|step| {
                let from = (start + step) % 3;
                (
                    builder.line(&vertices[from], &vertices[(from + 1) % 3]),
                    Sense::Forward,
                )
            })
            .collect();
        builder.face(&surface, Sense::Forward, &[ring], true);
        let ir = builder.finish(BodyKind::Sheet);
        let report = measure(&ir, &faces());
        assert_measured(&report.faces[0].measures.area, 0.5, 1.0);
    }
}

/// The strip `0 ≤ x ≤ 1`, `0 ≤ y ≤ 0.005` as a bilinear patch whose `v`
/// sides are 0.005 long: shorter than the coincidence allowance and longer
/// than the model tolerance, so `u` is defined along them. The patch is the
/// whole face, of area 0.005.
#[test]
fn a_strip_narrower_than_the_allowance_measures_its_whole_area() {
    let width = 0.005;
    assert!(width < cadmpeg_ir::units::COINCIDENCE_TOLERANCE);
    let corners = [
        [0.0, 0.0, 0.0],
        [0.0, width, 0.0],
        [1.0, width, 0.0],
        [1.0, 0.0, 0.0],
    ];
    let mut builder = Builder::new();
    let surface = builder.nurbs(
        (1, vec![0.0, 0.0, 1.0, 1.0]),
        (1, vec![0.0, 0.0, 1.0, 1.0]),
        &[
            vec![(corners[0], 1.0), (corners[3], 1.0)],
            vec![(corners[1], 1.0), (corners[2], 1.0)],
        ],
    );
    let vertices: Vec<_> = corners
        .iter()
        .map(|corner| builder.vertex(*corner))
        .collect();
    let ring: Vec<_> = (0..4)
        .map(|from| {
            (
                builder.line(&vertices[from], &vertices[(from + 1) % 4]),
                Sense::Forward,
            )
        })
        .collect();
    builder.face(&surface, Sense::Forward, &[ring], true);
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    assert_measured(&report.faces[0].measures.area, width, 1.0);
}

/// The rectangle `[0, 2] × [0, 3]` at `z = 0` as a patch linear in `u` and
/// piecewise linear in `v` over ten spans of `v_extent / 10`, with poles at
/// `y = 3 (j / 10)²` so that `S_v` jumps at every knot. Its normal is `+z`.
fn uneven_rectangle(builder: &mut Builder, v_extent: f64) -> SurfaceId {
    let spans = 10;
    let mut v_knots = vec![0.0];
    v_knots.extend((0..=spans).map(|j| v_extent * f64::from(j) / f64::from(spans)));
    v_knots.push(v_extent);
    let row = |x: f64| -> Vec<([f64; 3], f64)> {
        (0..=spans)
            .map(|j| {
                let fraction = f64::from(j) / f64::from(spans);
                ([x, 3.0 * fraction * fraction, 0.0], 1.0)
            })
            .collect()
    };
    builder.nurbs(
        (1, vec![0.0, 0.0, 1.0, 1.0]),
        (1, v_knots),
        &[row(0.0), row(2.0)],
    )
}

/// The triangle `(0,0,0)`, `(2,0,0)`, `(0,3,0)` on the uneven rectangle
/// bounds a face of area 3 whose hypotenuse crosses every `v` knot. The
/// patch has one `u` span and ten `v` spans, so its inner integrals run
/// along `u`.
#[test]
fn a_patch_with_fewer_u_spans_integrates_along_u() {
    for v_extent in [1.0, 0.0015] {
        let mut builder = Builder::new();
        let surface = uneven_rectangle(&mut builder, v_extent);
        let vertices: Vec<_> = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]]
            .iter()
            .map(|corner| builder.vertex(*corner))
            .collect();
        let ring: Vec<_> = (0..3)
            .map(|from| {
                (
                    builder.line(&vertices[from], &vertices[(from + 1) % 3]),
                    Sense::Forward,
                )
            })
            .collect();
        builder.face(&surface, Sense::Forward, &[ring], true);
        let report = measure(&builder.finish(BodyKind::Sheet), &faces());
        assert_measured(only_face(&report).measures().area(), 3.0, 3.0);
    }
}

/// The tetrahedron `A = (0,0,0)`, `B = (2,0,0)`, `C = (0,3,0)`,
/// `D = (0,0,1)` whose base `ABC` lies on the uneven rectangle, reversed
/// so that its normal points out, and whose other faces are planes. Its
/// volume is `2·3·1/6 = 1`.
#[test]
fn a_solid_on_a_patch_with_fewer_u_spans_encloses_its_volume() {
    for v_extent in [1.0, 0.0015] {
        let mut builder = Builder::new();
        let base = uneven_rectangle(&mut builder, v_extent);
        let [a, b, c, d] = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 3.0, 0.0],
            [0.0, 0.0, 1.0],
        ]
        .map(|corner| builder.vertex(corner));
        let ab = builder.line(&a, &b);
        let bc = builder.line(&b, &c);
        let ca = builder.line(&c, &a);
        let ad = builder.line(&a, &d);
        let bd = builder.line(&b, &d);
        let cd = builder.line(&c, &d);
        let x0 = builder.plane([0.0; 3], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let y0 = builder.plane([0.0; 3], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
        let root13 = 13.0_f64.sqrt();
        let slant = builder.plane(
            [2.0, 0.0, 0.0],
            [3.0 / 7.0, 2.0 / 7.0, 6.0 / 7.0],
            [-2.0 / root13, 3.0 / root13, 0.0],
        );
        let (forward, reversed) = (Sense::Forward, Sense::Reversed);
        builder.face(
            &base,
            reversed,
            &[vec![
                (ca.clone(), reversed),
                (bc.clone(), reversed),
                (ab.clone(), reversed),
            ]],
            true,
        );
        builder.face(
            &x0,
            forward,
            &[vec![
                (ad.clone(), forward),
                (cd.clone(), reversed),
                (ca, forward),
            ]],
            true,
        );
        builder.face(
            &y0,
            forward,
            &[vec![(ab, forward), (bd.clone(), forward), (ad, reversed)]],
            true,
        );
        builder.face(
            &slant,
            forward,
            &[vec![(bc, forward), (cd, forward), (bd, reversed)]],
            true,
        );
        let report = measure(&builder.finish(BodyKind::Solid), &Selection::default());
        let [body] = report.bodies().as_slice() else {
            panic!("one body row");
        };
        assert_measured(body.volume().as_ref().expect("volume"), 1.0, 1.0);
    }
}

/// The disk `r ≤ 50` at `z = 0` as a `u`-periodic rational quadratic by
/// linear patch whose `v = 1` side collapses to the centre, bounded by a
/// triangle whose first side runs along `y = 0.005` from `x = -24` to
/// `x = 26`. That side passes within the allowance of the centre between
/// two samples at every sample count up to the cap, where its parameter
/// image turns by about half a period. The side splits at the centre, and
/// the loop reaches the region rule for a periodic NURBS domain.
#[test]
fn a_side_grazing_a_collapsed_nurbs_side_between_samples_splits_there() {
    let radius = 50.0;
    let circle = [
        (1.0, 0.0, 1.0),
        (1.0, 1.0, FRAC_1_SQRT_2),
        (0.0, 1.0, 1.0),
        (-1.0, 1.0, FRAC_1_SQRT_2),
        (-1.0, 0.0, 1.0),
        (-1.0, -1.0, FRAC_1_SQRT_2),
        (0.0, -1.0, 1.0),
        (1.0, -1.0, FRAC_1_SQRT_2),
        (1.0, 0.0, 1.0),
    ];
    let grid = circle
        .iter()
        .map(|(x, y, _)| {
            vec![
                Point3::new(radius * x, radius * y, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ]
        })
        .collect();
    let weights = circle
        .iter()
        .map(|(_, _, weight)| vec![*weight, *weight])
        .collect();
    let mut builder = Builder::new();
    let surface = builder.surface(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
                true,
            ),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(grid, Some(weights)),
            false,
        )
        .expect("disk"),
    ));
    let corners: Vec<VertexId> = [[-24.0, 0.005, 0.0], [26.0, 0.005, 0.0], [0.0, 20.0, 0.0]]
        .iter()
        .map(|corner| builder.vertex(*corner))
        .collect();
    let ring: Vec<_> = (0..3)
        .map(|from| {
            (
                builder.line(&corners[from], &corners[(from + 1) % 3]),
                Sense::Forward,
            )
        })
        .collect();
    builder.face(&surface, Sense::Forward, &[ring], true);
    let ir = builder.finish(BodyKind::Sheet);
    let report = measure(&ir, &faces());
    let area = only_face(&report)
        .measures()
        .area()
        .unavailable()
        .expect("unavailable area");
    assert_eq!(area.reason(), UnavailableReason::RegionDomainUnsupported);
}
