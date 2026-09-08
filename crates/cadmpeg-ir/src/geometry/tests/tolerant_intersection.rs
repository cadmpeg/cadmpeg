use super::super::{
    PcurveGeometry, ProceduralCurveDefinition, TolerantIntersectionConstruction,
    TolerantIntersectionParameterization,
};
use crate::{ids::SurfaceId, math::Point3};
use serde_json::json;

fn supports() -> [SurfaceId; 2] {
    ["test:surface#1", "test:surface#2"].map(|id| SurfaceId::mint(id).unwrap())
}

#[test]
fn tolerant_intersection_rejects_equal_supports_and_invalid_numeric_bounds() {
    let endpoints = [Point3::new(0.0, 0.0, 0.0); 2];
    assert!(TolerantIntersectionConstruction::try_new(supports(), endpoints, 0.0).is_ok());
    let [first, _] = supports();
    assert!(
        TolerantIntersectionConstruction::try_new([first.clone(), first], endpoints, 0.0).is_err()
    );
    for tolerance in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(
            TolerantIntersectionConstruction::try_new(supports(), endpoints, tolerance).is_err()
        );
    }
    for coordinate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(TolerantIntersectionConstruction::try_new(
            supports(),
            [Point3::new(coordinate, 0.0, 0.0), endpoints[1]],
            0.0
        )
        .is_err());
    }
}

#[test]
fn tolerant_parameterization_requires_a_finite_strict_interval() {
    let pcurves =
        || std::array::from_fn(|_| PcurveGeometry::Line(super::super::LinePcurve::U_AXIS));
    assert!(TolerantIntersectionParameterization::try_new(pcurves(), [-1.0, 1.0]).is_ok());
    for range in [
        [0.0, 0.0],
        [1.0, 0.0],
        [f64::NAN, 1.0],
        [0.0, f64::INFINITY],
    ] {
        assert!(TolerantIntersectionParameterization::try_new(pcurves(), range).is_err());
    }
}

#[test]
fn tolerant_intersection_wire_retains_flat_fields_and_rejects_invalid_admission() {
    let wire = json!({
        "kind": "tolerant_intersection", "supports": supports(),
        "endpoints": [{"x": 0.0, "y": 0.0, "z": 0.0}, {"x": 0.0, "y": 0.0, "z": 0.0}],
        "tolerance": 0.0
    });
    let value: ProceduralCurveDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), wire);
    let mut invalid = wire.clone();
    invalid["supports"][1] = invalid["supports"][0].clone();
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(invalid).is_err());
    let mut invalid = wire;
    invalid["tolerance"] = json!(-1.0);
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(invalid).is_err());
    let parameterization = TolerantIntersectionParameterization::try_new(
        [
            PcurveGeometry::Line(super::super::LinePcurve::U_AXIS),
            PcurveGeometry::Line(super::super::LinePcurve::U_AXIS),
        ],
        [0.0, 1.0],
    )
    .unwrap();
    let mut invalid = serde_json::to_value(parameterization).unwrap();
    invalid["parameter_range"] = json!([1.0, 1.0]);
    assert!(serde_json::from_value::<TolerantIntersectionParameterization>(invalid).is_err());
}
