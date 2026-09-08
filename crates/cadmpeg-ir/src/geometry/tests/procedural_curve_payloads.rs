// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{CurveOffsetRange, OffsetSide, ProceduralCurve, ProceduralCurveDefinition};
use crate::ids::{CurveId, ProceduralCurveId};
use crate::math::Vector3;

fn id() -> ProceduralCurveId {
    ProceduralCurveId::mint("synthetic:test:procedural_curve#payload").unwrap()
}

fn source() -> CurveId {
    CurveId::mint("synthetic:test:curve#source").unwrap()
}

fn subset(range: [f64; 2]) -> ProceduralCurveDefinition {
    ProceduralCurveDefinition::Subset {
        source: source(),
        parameter_range: range,
        sense: false,
    }
}

#[test]
fn curve_payload_admission_and_edits_require_finite_ordered_subset_ranges() {
    let mut curve = ProceduralCurve::try_new(id(), subset([1.0, 1.0]), Some(0.5)).unwrap();
    let before = curve.clone();
    let wire = serde_json::to_value(&curve).unwrap();
    assert_eq!(
        wire["definition"]["parameter_range"],
        serde_json::json!([1.0, 1.0])
    );
    assert_eq!(
        serde_json::from_value::<ProceduralCurve>(wire).unwrap(),
        curve
    );
    for range in [[1.0, 0.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        let invalid = subset(range);
        assert!(ProceduralCurve::new(id(), invalid.clone()).is_err());
        assert!(ProceduralCurve::try_new(id(), invalid.clone(), None).is_err());
        assert!(curve.replace_definition(invalid.clone()).is_err());
        assert_eq!(curve, before);
        assert!(curve
            .try_replace_definition(invalid.clone(), Some(2.0))
            .is_err());
        assert_eq!(curve, before);
        assert!(curve
            .edit_definition(|definition| *definition = invalid)
            .is_err());
        assert_eq!(curve, before);
    }
    let invalid = serde_json::to_value(subset([1.0, 0.0])).unwrap();
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(invalid.clone()).is_err());
    let mut wire = serde_json::to_value(before).unwrap();
    wire["definition"] = invalid;
    assert!(serde_json::from_value::<ProceduralCurve>(wire).is_err());
}

#[test]
fn offset_payload_preserves_direction_magnitude_and_requires_strict_ranges() {
    let definition = |side, range| ProceduralCurveDefinition::Offset {
        source: source(),
        distance: -2.0,
        side,
        range,
    };
    let direction = OffsetSide::Direction {
        direction: Vector3::new(0.0, 0.0, 2.0),
        support: None,
    };
    assert!(ProceduralCurve::new(id(), definition(direction.clone(), None)).is_ok());
    for range in [[1.0, 0.0], [0.0, 0.0]] {
        let invalid = definition(
            direction.clone(),
            Some(CurveOffsetRange::Uniform {
                parameter_range: range,
            }),
        );
        assert!(ProceduralCurve::new(id(), invalid.clone()).is_err());
        assert!(serde_json::from_value::<ProceduralCurveDefinition>(
            serde_json::to_value(invalid).unwrap()
        )
        .is_err());
    }
    assert!(ProceduralCurve::new(
        id(),
        definition(OffsetSide::PlaneNormal(Vector3::new(0.0, 0.0, 2.0)), None)
    )
    .is_err());
    assert!(ProceduralCurve::new(
        id(),
        definition(OffsetSide::PlaneNormal(Vector3::new(0.0, 0.0, 1.0)), None)
    )
    .is_ok());
    assert!(ProceduralCurve::new(
        id(),
        definition(
            OffsetSide::Direction {
                direction: Vector3::new(0.0, 0.0, 0.0),
                support: None
            },
            None
        )
    )
    .is_err());
}
