// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use std::collections::BTreeMap;

use super::{SolverSettings, SolverTarget, SolverValue};
use crate::design::units::Quantity;
use crate::features::FeatureId;
use crate::ids::{OccurrenceId, SolverSettingsId};
use crate::scalar::FiniteReal;

fn settings() -> SolverSettings {
    SolverSettings {
        id: SolverSettingsId::mint("test:model:solver#0").unwrap(),
        solver: "physx".to_owned(),
        schema: "PhysxJointAPI".to_owned(),
        target: SolverTarget::Joint {
            joint: FeatureId::mint("test:model:feature#joint").unwrap(),
        }
        .into(),
        values: BTreeMap::from([
            (
                "physxJoint:armature".to_owned(),
                SolverValue::Real {
                    value: FiniteReal::new(2.0).unwrap(),
                    dimension: Quantity::of(2, 1, 0, 0),
                },
            ),
            (
                "physxJoint:jointFriction".to_owned(),
                SolverValue::Real {
                    value: FiniteReal::new(0.1).unwrap(),
                    dimension: Quantity::REAL,
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
                    }
                    .into()],
                },
            ),
        ]),
        native_ref: None,
    }
}

#[test]
fn solver_settings_round_trip_through_json() {
    let record = settings();
    let json = json::to_value(&record).unwrap();
    assert_eq!(json["target"]["entity"]["kind"], "joint");
    assert_eq!(
        json["values"]["physxJoint:armature"]["dimension"],
        serde_json::json!({"length": 2, "mass": 1, "angle": 0, "time": 0})
    );
    assert_eq!(
        json["values"]["physxJoint:jointFriction"]["dimension"],
        serde_json::json!({"length": 0, "mass": 0, "angle": 0, "time": 0})
    );
    assert_eq!(json::from_value::<SolverSettings>(json).unwrap(), record);
}

#[test]
fn a_quantity_converts_by_the_powers_of_its_units() {
    let torque_per_degree = Quantity::of(2, 1, -1, -2);
    let factor = torque_per_degree.factor(1000.0, 2.0, std::f64::consts::PI / 180.0, 60.0);
    let expected = 1000.0 * 1000.0 * 2.0 * 180.0 / std::f64::consts::PI / 3600.0;
    assert!((factor - expected).abs() <= expected * f64::EPSILON * 4.0);
    assert_eq!(Quantity::REAL.factor(1000.0, 2.0, 3.0, 4.0), 1.0);
}
