// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::geometry::{Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition};
use crate::ids::{CurveId, ProceduralCurveId};

fn helix_fixture() -> (CadIr, CurveId) {
    let curve_id = CurveId("helix-evaluation-curve".into());
    let construction_id = ProceduralCurveId("helix-evaluation-construction".into());
    let definition = ProceduralCurveDefinition::Helix {
        angle_range: [0.25, 2.0],
        center: Point3::new(1.0, -2.0, 3.0),
        major: Vector3::new(2.0, 0.0, 0.0),
        minor: Vector3::new(0.0, 2.0, 0.0),
        pitch: Vector3::new(0.0, 0.0, 3.0),
        apex_factor: 0.4,
        axis: Vector3::new(0.0, 0.0, 1.0),
    };
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction_id.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction_id,
        curve: curve_id.clone(),
        definition,
        cache_fit_tolerance: None,
    });
    (ir, curve_id)
}

fn expected_helix(parameter: f64) -> (Point3, Vector3, Vector3) {
    let start = 0.25;
    let major = Vector3::new(2.0, 0.0, 0.0);
    let minor = Vector3::new(0.0, 2.0, 0.0);
    let pitch = Vector3::new(0.0, 0.0, 3.0);
    let inverse_revolution = 1.0 / std::f64::consts::TAU;
    let revolution_fraction = (parameter - start) * inverse_revolution;
    let radial_scale = 1.0 + 0.4 * revolution_fraction;
    let radial = Vector3::new(
        major.x * parameter.cos() + minor.x * parameter.sin(),
        major.y * parameter.cos() + minor.y * parameter.sin(),
        major.z * parameter.cos() + minor.z * parameter.sin(),
    );
    let radial_first = Vector3::new(
        -major.x * parameter.sin() + minor.x * parameter.cos(),
        -major.y * parameter.sin() + minor.y * parameter.cos(),
        -major.z * parameter.sin() + minor.z * parameter.cos(),
    );
    let point = Point3::new(
        1.0 + radial_scale * radial.x,
        -2.0 + radial_scale * radial.y,
        3.0 + radial_scale * radial.z + revolution_fraction * pitch.z,
    );
    let scale_first = 0.4 * inverse_revolution;
    let tangent = Vector3::new(
        radial_scale * radial_first.x + scale_first * radial.x + inverse_revolution * pitch.x,
        radial_scale * radial_first.y + scale_first * radial.y + inverse_revolution * pitch.y,
        radial_scale * radial_first.z + scale_first * radial.z + inverse_revolution * pitch.z,
    );
    let acceleration = Vector3::new(
        -radial_scale * radial.x + 2.0 * scale_first * radial_first.x,
        -radial_scale * radial.y + 2.0 * scale_first * radial_first.y,
        -radial_scale * radial.z + 2.0 * scale_first * radial_first.z,
    );
    (point, tangent, acceleration)
}

fn assert_point_close(actual: Point3, expected: Point3) {
    let error = Vector3::new(
        actual.x - expected.x,
        actual.y - expected.y,
        actual.z - expected.z,
    )
    .norm();
    assert!(error <= 1.0e-12, "{actual:?} != {expected:?}: {error}");
}

fn assert_vector_close(actual: Vector3, expected: Vector3) {
    let error = Vector3::new(
        actual.x - expected.x,
        actual.y - expected.y,
        actual.z - expected.z,
    )
    .norm();
    assert!(error <= 1.0e-12, "{actual:?} != {expected:?}: {error}");
}

#[test]
fn cacheless_helix_curve_evaluates_point_and_exact_differentials() {
    let (ir, curve_id) = helix_fixture();
    let index = crate::index::ModelIndex::new(&ir);

    for parameter in [0.25, 0.75, 1.7, 2.0] {
        let (expected_point, expected_tangent, expected_acceleration) = expected_helix(parameter);
        let actual_point =
            super::model_curve_point_by_id(&index, &curve_id, parameter).expect("helix point");
        let actual = super::model_curve_differential_by_id(&index, &curve_id, parameter)
            .expect("helix differential");
        assert_point_close(actual_point, expected_point);
        assert_point_close(actual.point, expected_point);
        assert_vector_close(actual.tangent, expected_tangent);
        assert_vector_close(actual.acceleration, expected_acceleration);
    }
}

#[test]
fn cacheless_helix_curve_rejects_parameters_outside_its_native_interval() {
    let (ir, curve_id) = helix_fixture();
    let index = crate::index::ModelIndex::new(&ir);

    assert!(super::model_curve_point_by_id(&index, &curve_id, 0.24).is_none());
    assert!(super::model_curve_differential_by_id(&index, &curve_id, 2.01).is_none());
}

#[test]
fn cacheless_helix_curve_inversion_is_seeded_and_forward_validated() {
    let (ir, curve_id) = helix_fixture();
    let index = crate::index::ModelIndex::new(&ir);
    let target_parameter = 1.7;
    let target =
        super::model_curve_point_by_id(&index, &curve_id, target_parameter).expect("helix target");

    let inverse = super::model_curve_parameter_near_point(&ir, &curve_id, target, 1.5)
        .expect("helix inverse");
    assert!((0.25..=2.0).contains(&inverse));
    let resolved = super::model_curve_point_by_id(&index, &curve_id, inverse)
        .expect("forward-validated helix inverse");
    let residual = Vector3::new(
        resolved.x - target.x,
        resolved.y - target.y,
        resolved.z - target.z,
    )
    .norm();
    assert!(residual <= ir.tolerances.linear, "residual={residual}");
    assert!(super::model_curve_parameter_near_point(&ir, &curve_id, target, 0.24).is_none());
}
