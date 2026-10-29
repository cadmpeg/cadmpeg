// SPDX-License-Identifier: Apache-2.0
use super::{literal_fields, wire_dimensions};
use crate::math::{Point3, Vector3};

#[test]
fn equation_curve_admission_preserves_expression_text_and_requires_an_increasing_domain() {
    use crate::features::{FeatureDefinition, FeatureEquationCurve, FeatureOperation};
    let curve = FeatureEquationCurve::new(
        " t ".into(),
        " t*t ".into(),
        "0".into(),
        " -t ".into(),
        crate::features::Dimension::Literal(crate::scalar::FiniteReal::new(-2.0).unwrap()),
        crate::features::Dimension::Literal(crate::scalar::FiniteReal::new(3.0).unwrap()),
    )
    .unwrap();
    let definition = FeatureDefinition::Operation(FeatureOperation::EquationCurve { curve });
    let mut wire = serde_json::json!({"definition":"equation_curve", "curve":{"parameter":" t ", "x_expression":" t*t ", "y_expression":"0", "z_expression":" -t ", "start":-2.0, "end":3.0}});
    wire["curve"] = literal_fields(wire["curve"].take());
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
        definition
    );
    for field in ["parameter", "x_expression", "y_expression", "z_expression"] {
        for blank in ["", " \t\n"] {
            let mut invalid = wire.clone();
            invalid["curve"][field] = serde_json::json!(blank);
            assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
        }
    }
    for [start, end] in [
        [1.0, 1.0],
        [2.0, 1.0],
        [f64::NAN, 2.0],
        [0.0, f64::INFINITY],
        [f64::NEG_INFINITY, 0.0],
    ] {
        let curve = crate::scalar::FiniteReal::new(start)
            .zip(crate::scalar::FiniteReal::new(end))
            .and_then(|(start, end)| {
                FeatureEquationCurve::new(
                    "t".into(),
                    "t".into(),
                    "0".into(),
                    "0".into(),
                    start.into(),
                    end.into(),
                )
            });
        assert!(curve.is_none());
    }
    for [start, end] in [[1.0, 1.0], [2.0, 1.0]] {
        let mut invalid = wire.clone();
        invalid["curve"]["start"]["value"] = serde_json::json!(start);
        invalid["curve"]["end"]["value"] = serde_json::json!(end);
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
    }
}

#[test]
fn feature_arcs_preserve_directed_spans_and_admit_only_valid_frames_and_radii() {
    use crate::geometry::DirectedParameterRange;
    use crate::{
        features::{FeatureCircularArc, FeatureDefinition, FeatureEllipticArc, FeatureOperation},
        scalar::PositiveLength,
    };
    let center = Point3::new(1.0, 2.0, 3.0);
    let normal = Vector3::new(0.0, 0.0, 2.0);
    let major_axis = Vector3::new(3.0, 0.0, 0.0);
    let major = PositiveLength::new(4.0).unwrap();
    let minor = PositiveLength::new(2.0).unwrap();
    for endpoints in [[0.0, std::f64::consts::TAU], [2.0, -1.0]] {
        let angles = DirectedParameterRange::new(endpoints).unwrap();
        let arc =
            FeatureCircularArc::new(center, normal, (major).into(), (angles).try_into().unwrap())
                .unwrap();
        let definition = FeatureDefinition::Operation(FeatureOperation::CircularArc { arc });
        let mut wire = serde_json::json!({"definition":"circular_arc", "arc":{"center":center, "normal":normal, "radius":4.0, "start_angle":endpoints[0], "end_angle":endpoints[1]}});
        wire["arc"] = literal_fields(wire["arc"].take());
        assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
            definition
        );
        let mut invalid = wire;
        invalid["arc"]["end_angle"] = invalid["arc"]["start_angle"].clone();
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());

        let arc = FeatureEllipticArc::new(
            center,
            normal,
            major_axis,
            ([major, minor]).map(Into::into),
            (angles).try_into().unwrap(),
        )
        .unwrap();
        let definition = FeatureDefinition::Operation(FeatureOperation::EllipticArc { arc });
        let mut wire = serde_json::json!({"definition":"elliptic_arc", "arc":{"center":center, "normal":normal, "major_axis":major_axis, "major_radius":4.0, "minor_radius":2.0, "start_angle":endpoints[0], "end_angle":endpoints[1]}});
        wire["arc"] = literal_fields(wire["arc"].take());
        assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
            definition
        );
        for (field, value) in [
            ("minor_radius", serde_json::json!(5.0)),
            ("major_axis", serde_json::json!({"x":0.0,"y":0.0,"z":1.0})),
            ("end_angle", serde_json::json!(endpoints[0])),
        ] {
            let mut invalid = wire.clone();
            if value.is_number() {
                *wire_dimensions::value_mut(&mut invalid["arc"], field) = value;
            } else {
                invalid["arc"][field] = value;
            }
            assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
        }
    }
    let angles = DirectedParameterRange::new([0.0, 1.0]).unwrap();
    assert!(FeatureCircularArc::new(
        center,
        Vector3::new(0.0, 0.0, 0.0),
        (major).into(),
        (angles).try_into().unwrap()
    )
    .is_none());
    assert!(FeatureEllipticArc::new(
        center,
        normal,
        major_axis,
        ([minor, major]).map(Into::into),
        (angles).try_into().unwrap()
    )
    .is_none());
    assert!(FeatureEllipticArc::new(
        center,
        normal,
        major_axis,
        ([major, major]).map(Into::into),
        (angles).try_into().unwrap()
    )
    .is_some());
    assert!(FeatureEllipticArc::new(
        center,
        normal,
        normal,
        ([major, minor]).map(Into::into),
        (angles).try_into().unwrap()
    )
    .is_none());
    assert!(FeatureEllipticArc::new(
        center,
        normal,
        Vector3::new(
            1.0,
            0.0,
            crate::features::EPS_FEATURE_ELLIPSE_AXES_ORTHO / 2.0
        ),
        ([major, minor]).map(Into::into),
        (angles).try_into().unwrap()
    )
    .is_some());
    assert!(FeatureEllipticArc::new(
        center,
        normal,
        Vector3::new(1.0, 0.0, crate::features::EPS_FEATURE_ELLIPSE_AXES_ORTHO),
        ([major, minor]).map(Into::into),
        (angles).try_into().unwrap()
    )
    .is_none());
}

