// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    Articulation, Collider, ColliderShape, MassProperties, PhysicalMaterial, PhysicsScene,
    PhysicsTarget, PrincipalInertia, RigidBody,
};

rewrite_enum!(PhysicsTarget, []; {
    Body {body},
    Occurrence {occurrence},
    Definition {definition},
});
rewrite_record!(PhysicalMaterial, []; {id, name, density, static_friction, dynamic_friction, restitution, native_ref});
rewrite_record!(PrincipalInertia, []; {moments, axes});
rewrite_record!(MassProperties, []; {id, target, mass, density, center_of_mass, inertia, material, native_ref});
rewrite_record!(RigidBody, []; {id, target, kinematic, enabled, linear_velocity, angular_velocity, starts_asleep, native_ref});
rewrite_record!(Articulation, []; {id, root, native_ref});
rewrite_scalar!(ColliderShape);
rewrite_record!(Collider, []; {id, target, shape, enabled, material, native_ref});
rewrite_record!(PhysicsScene, []; {id, name, gravity_direction, gravity_magnitude, native_ref});
