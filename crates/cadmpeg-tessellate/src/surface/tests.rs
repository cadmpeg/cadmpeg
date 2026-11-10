// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::compare::floats_agree;
use cadmpeg_ir::math::portable::Portable;

use super::circular_cone_normal_curvature;

#[test]
fn a_circular_cone_generatrix_is_straight() {
    assert_eq!(circular_cone_normal_curvature(1, 1.0, 2.0, 0.5), Some(0.0));
}

#[test]
fn a_circular_cone_parallel_uses_the_section_radius() {
    let radius = 2.0_f64;
    let half_angle = 0.5_f64;
    let v = 1.5_f64;
    let rho = radius + v * half_angle.tan();
    let expected = half_angle.cos().abs() / rho.abs();
    assert!(floats_agree(
        circular_cone_normal_curvature(0, v, radius, half_angle).expect("finite"),
        expected
    ));
}

#[test]
fn a_circular_cone_apex_has_no_finite_parallel_curvature() {
    let radius = 2.0_f64;
    let half_angle = 0.5_f64;
    let apex = -radius / half_angle.portable_tan();
    assert_eq!(
        circular_cone_normal_curvature(0, apex, radius, half_angle),
        None
    );
}

#[test]
fn a_cone_apex_has_no_normal_even_when_its_parameter_rounds() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::geometry::analytic::ConeSurface;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::index::ModelIndex;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let surface = &mut ir.model.surfaces[0];
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            15.0,
            1.0,
            0.5_f64.atan(),
        )
        .unwrap(),
    ));
    let id = surface.id.clone();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(1000);
    let carrier = super::Carrier::new(&index, &work, &id).unwrap();
    assert!(carrier.normal([0.0, -30.0]).unwrap().is_none());
    assert!(carrier.normal([0.0, -29.0]).unwrap().is_some());
}

#[test]
fn bicubic_bezier_carrier_evaluates_tensor_grid_via_cadmpeg_gpu() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::index::ModelIndex;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut rows = Vec::with_capacity(4);
    for i in 0..4 {
        let mut row = Vec::with_capacity(4);
        for j in 0..4 {
            let u = f64::from(i) / 3.0;
            let v = f64::from(j) / 3.0;
            row.push(Point3::new(u * 10.0, v * 10.0, u * v));
        }
        rows.push(row);
    }
    let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let nurbs = NurbsSurface::new(
        NurbsSurfaceAxis::new(3, knots.clone(), false),
        NurbsSurfaceAxis::new(3, knots, false),
        NurbsPoleGrid::Polynomial { rows },
        false,
    )
    .unwrap();
    let surface = &mut ir.model.surfaces[0];
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs));
    let id = surface.id.clone();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(10_000);
    let carrier = super::Carrier::new(&index, &work, &id).unwrap();

    let params: Vec<f64> = (0..=8).map(|k| f64::from(k) / 8.0).collect();
    let grid = carrier
        .eval_bicubic_bezier_grid(&params, &params)
        .expect("bicubic Bezier carrier");
    assert_eq!(grid.len(), 81);
    for (i, &u) in params.iter().enumerate() {
        for (j, &v) in params.iter().enumerate() {
            let expected = carrier.point([u, v]).unwrap().unwrap();
            let actual = grid[i * params.len() + j];
            assert!((actual.x - expected.x).abs() < 1e-11);
            assert!((actual.y - expected.y).abs() < 1e-11);
            assert!((actual.z - expected.z).abs() < 1e-11);
        }
    }
}
