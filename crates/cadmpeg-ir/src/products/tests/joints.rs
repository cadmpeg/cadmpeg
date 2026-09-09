// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::super::*;

#[test]
fn joint_wire_rejects_fields_outside_its_kinematic_family() {
    let cases = [
        (JointKind::Grounded, vec![]),
        (
            JointKind::Fixed,
            vec![
                "angle",
                "translation_offset",
                "angular_limits",
                "linear_limits",
            ],
        ),
        (JointKind::Revolute, vec!["angle", "angular_limits"]),
        (
            JointKind::Slider,
            vec!["distance", "translation_offset", "linear_limits"],
        ),
        (
            JointKind::Cylindrical,
            vec!["angle", "distance", "angular_limits", "linear_limits"],
        ),
        (JointKind::Ball, vec![]),
        (JointKind::Distance, vec!["distance"]),
        (JointKind::Parallel, vec![]),
        (JointKind::Perpendicular, vec![]),
        (JointKind::Angle, vec!["angle"]),
        (JointKind::RackPinion, vec!["distance", "distance2"]),
        (JointKind::Screw, vec!["distance"]),
        (JointKind::Gears, vec!["distance", "distance2"]),
        (JointKind::Belt, vec!["distance", "distance2"]),
        (
            JointKind::Native("application_joint".into()),
            vec![
                "angle",
                "translation_offset",
                "distance",
                "distance2",
                "angular_limits",
                "linear_limits",
            ],
        ),
    ];
    let fields = [
        ("angle", serde_json::json!(0.5)),
        ("translation_offset", serde_json::json!([1.0, 2.0, 3.0])),
        ("distance", serde_json::json!(2.0)),
        ("distance2", serde_json::json!(3.0)),
        (
            "angular_limits",
            serde_json::json!({"minimum": 0.0, "maximum": 1.0}),
        ),
        (
            "linear_limits",
            serde_json::json!({"minimum": 0.0, "maximum": 2.0}),
        ),
    ];
    for (kind, allowed) in cases {
        let connector = || JointConnector {
            operand: JointOperand::root("root:object", Vec::new()),
            frame: Transform::identity(),
            detached: false,
        };
        let id = JointId::mint("test:model:joint#0").expect("identity grammar");
        let joint = if kind == JointKind::Grounded {
            AssemblyJoint::grounded(id, connector(), None)
        } else {
            AssemblyJoint::paired(
                id,
                kind.clone().try_into().unwrap(),
                [connector(), connector()],
                None,
            )
        };
        let wire = serde_json::to_value(joint).unwrap();
        for (field, value) in &fields {
            let mut candidate = wire.clone();
            candidate[*field] = value.clone();
            let result = serde_json::from_value::<AssemblyJoint>(candidate.clone());
            assert_eq!(result.is_ok(), allowed.contains(field), "{kind:?}: {field}");
            if let Ok(joint) = result {
                assert_eq!(
                    serde_json::to_value(joint).unwrap(),
                    candidate,
                    "{kind:?}: {field}"
                );
            }
        }
    }
}

#[test]
fn joint_scalar_admission_rejects_each_nonfinite_field() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for field in ["angle", "distance", "distance2", "translation_offset"] {
            let mut scalars = JointScalars {
                angle: None,
                translation_offset: None,
                distance: None,
                distance2: None,
                angular_limits: None,
                linear_limits: None,
            };
            match field {
                "angle" => scalars.angle = Some(value),
                "distance" => scalars.distance = Some(value),
                "distance2" => scalars.distance2 = Some(value),
                _ => scalars.translation_offset = Some([0.0, value, 0.0]),
            }
            let error = PairedJointKind::from_wire(JointKind::Native("custom".into()), scalars)
                .unwrap_err();
            assert!(error.contains(field), "{field}: {error}");
        }
    }
    for value in [-1.0, 0.0, 1.0] {
        let kind = PairedJointKind::from_wire(
            JointKind::Native("custom".into()),
            JointScalars {
                angle: Some(value),
                translation_offset: Some([value; 3]),
                distance: Some(value),
                distance2: Some(value),
                angular_limits: None,
                linear_limits: None,
            },
        )
        .unwrap();
        let scalars = kind.scalars();
        assert_eq!(scalars.angle, Some(value));
        assert_eq!(scalars.translation_offset, Some([value; 3]));
        assert_eq!(scalars.distance, Some(value));
        assert_eq!(scalars.distance2, Some(value));
    }
}
