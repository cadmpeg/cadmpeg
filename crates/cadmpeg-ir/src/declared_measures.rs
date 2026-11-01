// SPDX-License-Identifier: Apache-2.0
//! Geometric quantities that a source states for its own bodies, so that a
//! reader can compare them with the quantities it measures on the decoded
//! geometry.
//!
//! Volumes are cubic millimetres, areas square millimetres, and points
//! millimetres, in the coordinates of the target.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::features::FinitePoint3;
use crate::ids::{BodyId, DeclaredMeasureId, ProductDefinitionId};
use crate::scalar::FiniteReal;

/// The bodies that a declared measure describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeclaredTarget {
    /// The bodies that a definition lists, in the definition's coordinates.
    /// A solid quantity covers its solid bodies and a sheet quantity its
    /// sheet bodies; a body whose `visible` is `false` is not covered.
    Definition {
        /// The definition.
        definition: ProductDefinitionId,
    },
    /// The listed bodies together, each in its own coordinates.
    Bodies {
        /// The bodies.
        bodies: Vec<BodyId>,
    },
    /// The source names a target that does not resolve to bodies of the
    /// model.
    Unresolved,
}

/// One stated quantity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeclaredQuantity {
    /// The enclosed volume of the solids, in cubic millimetres.
    Volume {
        /// The volume.
        value: FiniteReal,
    },
    /// The area of every face of the solids, void shells included, in
    /// square millimetres.
    SolidArea {
        /// The area.
        value: FiniteReal,
    },
    /// The area of the faces of the outer shells of the solids, in square
    /// millimetres.
    WettedArea {
        /// The area.
        value: FiniteReal,
    },
    /// The centroid of the volume of the solids, in millimetres.
    SolidCentroid {
        /// The centroid.
        value: FinitePoint3,
    },
    /// The area of the faces of the sheets, in square millimetres.
    SheetArea {
        /// The area.
        value: FiniteReal,
    },
    /// The centroid of the area of the sheets, in millimetres.
    SheetCentroid {
        /// The centroid.
        value: FinitePoint3,
    },
}

/// A quantity that the source states for a target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeclaredMeasure {
    /// Globally unique record identity.
    pub id: DeclaredMeasureId,
    /// The described bodies.
    pub target: DeclaredTarget,
    /// The stated quantity.
    pub quantity: DeclaredQuantity,
    /// Description text stored with the quantity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_label"
    )]
    pub label: Option<String>,
    /// Format-native record supplying the quantity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

mod identity_rewrite;

cadmpeg_core::named_optional_field!(deserialize_label, String, "label");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
