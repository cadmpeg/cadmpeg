// SPDX-License-Identifier: Apache-2.0

use super::{Collider, ColliderShape, MassProperties, PhysicsTarget, PrincipalInertia};
use crate::ids::{BodyId, ColliderId, MassPropertiesId, OccurrenceId};
use crate::scalar::{NonNegativeReal, PositiveReal};

#[test]
fn mass_properties_round_trip_through_json() {
    let mass = MassProperties {
        id: MassPropertiesId::mint("test:model:mass#0").unwrap(),
        target: PhysicsTarget::Occurrence {
            occurrence: OccurrenceId::mint("test:model:occurrence#0").unwrap(),
        },
        mass: PositiveReal::new(2.5),
        density: None,
        center_of_mass: None,
        inertia: Some(PrincipalInertia {
            moments: [1.0, 2.0, 3.0].map(|value| NonNegativeReal::new(value).unwrap()),
            axes: None,
        }),
        material: None,
        native_ref: None,
    };
    let json = serde_json::to_value(&mass).unwrap();
    assert_eq!(json["target"]["kind"], "occurrence");
    assert!(json.get("density").is_none());
    assert_eq!(
        serde_json::from_value::<MassProperties>(json).unwrap(),
        mass
    );
}

#[test]
fn a_native_collider_shape_keeps_its_name() {
    let collider = Collider {
        id: ColliderId::mint("test:model:collider#0").unwrap(),
        target: PhysicsTarget::Body {
            body: BodyId::mint("test:model:body#0").unwrap(),
        },
        shape: ColliderShape::Native {
            name: "sdf".to_owned(),
        },
        enabled: true,
        material: None,
        native_ref: None,
    };
    let json = serde_json::to_value(&collider).unwrap();
    assert_eq!(
        json["shape"],
        serde_json::json!({"kind": "native", "name": "sdf"})
    );
    assert_eq!(serde_json::from_value::<Collider>(json).unwrap(), collider);
}

#[test]
fn an_explicit_null_mass_is_refused() {
    let json = serde_json::json!({
        "id": "test:model:mass#0",
        "target": {"kind": "occurrence", "occurrence": "test:model:occurrence#0"},
        "mass": null,
    });
    let error = serde_json::from_value::<MassProperties>(json).unwrap_err();
    assert!(error.to_string().contains("mass"), "{error}");
}

#[test]
fn a_rigid_body_keeps_its_initial_velocities_and_rest_state() {
    use super::RigidBody;
    use crate::features::FiniteVector3;
    use crate::ids::RigidBodyId;
    use crate::math::Vector3;
    let body = RigidBody {
        id: RigidBodyId::mint("test:model:rigid-body#0").unwrap(),
        target: PhysicsTarget::Occurrence {
            occurrence: OccurrenceId::mint("test:model:occurrence#0").unwrap(),
        },
        kinematic: false,
        enabled: true,
        linear_velocity: FiniteVector3::new(Vector3::new(100.0, 0.0, 0.0)),
        angular_velocity: None,
        starts_asleep: true,
        native_ref: None,
    };
    let json = serde_json::to_value(&body).unwrap();
    assert!(json.get("angular_velocity").is_none());
    assert_eq!(json["starts_asleep"], true);
    assert_eq!(serde_json::from_value::<RigidBody>(json).unwrap(), body);
}
