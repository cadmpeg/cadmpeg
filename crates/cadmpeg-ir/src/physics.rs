// SPDX-License-Identifier: Apache-2.0
//! Physical properties for simulation: physics scenes, physical materials,
//! mass properties, rigid bodies, colliders, articulations, and the settings
//! of particular solvers.
//!
//! Masses are kilograms, densities kilograms per cubic metre, lengths and
//! positions millimetres, and moments of inertia kilogram square
//! millimetres.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::features::{FinitePoint3, FiniteVector3};
use crate::ids::{
    ArticulationId, BodyId, ColliderId, MassPropertiesId, OccurrenceId, PhysicalMaterialId,
    PhysicsSceneId, ProductDefinitionId, RigidBodyId,
};
use crate::scalar::{NonNegativeReal, PositiveReal};
use crate::transform::Transform;
use crate::units::UnitVector3;

pub mod solver;

/// The model entity that a physical record describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PhysicsTarget {
    /// One body, in its own coordinates.
    Body {
        /// The body.
        body: BodyId,
    },
    /// One placed occurrence, in the coordinates of its prototype.
    Occurrence {
        /// The occurrence.
        occurrence: OccurrenceId,
    },
    /// Every placement of a definition, in the definition's coordinates.
    Definition {
        /// The definition.
        definition: ProductDefinitionId,
    },
}

/// Bulk and contact properties of a material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PhysicalMaterial {
    /// Globally unique material identity.
    pub id: PhysicalMaterialId,
    /// Display name stored in the source.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// Mass per unit volume in kilograms per cubic metre.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_density"
    )]
    pub density: Option<PositiveReal>,
    /// Coefficient of static friction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_static_friction"
    )]
    pub static_friction: Option<NonNegativeReal>,
    /// Coefficient of kinetic friction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_dynamic_friction"
    )]
    pub dynamic_friction: Option<NonNegativeReal>,
    /// Coefficient of restitution.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_restitution"
    )]
    pub restitution: Option<NonNegativeReal>,
    /// Format-native record supplying this material.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// Principal moments of inertia about the center of mass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PrincipalInertia {
    /// Moments about the three principal axes, in kilogram square
    /// millimetres.
    pub moments: [NonNegativeReal; 3],
    /// Rotation from the target's axes to the principal axes. Its
    /// translation is zero. Absent when the principal axes are the target's
    /// axes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_axes"
    )]
    pub axes: Option<Transform>,
}

/// Mass, density, center of mass, and inertia of a target.
///
/// A stated mass takes precedence over a density; a density takes precedence
/// over the density of the target's material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MassProperties {
    /// Globally unique record identity.
    pub id: MassPropertiesId,
    /// The described entity.
    pub target: PhysicsTarget,
    /// Mass in kilograms.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_mass"
    )]
    pub mass: Option<PositiveReal>,
    /// Mass per unit volume in kilograms per cubic metre.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_density"
    )]
    pub density: Option<PositiveReal>,
    /// Center of mass in millimetres, in the target's coordinates.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_center_of_mass"
    )]
    pub center_of_mass: Option<FinitePoint3>,
    /// Principal moments of inertia.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_inertia"
    )]
    pub inertia: Option<PrincipalInertia>,
    /// The material of the target.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_material"
    )]
    pub material: Option<PhysicalMaterialId>,
    /// Format-native record supplying these properties.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// A target that moves as one rigid body in a simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RigidBody {
    /// Globally unique record identity.
    pub id: RigidBodyId,
    /// The moving entity: an occurrence or a definition.
    pub target: PhysicsTarget,
    /// Whether the simulation moves the body only by its animated placement.
    pub kinematic: bool,
    /// Whether the simulation includes the body.
    pub enabled: bool,
    /// Initial linear velocity in millimetres per second, in the
    /// coordinates that the target's placement maps into.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_linear_velocity"
    )]
    pub linear_velocity: Option<FiniteVector3>,
    /// Initial angular velocity in radians per second, in the coordinates
    /// that the target's placement maps into.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angular_velocity"
    )]
    pub angular_velocity: Option<FiniteVector3>,
    /// Whether the body starts at rest until a contact or a force wakes it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub starts_asleep: bool,
    /// Format-native record supplying this body.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The shape a collider gives its target's geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColliderShape {
    /// The geometry itself.
    Exact {},
    /// The convex hull of the geometry.
    ConvexHull {},
    /// A union of convex parts that approximates the geometry.
    ConvexDecomposition {},
    /// The bounding box of the geometry.
    BoundingBox {},
    /// The bounding sphere of the geometry.
    BoundingSphere {},
    /// A mesh with fewer faces that approximates the geometry.
    Simplified {},
    /// An application-defined approximation.
    Native {
        /// Application-defined approximation name.
        name: String,
    },
}

/// Geometry that a simulation uses for contact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Collider {
    /// Globally unique record identity.
    pub id: ColliderId,
    /// The geometry: a body, or the bodies of an occurrence or a definition.
    pub target: PhysicsTarget,
    /// The contact shape.
    pub shape: ColliderShape,
    /// Whether the simulation includes the collider.
    pub enabled: bool,
    /// The contact material.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_material"
    )]
    pub material: Option<PhysicalMaterialId>,
    /// Format-native record supplying this collider.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// A tree of rigid bodies that the joints connect and that a simulation
/// solves as one system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Articulation {
    /// Globally unique record identity.
    pub id: ArticulationId,
    /// The root: a rigid body of the tree, or an occurrence or a definition
    /// that holds the tree.
    pub root: PhysicsTarget,
    /// Format-native record supplying this articulation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// A simulated world: the gravity that acts on its rigid bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PhysicsScene {
    /// Globally unique scene identity.
    pub id: PhysicsSceneId,
    /// Display name stored in the source.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The direction of gravity in model coordinates. Absent when the
    /// source states no direction: gravity then acts against the up axis.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_gravity_direction"
    )]
    pub gravity_direction: Option<UnitVector3>,
    /// The magnitude of gravity in millimetres per square second. Absent
    /// when the source states none: gravity is then standard gravity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_gravity_magnitude"
    )]
    pub gravity_magnitude: Option<NonNegativeReal>,
    /// Format-native record supplying this scene.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

mod identity_rewrite;
#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_density, PositiveReal, "density");
cadmpeg_core::named_optional_field!(
    deserialize_static_friction,
    NonNegativeReal,
    "static_friction"
);
cadmpeg_core::named_optional_field!(
    deserialize_dynamic_friction,
    NonNegativeReal,
    "dynamic_friction"
);
cadmpeg_core::named_optional_field!(deserialize_restitution, NonNegativeReal, "restitution");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_axes, Transform, "axes");
cadmpeg_core::named_optional_field!(deserialize_mass, PositiveReal, "mass");
cadmpeg_core::named_optional_field!(deserialize_center_of_mass, FinitePoint3, "center_of_mass");
cadmpeg_core::named_optional_field!(deserialize_inertia, PrincipalInertia, "inertia");
cadmpeg_core::named_optional_field!(deserialize_material, PhysicalMaterialId, "material");
cadmpeg_core::named_optional_field!(
    deserialize_linear_velocity,
    FiniteVector3,
    "linear_velocity"
);
cadmpeg_core::named_optional_field!(
    deserialize_angular_velocity,
    FiniteVector3,
    "angular_velocity"
);
cadmpeg_core::named_optional_field!(
    deserialize_gravity_direction,
    UnitVector3,
    "gravity_direction"
);
cadmpeg_core::named_optional_field!(
    deserialize_gravity_magnitude,
    NonNegativeReal,
    "gravity_magnitude"
);
