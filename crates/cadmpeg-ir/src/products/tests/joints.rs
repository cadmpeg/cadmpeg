// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::super::*;

fn connector() -> JointConnector {
    JointConnector {
        operand: JointOperand::root("root:object", Vec::new()),
        frame: Transform::identity(),
        detached: false,
    }
}

fn joint_id() -> JointId {
    JointId::mint("test:model:joint#0").expect("identity grammar")
}

/// Each kinematic family names exactly the scalars it carries, so a scalar
/// outside the family is refused by name rather than admitted and dropped.
#[test]
fn a_joint_family_refuses_a_scalar_outside_it_by_name() {
    let families: [(PairedJointKind, &[&str]); 14] = [
        (
            PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            &[
                "angle",
                "translation_offset",
                "angular_limits",
                "linear_limits",
            ],
        ),
        (
            PairedJointKind::Revolute {
                angle: None,
                angular_limits: None,
            },
            &["angle", "angular_limits"],
        ),
        (
            PairedJointKind::Slider {
                distance: None,
                translation_offset: None,
                linear_limits: None,
            },
            &["distance", "translation_offset", "linear_limits"],
        ),
        (
            PairedJointKind::Cylindrical {
                angle: None,
                distance: None,
                angular_limits: None,
                linear_limits: None,
            },
            &["angle", "distance", "angular_limits", "linear_limits"],
        ),
        (PairedJointKind::Ball {}, &[]),
        (PairedJointKind::Distance { distance: None }, &["distance"]),
        (PairedJointKind::Parallel {}, &[]),
        (PairedJointKind::Perpendicular {}, &[]),
        (PairedJointKind::Angle { angle: None }, &["angle"]),
        (
            PairedJointKind::RackPinion {
                distance: None,
                distance2: None,
            },
            &["distance", "distance2"],
        ),
        (PairedJointKind::Screw { distance: None }, &["distance"]),
        (
            PairedJointKind::Gears {
                distance: None,
                distance2: None,
            },
            &["distance", "distance2"],
        ),
        (
            PairedJointKind::Belt {
                distance: None,
                distance2: None,
            },
            &["distance", "distance2"],
        ),
        (
            PairedJointKind::Native {
                name: "application_joint".into(),
                angle: None,
                translation_offset: None,
                distance: None,
                distance2: None,
                angular_limits: None,
                linear_limits: None,
            },
            &[
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
            serde_json::json!({"bounds": "range", "minimum": 0.0, "maximum": 1.0}),
        ),
        (
            "linear_limits",
            serde_json::json!({"bounds": "range", "minimum": 0.0, "maximum": 2.0}),
        ),
    ];
    for (kind, allowed) in families {
        let joint =
            AssemblyJoint::paired(joint_id(), kind.clone(), [connector(), connector()], None);
        let wire = serde_json::to_value(&joint).unwrap();
        assert_eq!(
            serde_json::from_value::<AssemblyJoint>(wire.clone()).unwrap(),
            joint
        );
        for (field, value) in &fields {
            let mut candidate = wire.clone();
            candidate["operands"]["kind"][*field] = value.clone();
            let result = serde_json::from_value::<AssemblyJoint>(candidate.clone());
            assert_eq!(result.is_ok(), allowed.contains(field), "{kind:?}: {field}");
            match result {
                Ok(joint) => assert_eq!(
                    serde_json::to_value(joint).unwrap(),
                    candidate,
                    "{kind:?}: {field}"
                ),
                Err(error) => assert!(
                    error
                        .to_string()
                        .contains(&format!("unknown field `{field}`")),
                    "{kind:?}: {field}: {error}"
                ),
            }
        }
    }
}

/// The deleted parallel arrays and the read-and-dropped `properties` key are
/// refused at the level they were deleted from.
#[test]
fn the_joint_wire_refuses_the_deleted_keys() {
    let joint = AssemblyJoint::grounded(joint_id(), connector(), None);
    let wire = serde_json::to_value(&joint).unwrap();
    assert_eq!(wire["operands"]["arity"], "grounded");
    assert!(wire.get("kind").is_none());
    assert!(wire.get("frames").is_none());
    assert!(wire.get("detached").is_none());
    assert_eq!(
        serde_json::from_value::<AssemblyJoint>(wire.clone()).unwrap(),
        joint
    );

    for (level, key, value) in [
        ("properties", serde_json::json!({})),
        ("frames", serde_json::json!([])),
        ("detached", serde_json::json!([false, false])),
    ]
    .map(|(key, value)| ("joint", key, value))
    {
        let mut candidate = wire.clone();
        candidate.as_object_mut().unwrap().insert(key.into(), value);
        let error = serde_json::from_value::<AssemblyJoint>(candidate)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("unknown field `{key}`")),
            "{level}: {key}: {error}"
        );
    }

    let mut candidate = wire;
    candidate["operands"]["distance"] = serde_json::json!(1);
    let error = serde_json::from_value::<AssemblyJoint>(candidate)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field `distance`"), "{error}");
}

/// A non-finite joint scalar is refused where the scalar is read.
#[test]
fn a_nonfinite_joint_scalar_is_refused() {
    let joint = AssemblyJoint::paired(
        joint_id(),
        PairedJointKind::Native {
            name: "custom".into(),
            angle: None,
            translation_offset: None,
            distance: None,
            distance2: None,
            angular_limits: None,
            linear_limits: None,
        },
        [connector(), connector()],
        None,
    );
    let wire = serde_json::to_string(&joint).unwrap();
    for field in ["angle", "distance", "distance2"] {
        let candidate = wire.replace(
            "\"name\":\"custom\"",
            &format!("\"name\":\"custom\",\"{field}\":1e400"),
        );
        assert!(
            serde_json::from_str::<AssemblyJoint>(&candidate).is_err(),
            "{field}"
        );
    }
}