#[test]
fn feature_arcs_rebuild_from_checked_parts() {
    use crate::features::{
        FeatureCircularArc, FeatureDirection3, FeatureEllipticArc, FinitePoint3,
    };
    use crate::geometry::DirectedParameterRange;
    use crate::scalar::{PositiveLength, PositiveReal};

    let center = Point3::new(1.0, 2.0, 3.0);
    let normal = Vector3::new(0.0, 0.0, 2.0);
    let major_axis = Vector3::new(3.0, 0.0, 0.0);
    let angles = DirectedParameterRange::new([2.0, -1.0]).unwrap();
    let radius = PositiveLength::new(4.0).unwrap();

    let arc = FeatureCircularArc::new(
        center,
        normal,
        (radius).into(),
        (angles).try_into().unwrap(),
    )
    .unwrap();
    assert_eq!(arc.center(), FinitePoint3::new(center).unwrap());
    assert_eq!(arc.normal(), FeatureDirection3::new(normal).unwrap());
    assert_eq!(
        FeatureCircularArc::from_parts(
            arc.center(),
            arc.normal(),
            arc.radius().clone(),
            arc.angles().clone()
        ),
        arc
    );
    let moved = FinitePoint3::new(Point3::new(-5.0, 0.0, 7.5)).unwrap();
    let wider = PositiveLength::new(8.0).unwrap();
    assert_eq!(
        FeatureCircularArc::from_parts(moved, arc.normal(), (wider).into(), arc.angles().clone()),
        FeatureCircularArc::new(
            moved.get(),
            normal,
            (wider).into(),
            (angles).try_into().unwrap()
        )
        .unwrap()
    );

    let [major, minor] = [4.0, 2.0].map(|value| PositiveLength::new(value).unwrap());
    let ellipse = FeatureEllipticArc::new(
        center,
        normal,
        major_axis,
        ([major, minor]).map(Into::into),
        (angles).try_into().unwrap(),
    )
    .unwrap();
    let scale = PositiveReal::new(2.0).unwrap();
    let scaled = [8.0, 4.0].map(|value| PositiveLength::new(value).unwrap());
    assert_eq!(
        ellipse
            .with_scaled_radii(scale)
            .map(|arc| arc.with_center(moved)),
        FeatureEllipticArc::new(
            moved.get(),
            normal,
            major_axis,
            (scaled).map(Into::into),
            (angles).try_into().unwrap()
        )
    );
    let equal = FeatureEllipticArc::new(
        center,
        normal,
        major_axis,
        ([major, major]).map(Into::into),
        (angles).try_into().unwrap(),
    )
    .unwrap();
    assert_eq!(
        Some(equal.with_center(moved)),
        FeatureEllipticArc::new(
            moved.get(),
            normal,
            major_axis,
            ([major, major]).map(Into::into),
            (angles).try_into().unwrap()
        )
    );
    assert!(FeatureEllipticArc::new(
        moved.get(),
        normal,
        major_axis,
        ([minor, major]).map(Into::into),
        (angles).try_into().unwrap()
    )
    .is_none());
}
