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
