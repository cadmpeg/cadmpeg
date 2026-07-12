// SPDX-License-Identifier: Apache-2.0
//! Canonical units and tolerances.
//!
//! Stored lengths and coordinates use millimeters. Angular quantities use
//! radians.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The canonical IR length unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    /// Millimeter, the IR canonical length unit.
    Millimeter,
}

/// Unit declaration for stored document coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Units {
    /// Unit the stored coordinate values are expressed in.
    pub length: LengthUnit,
}

impl Default for Units {
    fn default() -> Self {
        Units {
            length: LengthUnit::Millimeter,
        }
    }
}

/// Document-wide linear and angular tolerances.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tolerances {
    /// Linear tolerance in millimeters.
    pub linear: f64,
    /// Angular tolerance in radians.
    pub angular: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        Tolerances {
            linear: 1e-6,
            angular: 1e-10,
        }
    }
}
