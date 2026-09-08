// SPDX-License-Identifier: Apache-2.0
//! Canonical units and tolerances.
//!
//! Stored lengths and coordinates use millimeters. Angular quantities use
//! radians.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct CanonicalUnitsWire {
    length: CanonicalLengthUnitWire,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum CanonicalLengthUnitWire {
    #[default]
    Millimeter,
}

/// Maximum distance, in the document's length unit, between an evaluated
/// carrier point and the vertex position it must coincide with. Exact carriers
/// agree to rational-weight rounding (well under `1e-3` mm); real mismatches
/// from decoder defects start orders of magnitude above this bound. A decoder
/// that binds a carrier to topology applies the same bound, so a binding it
/// accepts is one the topology contract also accepts.
pub const COINCIDENCE_TOLERANCE: f64 = 0.01;

const DEFAULT_LINEAR_TOLERANCE: f64 = 1.0e-6;
const DEFAULT_ANGULAR_TOLERANCE: f64 = 1.0e-10;

/// Document-wide linear and angular tolerances.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Tolerances {
    /// Linear tolerance in millimeters.
    pub linear: f64,
    /// Angular tolerance in radians.
    pub angular: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        Tolerances {
            linear: DEFAULT_LINEAR_TOLERANCE,
            angular: DEFAULT_ANGULAR_TOLERANCE,
        }
    }
}
