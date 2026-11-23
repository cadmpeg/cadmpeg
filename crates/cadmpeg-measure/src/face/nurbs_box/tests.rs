// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::topology::{BodyKind, Sense};

use crate::select::Selection;
use crate::test_support::builders::{measure, Builder};
use crate::value::{BoxKind, Method};

const EXTENT_TOLERANCE: f64 = 1.0e-6;

#[test]
fn quadratic_patches_bound_the_surface_with_an_interior_extreme() {
    for (heights, weight, expected) in [
        ([0.0, 2.0, 0.0], 1.0, [0.0, 1.0]),
        ([0.0, 2.0, 0.0], 2.0, [0.0, 4.0 / 3.0]),
        ([0.0, 1.5, -2.0], 1.0, [-2.0, 0.45]),
    ] {
        let mut builder = Builder::new();
        let rows: Vec<_> = heights
            .iter()
            .enumerate()
            .map(|(index, z)| {
                let x = f64::from(u32::try_from(index).expect("three poles")) / 2.0;
                let w = if index == 1 { weight } else { 1.0 };
                vec![([x, 0.0, *z], w), ([x, 1.0, *z], w)]
            })
            .collect();
        let surface = builder.nurbs(
            (2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
            (1, vec![0.0, 0.0, 1.0, 1.0]),
            &rows,
        );
        builder.face(&surface, Sense::Forward, &[], false);
        let ir = builder.finish(BodyKind::Sheet);
        let report = measure(
            &ir,
            &Selection {
                all_faces: true,
                ..Selection::default()
            },
        );
        let bbox = report.faces()[0]
            .measures()
            .bbox()
            .measured()
            .expect("surface box");
        assert_eq!(bbox.value().kind, BoxKind::Tight);
        assert_eq!(bbox.method(), Method::SurfaceSubdivision);
        let low = bbox.value().min.get();
        let high = bbox.value().max.get();
        assert_eq!([low.x, low.y, high.x, high.y], [0.0, 0.0, 1.0, 1.0]);
        assert!((low.z - expected[0]).abs() <= EXTENT_TOLERANCE);
        assert!((high.z - expected[1]).abs() <= EXTENT_TOLERANCE);
    }
}

#[test]
fn trim_membership_removes_the_carrier_peak_from_the_box() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::Point3;

    use crate::budget::{PassAllowance, WorkProfile};
    use crate::canonical::placement::Placement;
    use crate::canonical::surface::{Chart, NurbsChart};
    use crate::context::Eval;
    use crate::geom::Aabb;
    use crate::region::{LoopIntegral, LoopRegion, LoopShape, Region};
    use crate::report::{Classification, LoopRole, RegionRule};
    use crate::test_support::builders::service_context;

    let mut builder = Builder::new();
    let rows: Vec<_> = [0.0, 2.0, 0.0]
        .iter()
        .enumerate()
        .map(|(index, z)| {
            let x = f64::from(u32::try_from(index).expect("three poles")) / 2.0;
            vec![([x, 0.0, *z], 1.0), ([x, 1.0, *z], 1.0)]
        })
        .collect();
    let surface = builder.nurbs(
        (2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
        (1, vec![0.0, 0.0, 1.0, 1.0]),
        &rows,
    );
    builder.face(&surface, Sense::Forward, &[], false);
    let ir = builder.finish(BodyKind::Sheet);
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
        &ir.geometry.surfaces[0].geometry
    else {
        panic!("NURBS carrier");
    };
    let chart = Chart::Nurbs(NurbsChart::new(surface, Placement::identity()).unwrap());
    let region = Region {
        rule: RegionRule::Nesting,
        loops: vec![LoopRegion {
            role: LoopRole::Outer,
            factor: 1.0,
        }],
        base: 0.0,
        classification: Classification::Unspecified,
        apex: None,
        exterior: 0.0,
        closed: None,
    };
    let shapes = [Some(LoopShape {
        points: vec![[0.0, 0.0], [0.25, 0.0], [0.25, 1.0], [0.0, 1.0]],
        diagonal: 1.0,
        integral: LoopIntegral {
            value: 0.25,
            error: 0.0,
            base: 0.0,
        },
    })];
    let boundary = Aabb {
        min: [0.0, 0.0, 0.0],
        max: [0.25, 1.0, 0.75],
    };
    let ctx = service_context();
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
    let pass = PassAllowance::new(WorkProfile::Desktop.pass_allowance(1).unwrap());
    let slice = pass.face_slice();
    let eval = Eval {
        ctx: &ctx,
        index: &index,
        work: &slice,
    };
    let (bbox, numeric) = super::bound(
        &eval,
        &chart,
        &region,
        super::Trims {
            shapes: &shapes,
            resolved: None,
        },
        Some(boundary),
        EXTENT_TOLERANCE,
        false,
    )
    .expect("trimmed box");
    assert_eq!(bbox, boundary);
    assert!(numeric <= EXTENT_TOLERANCE);
    assert_eq!(bbox.max_point(), Point3::new(0.25, 1.0, 0.75));
}

#[test]
fn subdivision_preserves_identical_subnormal_homogeneous_controls() {
    let weight = f64::from_bits(1);
    let point = [weight, 0.0, 0.0, weight];
    for fraction in [0.5, 0.37] {
        let (left, right) = super::split_polygon(&[point, point], fraction).expect("finite blend");
        assert_eq!(left, [point, point]);
        assert_eq!(right, [point, point]);
    }
}
