// SPDX-License-Identifier: Apache-2.0
//! Reserved assembly model.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Placeholder for assembly instancing, component trees, and joint constraints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum Assembly {
    /// Marks assembly structure as unavailable.
    Reserved,
}
