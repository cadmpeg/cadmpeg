// SPDX-License-Identifier: Apache-2.0
//! Product prototypes and placed occurrence trees.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{BodyId, OccurrenceId, ProductId};
use crate::transform::Transform;

/// A reusable product prototype with zero or more shape bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Product {
    /// Stable product identity.
    pub id: ProductId,
    /// Source product identifier or part number.
    pub product_id: String,
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Prototype shape bodies owned by this product.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<BodyId>,
}

/// Position of an occurrence in the product tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OccurrenceParent {
    /// A root occurrence has no containing occurrence.
    Root,
    /// A child occurrence is placed inside another occurrence.
    Occurrence {
        /// Containing occurrence identity.
        occurrence: OccurrenceId,
    },
}

/// One placed use of a product prototype.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProductOccurrence {
    /// Stable occurrence identity.
    pub id: OccurrenceId,
    /// Reused product prototype.
    pub product: ProductId,
    /// Position in the occurrence tree.
    pub parent: OccurrenceParent,
    /// Placement relative to the parent occurrence, or model space for a root.
    pub transform: Transform,
    /// Source occurrence identifier or display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}
