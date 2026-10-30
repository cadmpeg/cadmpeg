// SPDX-License-Identifier: Apache-2.0
//! The `UsdPhysics` schemas (spec §16): joints, rigid bodies, mass,
//! collision, articulation roots, and physics materials, in scene units.

use crate::schema::{boolean, token, vector, Issues};
use crate::stage::Prim;

/// The joint prim types (spec §16.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JointType {
    Fixed,
    Revolute,
    Prismatic,
    Spherical,
    Distance,
    /// `PhysicsJoint`, constrained by its limits.
    Generic,
    /// `PhysxPhysicsGearJoint`: couples two revolute joints.
    Gear,
    /// `PhysxPhysicsRackAndPinionJoint`: couples a revolute joint to a
    /// prismatic joint.
    RackPinion,
}

impl JointType {
    pub(crate) fn of(type_name: &str) -> Option<Self> {
        Some(match type_name {
            "PhysicsFixedJoint" => Self::Fixed,
            "PhysicsRevoluteJoint" => Self::Revolute,
            "PhysicsPrismaticJoint" => Self::Prismatic,
            "PhysicsSphericalJoint" => Self::Spherical,
            "PhysicsDistanceJoint" => Self::Distance,
            "PhysicsJoint" => Self::Generic,
            "PhysxPhysicsGearJoint" => Self::Gear,
            "PhysxPhysicsRackAndPinionJoint" => Self::RackPinion,
            _ => return None,
        })
    }
}

/// One drive of a joint (spec §16.5), in scene units and degrees.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Drive {
    /// The instance name: `angular`, `linear`, `transX` to `rotZ`.
    pub(crate) instance: String,
    pub(crate) acceleration: bool,
    pub(crate) target_position: Option<f64>,
    pub(crate) target_velocity: Option<f64>,
    pub(crate) stiffness: Option<f64>,
    pub(crate) damping: Option<f64>,
    pub(crate) max_force: Option<f64>,
}

/// A joint prim's data in scene units and degrees.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointData {
    pub(crate) kind: JointType,
    pub(crate) bodies: [Option<String>; 2],
    pub(crate) positions: [[f64; 3]; 2],
    /// `(real, i, j, k)` rotations of the two joint frames.
    pub(crate) rotations: [[f64; 4]; 2],
    /// `X`, `Y`, or `Z`.
    pub(crate) axis: char,
    pub(crate) lower: Option<f64>,
    pub(crate) upper: Option<f64>,
    pub(crate) min_distance: Option<f64>,
    pub(crate) max_distance: Option<f64>,
    pub(crate) enabled: bool,
    pub(crate) drives: Vec<Drive>,
    /// The coupling ratio of a gear or rack-and-pinion joint: a pure ratio
    /// for a gear, degrees per length unit for a rack and pinion.
    pub(crate) ratio: Option<f64>,
    /// The joints that a gear or rack-and-pinion joint couples, in order.
    pub(crate) coupled: Vec<Option<String>>,
    /// Authored properties the model does not carry.
    pub(crate) dropped: Vec<String>,
}

/// Joint properties the model carries.
const JOINT_PROPERTIES: &[&str] = &[
    "physics:body0",
    "physics:body1",
    "physics:localPos0",
    "physics:localPos1",
    "physics:localRot0",
    "physics:localRot1",
    "physics:axis",
    "physics:lowerLimit",
    "physics:upperLimit",
    "physics:minDistance",
    "physics:maxDistance",
    "physics:jointEnabled",
];

/// The properties of a coupling joint that the model carries.
const COUPLING_PROPERTIES: &[&str] = &[
    "physics:gearRatio",
    "physics:ratio",
    "physics:hinge0",
    "physics:hinge1",
    "physics:hinge",
    "physics:prismatic",
    "physics:excludeFromArticulation",
];

