// SPDX-License-Identifier: Apache-2.0

use crate::reason::UnavailableReason;
use crate::select::Selection;
use crate::test_support::builders::{assert_measured, measure, Builder};
use crate::value::Quantity;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{BodyKind, Sense};

fn bipyramid(gap: f64) -> cadmpeg_ir::CadIr {
    let mut builder = Builder::new();
    let h = 3.0_f64.sqrt();
    let rows = [
        vec![([0.0, 0.0, 3.0], 1.0); 4],
        vec![
            ([2.0, 0.0, 0.0], 1.0),
            ([-1.0, h, 0.0], 1.0),
            ([-1.0, -h, 0.0], 1.0),
            ([2.0 + gap, 0.0, 0.0], 1.0),
        ],
        vec![([0.0, 0.0, -3.0], 1.0); 4],
    ];
    let surface = builder.nurbs(
        (1, vec![0.0, 0.0, 1.0, 2.0, 2.0]),
        (1, vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0]),
        &rows,
    );
    let north = builder.vertex([0.0, 0.0, 3.0]);
    let south = builder.vertex([0.0, 0.0, -3.0]);
    builder.vertex_face(&surface, Sense::Forward, &[north, south]);
    builder.finish(BodyKind::Solid)
}

#[test]
fn a_nurbs_domain_with_two_poles_and_a_seam_encloses_its_volume() {
    let report = measure(&bipyramid(0.0), &Selection::default());
    let body = &report.bodies()[0];
    assert!(*body.shells()[0].closed());
    assert_measured(
        body.volume().as_ref().expect("volume"),
        6.0 * 3.0_f64.sqrt(),
        27.0,
    );
}

#[test]
fn pole_loops_keep_an_open_nurbs_seam_open() {
    let report = measure(&bipyramid(0.125), &Selection::default());
    let body = &report.bodies()[0];
    assert!(!*body.shells()[0].closed());
    let Quantity::Unavailable(volume) = body.volume().as_ref().expect("volume") else {
        panic!("open seam");
    };
    assert_eq!(volume.reason(), UnavailableReason::VolumeShellOpen);
}

#[test]
fn a_loopless_doubly_periodic_nurbs_torus_encloses_its_volume() {
    let mut builder = Builder::new();
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let circle = [
        (1.0, 0.0, 1.0),
        (1.0, 1.0, h),
        (0.0, 1.0, 1.0),
        (-1.0, 1.0, h),
        (-1.0, 0.0, 1.0),
        (-1.0, -1.0, h),
        (0.0, -1.0, 1.0),
        (1.0, -1.0, h),
        (1.0, 0.0, 1.0),
    ];
    let grid = circle
        .iter()
        .map(|(r, z, _)| {
            circle
                .iter()
                .map(|(x, y, _)| Point3::new((7.0 + 2.0 * r) * x, (7.0 + 2.0 * r) * y, 2.0 * z))
                .collect()
        })
        .collect();
    let weights = circle
        .iter()
        .map(|(_, _, u)| circle.iter().map(|(_, _, v)| u * v).collect())
        .collect();
    let axis = || {
        NurbsSurfaceAxis::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
            true,
        )
    };
    let surface = builder.surface(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            axis(),
            axis(),
            NurbsSurfaceLanes::new(grid, Some(weights)),
            false,
        )
        .expect("torus"),
    ));
    builder.face(&surface, Sense::Reversed, &[], false);
    let report = measure(&builder.finish(BodyKind::Solid), &Selection::default());
    let body = &report.bodies()[0];
    assert!(*body.shells()[0].closed());
    assert_measured(
        body.volume().as_ref().expect("volume"),
        56.0 * std::f64::consts::PI.powi(2),
        343.0,
    );
}

/// A clamped degree-1 prism over the triangle `(0, 0)`, `(4, 0)`, `(0, 3)`
/// of height 2, whose first and last pole columns are both the corner at
/// the origin, with one face from the middle of the third side across that
/// seam to the middle of the first side.
fn prism_across_its_seam() -> cadmpeg_ir::CadIr {
    let mut builder = Builder::new();
    let corners = [[0.0, 0.0], [4.0, 0.0], [0.0, 3.0], [0.0, 0.0]];
    let rows: Vec<_> = corners
        .iter()
        .map(|&[x, y]| vec![([x, y, 0.0], 1.0), ([x, y, 2.0], 1.0)])
        .collect();
    let surface = builder.nurbs(
        (1, vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0]),
        (1, vec![0.0, 0.0, 1.0, 1.0]),
        &rows,
    );
    let at = |[x, y]: [f64; 2], z: f64| [x, y, z];
    let (third, first) = ([0.0, 1.5], [2.0, 0.0]);
    let a = builder.vertex(at(third, 0.0));
    let b = builder.vertex(at(first, 0.0));
    let c = builder.vertex(at(first, 2.0));
    let d = builder.vertex(at(third, 2.0));
    let bottom = builder.polyline(&[at(third, 0.0), [0.0, 0.0, 0.0], at(first, 0.0)], &a, &b);
    let rising = builder.line(&b, &c);
    let top = builder.polyline(&[at(first, 2.0), [0.0, 0.0, 2.0], at(third, 2.0)], &c, &d);
    let falling = builder.line(&d, &a);
    builder.face(
        &surface,
        Sense::Forward,
        &[vec![
            (bottom, Sense::Forward),
            (rising, Sense::Forward),
            (top, Sense::Forward),
            (falling, Sense::Forward),
        ]],
        true,
    );
    builder.finish(BodyKind::Sheet)
}

#[test]
fn a_face_across_the_seam_of_a_clamped_closed_nurbs_measures_its_area() {
    let selection = Selection {
        all_faces: true,
        ..Selection::default()
    };
    let report = measure(&prism_across_its_seam(), &selection);
    let [face] = report.faces().as_slice() else {
        panic!("one face");
    };
    // The two half sides from the corner, times the height.
    let area = (1.5 + 2.0) * 2.0;
    assert_measured(face.measures().area(), area, area);
}
