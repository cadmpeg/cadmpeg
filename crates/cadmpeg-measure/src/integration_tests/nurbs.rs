// SPDX-License-Identifier: Apache-2.0
//! NURBS faces: a whole bilinear patch, a rational quarter cylinder bounded
//! by its 3D edges, and unspecified loops whose region rests on the declared
//! orientation, with expectations from geometry.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use cadmpeg_ir::ids::{EdgeId, SurfaceId, VertexId};
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::CadIr;

use crate::reason::CaveatCode;
use crate::report::{FaceMeasures, MeasureReport, RegionRule, WitnessKind};
use crate::select::Selection;
use crate::test_support::{assert_measured, assert_range, measure, Builder, Circle, Frame};
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