/// A finite scalar, or `None` for an absent or infinite value.
fn finite(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<f64> {
    vector(prim, name, 1, issues)
        .and_then(|value| value.first().copied())
        .filter(|value| value.is_finite())
}

fn tuple<const N: usize>(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<[f64; N]> {
    let values = vector(prim, name, N, issues)?;
    let mut out = [0.0; N];
    for (slot, value) in out.iter_mut().zip(values) {
        *slot = value;
    }
    Some(out)
}

fn single_target(prim: &Prim<'_>, name: &str) -> Option<String> {
    match prim.relationship_targets(name).as_slice() {
        [target] => Some(target.clone()),
        _ => None,
    }
}

/// Reads a joint prim.
pub(crate) fn joint(prim: &Prim<'_>, kind: JointType, issues: &mut Issues) -> JointData {
    let mut drives = Vec::new();
    for schema in prim.api_schemas() {
        let Some(instance) = schema.strip_prefix("PhysicsDriveAPI:") else {
            continue;
        };
        let name = |field: &str| format!("drive:{instance}:physics:{field}");
        drives.push(Drive {
            instance: instance.to_owned(),
            acceleration: token(prim, &name("type"), issues).as_deref() == Some("acceleration"),
            target_position: finite(prim, &name("targetPosition"), issues),
            target_velocity: finite(prim, &name("targetVelocity"), issues),
            stiffness: finite(prim, &name("stiffness"), issues),
            damping: finite(prim, &name("damping"), issues),
            max_force: finite(prim, &name("maxForce"), issues),
        });
    }
    let instances: Vec<String> = drives.iter().map(|drive| drive.instance.clone()).collect();
    let coupling = matches!(kind, JointType::Gear | JointType::RackPinion);
    let schemas = prim.api_schemas();
    let carried = |name: &str| {
        schemas.iter().any(|schema| {
            let (base, instance) = crate::schema::physx::split(schema);
            crate::schema::physx::find(base, instance, name).is_some()
        })
    };
    let dropped = prim
        .property_names()
        .into_iter()
        .filter(|name| {
            let drive = name
                .strip_prefix("drive:")
                .and_then(|rest| rest.split_once(':'))
                .is_some_and(|(instance, _)| instances.iter().any(|known| known == instance));
            !JOINT_PROPERTIES.contains(name)
                && (!coupling || !COUPLING_PROPERTIES.contains(name))
                && !drive
                && name.contains("physics:")
                && !carried(name)
        })
        .map(str::to_owned)
        .collect();
    JointData {
        kind,
        bodies: [
            single_target(prim, "physics:body0"),
            single_target(prim, "physics:body1"),
        ],
        positions: [
            tuple(prim, "physics:localPos0", issues).unwrap_or_default(),
            tuple(prim, "physics:localPos1", issues).unwrap_or_default(),
        ],
        rotations: [
            tuple(prim, "physics:localRot0", issues).unwrap_or([1.0, 0.0, 0.0, 0.0]),
            tuple(prim, "physics:localRot1", issues).unwrap_or([1.0, 0.0, 0.0, 0.0]),
        ],
        axis: token(prim, "physics:axis", issues)
            .and_then(|axis| axis.chars().next())
            .filter(|axis| matches!(axis, 'X' | 'Y' | 'Z'))
            .unwrap_or('X'),
        lower: finite(prim, "physics:lowerLimit", issues),
        upper: finite(prim, "physics:upperLimit", issues),
        min_distance: finite(prim, "physics:minDistance", issues).filter(|value| *value >= 0.0),
        max_distance: finite(prim, "physics:maxDistance", issues).filter(|value| *value >= 0.0),
        enabled: boolean(prim, "physics:jointEnabled", issues).unwrap_or(true),
        drives,
        ratio: match kind {
            JointType::Gear => finite(prim, "physics:gearRatio", issues),
            JointType::RackPinion => finite(prim, "physics:ratio", issues),
            _ => None,
        },
        coupled: match kind {
            JointType::Gear => vec![
                single_target(prim, "physics:hinge0"),
                single_target(prim, "physics:hinge1"),
            ],
            JointType::RackPinion => vec![
                single_target(prim, "physics:hinge"),
                single_target(prim, "physics:prismatic"),
            ],
            _ => Vec::new(),
        },
        dropped,
    }
}

/// The `PhysicsRigidBodyAPI` state of a prim (spec §16.2), in scene units
/// and degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RigidBodyData {
    pub(crate) kinematic: bool,
    pub(crate) enabled: bool,
    pub(crate) velocity: Option<[f64; 3]>,
    pub(crate) angular_velocity: Option<[f64; 3]>,
    pub(crate) starts_asleep: bool,
}

pub(crate) fn rigid_body(prim: &Prim<'_>, issues: &mut Issues) -> RigidBodyData {
    let finite_tuple = |values: Option<[f64; 3]>| {
        values.filter(|values| values.iter().all(|value| value.is_finite()))
    };
    RigidBodyData {
        kinematic: boolean(prim, "physics:kinematicEnabled", issues).unwrap_or(false),
        enabled: boolean(prim, "physics:rigidBodyEnabled", issues).unwrap_or(true),
        velocity: finite_tuple(tuple(prim, "physics:velocity", issues)),
        angular_velocity: finite_tuple(tuple(prim, "physics:angularVelocity", issues)),
        starts_asleep: boolean(prim, "physics:startsAsleep", issues).unwrap_or(false),
    }
}

/// The `physics:` properties that the model carries for each body-level
/// `UsdPhysics` schema.
const BODY_PROPERTIES: &[(&str, &[&str])] = &[
    (
        "PhysicsRigidBodyAPI",
        &[
            "physics:rigidBodyEnabled",
            "physics:kinematicEnabled",
            "physics:velocity",
            "physics:angularVelocity",
            "physics:startsAsleep",
        ],
    ),
    (
        "PhysicsMassAPI",
        &[
            "physics:mass",
            "physics:density",
            "physics:centerOfMass",
            "physics:diagonalInertia",
            "physics:principalAxes",
        ],
    ),
    ("PhysicsCollisionAPI", &["physics:collisionEnabled"]),
    ("PhysicsMeshCollisionAPI", &["physics:approximation"]),
];

/// The authored `physics:` properties of a prim with a body-level
/// `UsdPhysics` schema that the model does not carry.
pub(crate) fn unread_body_properties(prim: &Prim<'_>) -> Vec<String> {
    let schemas = prim.api_schemas();
    let applied: Vec<&[&str]> = BODY_PROPERTIES
        .iter()
        .filter(|(schema, _)| schemas.contains(schema))
        .map(|(_, properties)| *properties)
        .collect();
    if applied.is_empty() {
        return Vec::new();
    }
    prim.property_names()
        .into_iter()
        .filter(|name| name.starts_with("physics:"))
        .filter(|name| !applied.iter().any(|properties| properties.contains(name)))
        .map(str::to_owned)
        .collect()
}

/// The `PhysicsMassAPI` values of a prim (spec §16.3), in scene units. A
/// zero mass or density is unauthored.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MassData {
    pub(crate) mass: Option<f64>,
    pub(crate) density: Option<f64>,
    pub(crate) center_of_mass: Option<[f64; 3]>,
    pub(crate) diagonal_inertia: Option<[f64; 3]>,
    pub(crate) principal_axes: Option<[f64; 4]>,
}

