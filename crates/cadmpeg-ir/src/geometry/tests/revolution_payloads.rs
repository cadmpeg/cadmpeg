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

fn reject_on_all_routes(invalid: ProceduralSurfaceDefinition) {
    assert!(ProceduralSurface::new(id(), invalid.clone(), None).is_err());
    assert!(ProceduralSurface::try_new(id(), invalid.clone(), Some(0.5), None).is_err());
    let mut surface =
        ProceduralSurface::try_new(id(), revolution([[0.0, 1.0]; 3]).unwrap(), Some(0.5), None)
            .unwrap();
    let before = surface.clone();
    assert!(surface.replace_definition(invalid.clone()).is_err());
    assert_eq!(surface, before);
    assert!(surface
        .try_replace_definition(invalid.clone(), Some(2.0))
        .is_err());
    assert_eq!(surface, before);
    assert!(surface
        .edit_definition(|definition| *definition = invalid.clone())
        .is_err());
    assert_eq!(surface, before);
    let definition_wire = serde_json::to_value(invalid).unwrap();
    assert!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(definition_wire.clone()).is_err()
    );
    let mut wire = serde_json::to_value(before).unwrap();
    wire["definition"] = definition_wire;
    assert!(serde_json::from_value::<ProceduralSurface>(wire).is_err());
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

#[test]
fn axis_revolution_and_sum_reject_nonfinite_frames_and_basepoints() {
    let axis = |axis_origin, axis_direction| ProceduralSurfaceDefinition::AxisRevolution {
        directrix: curve(),
        axis_origin,
        axis_direction,
    };
    let origin = Point3::new(0.0, 0.0, 0.0);
    for direction in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 2.0),
        Vector3::new(0.0, f64::INFINITY, 1.0),
    ] {
        reject_on_all_routes(axis(origin, direction));
    }
    reject_on_all_routes(axis(
        Point3::new(f64::NAN, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ));
    for direction in [Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 0.0, -1.0)] {
        assert!(ProceduralSurface::new(id(), axis(origin, direction), None).is_ok());
    }
    let sum = |basepoint| ProceduralSurfaceDefinition::Sum {
        first: curve(),
        second: curve(),
        basepoint,
        revision_form: None,
    };
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        reject_on_all_routes(sum(Vector3::new(0.0, value, 0.0)));
    }
    assert!(ProceduralSurface::new(id(), sum(Vector3::new(0.0, 0.0, 0.0)), None).is_ok());
}
