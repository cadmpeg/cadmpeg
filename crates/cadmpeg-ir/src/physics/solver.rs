// SPDX-License-Identifier: Apache-2.0
//! Settings that one physics solver states for a model record.
//!
//! A record holds the properties of one solver schema, such as a `PhysX` API
//! schema, on one target. A real value carries the dimension that converts
//! it between unit systems: it is stored in millimetres, kilograms, and
//! radians raised to the dimension's exponents.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{
    ArticulationId, BodyId, ColliderId, OccurrenceId, PhysicalMaterialId, PhysicsSceneId,
    ProductDefinitionId, SolverSettingsId,
};
use crate::products::JointId;
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

/// The exponents of length, mass, and angle in the unit of a real value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    /// Exponent of length; the value is in millimetres to this power.
    #[serde(default)]
    pub length: i8,
    /// Exponent of mass; the value is in kilograms to this power.
    #[serde(default)]
    pub mass: i8,
    /// Exponent of angle; the value is in radians to this power.
    #[serde(default)]
    pub angle: i8,
}

impl Dimension {
    /// A dimensionless value.
    pub const NONE: Self = Self {
        length: 0,
        mass: 0,
        angle: 0,
    };

    /// The dimension with these exponents.
    #[must_use]
    pub const fn new(length: i8, mass: i8, angle: i8) -> Self {
        Self {
            length,
            mass,
            angle,
        }
    }

    /// Whether every exponent is zero.
    #[must_use]
    pub fn is_none(&self) -> bool {
        *self == Self::NONE
    }

    /// The factor that converts a value of this dimension from a unit
    /// system of `millimetres`, `kilograms`, and `radians` per unit to
    /// millimetres, kilograms, and radians.
    #[must_use]
    pub fn factor(&self, millimetres: f64, kilograms: f64, radians: f64) -> f64 {
        millimetres.powi(i32::from(self.length))
            * kilograms.powi(i32::from(self.mass))
            * radians.powi(i32::from(self.angle))
    }
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
    /// A real value in the units its dimension states.
    Real {
        /// The value.
        value: FiniteReal,
        /// The unit of the value.
        #[serde(default, skip_serializing_if = "Dimension::is_none")]
        dimension: Dimension,
    },
    /// An infinite real value: no bound.
    Unbounded {
        /// Whether the value is negative infinity.
        negative: bool,
    },
    /// Real values in the units their dimension states.
    Reals {
        /// The values.
        values: Vec<FiniteReal>,
        /// The unit of every value.
        #[serde(default, skip_serializing_if = "Dimension::is_none")]
        dimension: Dimension,
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
        targets: Vec<SolverTarget>,
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
    pub target: SolverTarget,
    /// The properties by their schema names.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
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
