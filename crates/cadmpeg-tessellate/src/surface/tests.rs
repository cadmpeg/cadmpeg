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
