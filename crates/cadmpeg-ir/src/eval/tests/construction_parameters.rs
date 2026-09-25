// SPDX-License-Identifier: Apache-2.0
use crate::geometry::surface_payloads::ExtrusionSurfaceConstruction;
use crate::geometry::{
    nurbs::NurbsCurve, CacheContract, Curve, CurveGeometry, SolvedCurveGeometry,
};
use crate::ids::CurveId;
use crate::math::{Point3, Vector3};
use crate::CadIr;

fn line_in_nurbs_carrier() -> (CadIr, CurveId) {
    let curve_id = CurveId::mint("test:model:curve#construction-parameter").unwrap();
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1e200, 1e200],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
        source_object: None,
    });
    (ir, curve_id)
}

#[test]
fn construction_mapping_refuses_nonfinite_widths_and_derivatives() {
    let (ir, id) = line_in_nurbs_carrier();
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        super::super::construction_curve_parameter(
            &index,
            &id,
            5e-301,
            crate::scalar::FiniteReal::array([0.0, 1e308]),
            crate::scalar::FiniteReal::array([0.0, 1e-300]),
            false,
        ),
        Err(crate::eval::EvaluationFailure::NonFinite(()))
    );
    assert_eq!(
        super::super::construction_curve_parameter(
            &index,
            &id,
            0.0,
            crate::scalar::FiniteReal::array([-1e308, 1e308]),
            None,
            false,
        ),
        Err(crate::eval::EvaluationFailure::NonFinite(()))
    );
}

#[test]
fn extrusion_partials_preserve_zero_acceleration_at_large_parameter_scale() {
    let (ir, id) = line_in_nurbs_carrier();
    let index = crate::index::ModelIndex::new(&ir);
    let direction = Vector3::new(0.0, 0.0, 1.0);
    let extrusion = |direction| {
        ExtrusionSurfaceConstruction::try_new(
            id.clone(),
            Some([0.0, 1e200]),
            direction,
            None,
            CacheContract::from_form(None),
        )
    };
    let partials = super::super::model_native_extrusion_partials(
        &index,
        &extrusion(direction).unwrap(),
        crate::scalar::FiniteReal::array([0.0, 1.0]),
        0.5,
        0.0,
        None,
    )
    .unwrap();
    assert!((partials.point.x - 0.5).abs() <= 8.0 * f64::EPSILON);
    assert!((partials.du.x - 1.0).abs() <= 8.0 * f64::EPSILON);
    assert_eq!(partials.duu, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(partials.dv, direction);
    assert!(extrusion(Vector3::new(f64::NAN, 0.0, 1.0)).is_err());
}
