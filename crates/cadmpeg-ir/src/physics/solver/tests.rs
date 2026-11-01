// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{Dimension, SolverSettings, SolverTarget, SolverValue};
use crate::ids::{OccurrenceId, SolverSettingsId};
use crate::products::JointId;
use crate::scalar::FiniteReal;

fn settings() -> SolverSettings {
    SolverSettings {
        id: SolverSettingsId::mint("test:model:solver#0").unwrap(),
        solver: "physx".to_owned(),
        schema: "PhysxJointAPI".to_owned(),
        target: SolverTarget::Joint {
            joint: JointId::mint("test:model:joint#0").unwrap(),
        },
        values: BTreeMap::from([
            (
                "physxJoint:armature".to_owned(),
                SolverValue::Real {
                    value: FiniteReal::new(2.0).unwrap(),
                    dimension: Dimension::new(2, 1, 0),
                },
            ),
            (
                "physxJoint:jointFriction".to_owned(),
                SolverValue::Real {
                    value: FiniteReal::new(0.1).unwrap(),
                    dimension: Dimension::NONE,
                },
            ),
            (
                "physxJoint:maxJointVelocity".to_owned(),
                SolverValue::Unbounded { negative: false },
            ),
            (
                "physxMimicJoint:rotX:referenceJoint".to_owned(),
                SolverValue::Targets {
                    targets: vec![SolverTarget::Occurrence {
                        occurrence: OccurrenceId::mint("test:model:occurrence#0").unwrap(),
                    }],
                },
            ),
        ]),
        native_ref: None,
    }
}

#[test]
fn solver_settings_round_trip_through_json() {
    let record = settings();
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["target"]["kind"], "joint");
    assert_eq!(
        json["values"]["physxJoint:armature"]["dimension"],
        serde_json::json!({"length": 2, "mass": 1, "angle": 0})
    );
    assert!(json["values"]["physxJoint:jointFriction"]
        .get("dimension")
        .is_none());
    assert_eq!(
        serde_json::from_value::<SolverSettings>(json).unwrap(),
        record
    );
}

#[test]
fn a_dimension_converts_by_the_powers_of_its_units() {
    let torque_per_degree = Dimension::new(2, 1, -1);
    let factor = torque_per_degree.factor(1000.0, 2.0, std::f64::consts::PI / 180.0);
    let expected = 1000.0 * 1000.0 * 2.0 * 180.0 / std::f64::consts::PI;
    assert!((factor - expected).abs() <= expected * f64::EPSILON * 4.0);
    assert_eq!(Dimension::NONE.factor(1000.0, 2.0, 3.0), 1.0);
}
