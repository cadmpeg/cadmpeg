// SPDX-License-Identifier: Apache-2.0

use super::definition_wire;
use crate::math::Point3;
use cadmpeg_core::serde_erase::json;

#[test]
fn feature_lines_and_polylines_close_geometry_bounds_without_changing_wire_fields() {
    use crate::features::geometry::{FeatureLineSegment, FeaturePolyline};
    use crate::features::{FeatureDefinition, FeatureOperation};
    let first = Point3::new(0.0, 1.0, 2.0);
    let second = Point3::new(3.0, 4.0, 5.0);
    let segment = FeatureLineSegment::new(first, second).unwrap();
    let wire = definition_wire(
        "line_segment",
        cadmpeg_core::json!({"segment":{"start":first, "end":second}}),
    );
    let definition = FeatureDefinition::Operation(FeatureOperation::LineSegment { segment });
    assert_eq!(json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        json::from_value::<FeatureDefinition>(wire).unwrap(),
        definition
    );
    assert!(FeatureLineSegment::new(first, first).is_none());
    assert!(FeatureLineSegment::new(Point3::new(f64::NAN, 0.0, 0.0), second).is_none());
    assert!(json::from_value::<FeatureDefinition>(definition_wire(
        "line_segment",
        cadmpeg_core::json!({"segment":{"start":first, "end":first}})
    ))
    .is_err());

    for (points, closed) in [
        (vec![first, second], false),
        (vec![first, second, first], true),
        (vec![first, second, first], false),
    ] {
        let chain = FeaturePolyline::new(points.clone(), closed).unwrap();
        let admitted = points
            .iter()
            .copied()
            .map(crate::features::geometry::FinitePoint3::new)
            .collect::<Option<Vec<_>>>()
            .unwrap();
        assert_eq!(
            FeaturePolyline::from_parts(admitted, closed),
            Some(chain.clone())
        );
        let wire = definition_wire(
            "polyline",
            cadmpeg_core::json!({"chain":{"points":points, "closed":closed}}),
        );
        let definition = FeatureDefinition::Operation(FeatureOperation::Polyline { chain });
        assert_eq!(json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            json::from_value::<FeatureDefinition>(wire).unwrap(),
            definition
        );
    }
    for (points, closed) in [
        (vec![], false),
        (vec![first], false),
        (vec![first, second], true),
        (vec![first, first, second], false),
    ] {
        assert!(FeaturePolyline::new(points.clone(), closed).is_none());
        assert!(json::from_value::<FeatureDefinition>(definition_wire(
            "polyline",
            cadmpeg_core::json!({"chain":{"points":points, "closed":closed}})
        ))
        .is_err());
    }
    assert!(
        FeaturePolyline::new(vec![first, Point3::new(f64::INFINITY, 0.0, 0.0)], false).is_none()
    );
}

#[test]
fn equation_curve_admission_preserves_expression_text_and_requires_an_increasing_domain() {
    use crate::features::geometry::FeatureEquationCurve;
    use crate::features::{FeatureDefinition, FeatureOperation};
    let curve = FeatureEquationCurve::new(
        " t ".into(),
        " t*t ".into(),
        "0".into(),
        " -t ".into(),
        -2.0,
        3.0,
    )
    .unwrap();
    let definition = FeatureDefinition::Operation(FeatureOperation::EquationCurve { curve });
    let wire = definition_wire(
        "equation_curve",
        cadmpeg_core::json!({"curve":{"parameter":" t ", "x_expression":" t*t ", "y_expression":"0", "z_expression":" -t ", "start":-2.0, "end":3.0}}),
    );
    assert_eq!(json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
        definition
    );
    for field in ["parameter", "x_expression", "y_expression", "z_expression"] {
        for blank in ["", " \t\n"] {
            let mut invalid = wire.clone();
            invalid[field] = cadmpeg_core::json!(blank);
            assert!(json::from_value::<FeatureDefinition>(invalid).is_err());
        }
    }
    for [start, end] in [
        [1.0, 1.0],
        [2.0, 1.0],
        [f64::NAN, 2.0],
        [0.0, f64::INFINITY],
        [f64::NEG_INFINITY, 0.0],
    ] {
        assert!(FeatureEquationCurve::new(
            "t".into(),
            "t".into(),
            "0".into(),
            "0".into(),
            start,
            end
        )
        .is_none());
    }
    for [start, end] in [[1.0, 1.0], [2.0, 1.0]] {
        let mut invalid = wire.clone();
        invalid["value"]["value"]["curve"]["start"] = cadmpeg_core::json!(start);
        invalid["value"]["value"]["curve"]["end"] = cadmpeg_core::json!(end);
        assert!(json::from_value::<FeatureDefinition>(invalid).is_err());
    }
}
