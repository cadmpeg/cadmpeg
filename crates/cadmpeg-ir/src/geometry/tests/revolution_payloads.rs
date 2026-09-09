// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{ProceduralSurface, ProceduralSurfaceDefinition};
use crate::ids::{CurveId, ProceduralSurfaceId};
use crate::math::{Point3, Vector3};

fn id() -> ProceduralSurfaceId {
    ProceduralSurfaceId::mint("synthetic:test:procedural_surface#revolution").unwrap()
}

fn curve() -> CurveId {
    CurveId::mint("synthetic:test:curve#directrix").unwrap()
}

fn revolution(
    intervals: [[f64; 2]; 3],
) -> Result<ProceduralSurfaceDefinition, crate::geometry::ProceduralGeometryError> {
    Ok(ProceduralSurfaceDefinition::Revolution(
        crate::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
            curve(),
            (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
            intervals[0],
            Some(intervals[1]),
            Some(intervals[2]),
            false,
            None,
        )?,
    ))
}

#[test]
fn revolution_requires_three_strict_finite_intervals_on_all_routes() {
    for slot in 0..3 {
        for range in [
            [0.0, 0.0],
            [1.0, 0.0],
            [f64::NAN, 1.0],
            [0.0, f64::INFINITY],
        ] {
            let mut intervals = [[0.0, 1.0]; 3];
            intervals[slot] = range;
            assert!(revolution(intervals).is_err());
            let mut wire = serde_json::to_value(revolution([[0.0, 1.0]; 3]).unwrap()).unwrap();
            wire[[
                "angular_interval",
                "angular_parameter_interval",
                "parameter_interval",
            ][slot]] = serde_json::json!(range);
            assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(wire).is_err());
        }
    }
    let definition = ProceduralSurfaceDefinition::Revolution(
        crate::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
            curve(),
            (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
            [-1.0, 0.0],
            None,
            None,
            false,
            None,
        )
        .unwrap(),
    );
    let surface = ProceduralSurface::new(id(), definition, None).unwrap();
    let wire = serde_json::to_value(&surface).unwrap();
    assert_eq!(
        wire["definition"]["angular_interval"],
        serde_json::json!([-1.0, 0.0])
    );
    assert!(wire["definition"]
        .get("angular_parameter_interval")
        .is_none());
    assert!(wire["definition"].get("parameter_interval").is_none());
    assert_eq!(
        serde_json::from_value::<ProceduralSurface>(wire).unwrap(),
        surface
    );
}

/// Assert that replacing `field` of an admitted definition with `value` is
/// rejected by both the definition and the carrier deserialization routes.
fn reject_on_serde_routes<T: serde::Serialize>(
    valid: &ProceduralSurfaceDefinition,
    field: &str,
    value: T,
) {
    let surface = ProceduralSurface::new(id(), valid.clone(), None).unwrap();
    let mut definition_wire = serde_json::to_value(valid).unwrap();
    definition_wire[field] = serde_json::to_value(value).unwrap();
    assert!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(definition_wire.clone()).is_err()
    );
    let mut wire = serde_json::to_value(surface).unwrap();
    wire["definition"] = definition_wire;
    assert!(serde_json::from_value::<ProceduralSurface>(wire).is_err());
}

#[test]
fn axis_revolution_and_sum_reject_nonfinite_frames_and_basepoints() {
    let axis = |axis_origin, axis_direction| {
        crate::geometry::surface_payloads::AxisRevolutionSurfaceConstruction::try_new(
            curve(),
            axis_origin,
            axis_direction,
        )
        .map(ProceduralSurfaceDefinition::AxisRevolution)
    };
    let origin = Point3::new(0.0, 0.0, 0.0);
    let valid_axis = axis(origin, Vector3::new(0.0, 0.0, 1.0)).unwrap();
    for direction in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 2.0),
        Vector3::new(0.0, f64::INFINITY, 1.0),
    ] {
        assert!(axis(origin, direction).is_err());
        reject_on_serde_routes(&valid_axis, "axis_direction", direction);
    }
    let bad_origin = Point3::new(f64::NAN, 0.0, 0.0);
    assert!(axis(bad_origin, Vector3::new(0.0, 0.0, 1.0)).is_err());
    reject_on_serde_routes(&valid_axis, "axis_origin", bad_origin);
    for direction in [Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 0.0, -1.0)] {
        assert!(ProceduralSurface::new(id(), axis(origin, direction).unwrap(), None).is_ok());
    }
    let sum = |basepoint| {
        crate::geometry::surface_payloads::SumSurfaceConstruction::try_new(
            curve(),
            curve(),
            basepoint,
            None,
        )
        .map(ProceduralSurfaceDefinition::Sum)
    };
    let valid_sum = sum(Vector3::new(0.0, 0.0, 0.0)).unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let basepoint = Vector3::new(0.0, value, 0.0);
        assert!(sum(basepoint).is_err());
        reject_on_serde_routes(&valid_sum, "basepoint", basepoint);
    }
    assert!(ProceduralSurface::new(id(), valid_sum, None).is_ok());
}
