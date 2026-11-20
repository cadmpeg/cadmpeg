// SPDX-License-Identifier: Apache-2.0
//! Settings that one physics solver states for a model record.
//!
//! A record holds the properties of one solver schema, such as a `PhysX` API
//! schema, on one target. A real value carries the quantity that converts
//! it between unit systems: it is stored in millimetres, kilograms, radians,
//! and seconds raised to the quantity's exponents.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::units::Quantity;
use crate::ids::{
    ArticulationId, BodyId, ColliderId, OccurrenceId, PhysicalMaterialId, PhysicsSceneId,
    ProductDefinitionId, SolverSettingsId,
};
use crate::products::JointId;
use crate::references::InstanceTarget;
use crate::scalar::FiniteReal;

/// The model record that solver settings apply to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SolverTarget {
    /// A physics scene.
    Scene {
        /// The scene.
        scene: PhysicsSceneId,
    },
    /// A placed occurrence.
    Occurrence {
        /// The occurrence.
        occurrence: OccurrenceId,
    },
    /// Every placement of a definition.
    Definition {
        /// The definition.
        definition: ProductDefinitionId,
    },
    /// A body.
    Body {
        /// The body.
        body: BodyId,
    },
    /// A collider.
    Collider {
        /// The collider.
        collider: ColliderId,
    },
    /// An assembly joint.
    Joint {
        /// The joint.
        joint: JointId,
    },
    /// A physical material.
    PhysicalMaterial {
        /// The material.
        material: PhysicalMaterialId,
    },
    /// An articulation.
    Articulation {
        /// The articulation.
        articulation: ArticulationId,
    },
}

/// One solver property value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SolverValue {
    /// A switch.
    Bool {
        /// The value.
        value: bool,
    },
    /// An integer.
    Integer {
        /// The value.
        value: i64,
    },
    /// A real value in the canonical unit of its quantity.
    Real {
        /// The value.
        value: FiniteReal,
        /// The quantity of the value.
        dimension: Quantity,
    },
    /// An infinite real value: no bound.
    Unbounded {
        /// Whether the value is negative infinity.
        negative: bool,
    },
    /// Real values in the canonical unit of their quantity.
    Reals {
        /// The values.
        values: Vec<FiniteReal>,
        /// The quantity of every value.
        dimension: Quantity,
    },
    /// A token from the schema's vocabulary.
    Token {
        /// The token.
        value: String,
    },
    /// Tokens from the schema's vocabulary.
    Tokens {
        /// The tokens.
        values: Vec<String>,
    },
    /// Model records that the property names.
    Targets {
        /// The records.
        targets: Vec<InstanceTarget<SolverTarget>>,
    },
}

/// The properties that one solver schema states for one target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SolverSettings {
    /// Globally unique record identity.
    pub id: SolverSettingsId,
    /// The solver, such as `physx`.
    pub solver: String,
    /// The solver schema, with its instance name when the schema applies
    /// more than once, such as `PhysxLimitAPI:angular`.
    pub schema: String,
    /// The record the settings apply to.
    pub target: InstanceTarget<SolverTarget>,
    /// The properties by their schema names.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub values: BTreeMap<String, SolverValue>,
    /// Format-native record supplying these settings.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");

mod identity_rewrite;
#[cfg(test)]
mod tests;