pub(crate) fn mass(prim: &Prim<'_>, issues: &mut Issues) -> MassData {
    let positive = |value: Option<f64>| value.filter(|value| *value > 0.0);
    MassData {
        mass: positive(finite(prim, "physics:mass", issues)),
        density: positive(finite(prim, "physics:density", issues)),
        center_of_mass: tuple(prim, "physics:centerOfMass", issues)
            .filter(|point| point.iter().all(|value| value.is_finite())),
        diagonal_inertia: tuple(prim, "physics:diagonalInertia", issues)
            .filter(|moments: &[f64; 3]| {
                moments
                    .iter()
                    .all(|value| *value >= 0.0 && value.is_finite())
            })
            .filter(|moments| moments.iter().any(|value| *value > 0.0)),
        principal_axes: tuple(prim, "physics:principalAxes", issues)
            .filter(|quaternion: &[f64; 4]| quaternion.iter().any(|value| *value != 0.0)),
    }
}

/// The values of a `PhysicsScene` prim (spec §16.7), in scene units. A
/// magnitude of `-inf` is standard gravity and reads as `None`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SceneData {
    pub(crate) gravity_direction: Option<[f64; 3]>,
    pub(crate) gravity_magnitude: Option<f64>,
}

pub(crate) fn scene(prim: &Prim<'_>, issues: &mut Issues) -> SceneData {
    SceneData {
        gravity_direction: tuple(prim, "physics:gravityDirection", issues)
            .filter(|direction: &[f64; 3]| direction.iter().any(|value| *value != 0.0)),
        gravity_magnitude: finite(prim, "physics:gravityMagnitude", issues),
    }
}

/// The `PhysicsMaterialAPI` values of a material prim (spec §16.6), in
/// scene units.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MaterialData {
    pub(crate) density: Option<f64>,
    pub(crate) static_friction: Option<f64>,
    pub(crate) dynamic_friction: Option<f64>,
    pub(crate) restitution: Option<f64>,
}

pub(crate) fn material(prim: &Prim<'_>, issues: &mut Issues) -> MaterialData {
    let nonnegative = |value: Option<f64>| value.filter(|value| *value >= 0.0);
    MaterialData {
        density: finite(prim, "physics:density", issues).filter(|value| *value > 0.0),
        static_friction: nonnegative(finite(prim, "physics:staticFriction", issues)),
        dynamic_friction: nonnegative(finite(prim, "physics:dynamicFriction", issues)),
        restitution: nonnegative(finite(prim, "physics:restitution", issues)),
    }
}

/// The `PhysicsCollisionAPI` and `PhysicsMeshCollisionAPI` values of a
/// prim (spec §16.3).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CollisionData {
    pub(crate) enabled: bool,
    pub(crate) approximation: Option<String>,
}

pub(crate) fn collision(prim: &Prim<'_>, issues: &mut Issues) -> CollisionData {
    CollisionData {
        enabled: boolean(prim, "physics:collisionEnabled", issues).unwrap_or(true),
        approximation: token(prim, "physics:approximation", issues),
    }
}
