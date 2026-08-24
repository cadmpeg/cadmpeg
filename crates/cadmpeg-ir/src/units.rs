// SPDX-License-Identifier: Apache-2.0
//! Canonical units and tolerances.
//!
//! Stored lengths and coordinates use millimeters. Angular quantities use
//! radians.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const EPS_UNITS_DEFAULT_2_E6: f64 = 1e-6;
const EPS_UNITS_DEFAULT_2_E10: f64 = 1e-10;

/// The canonical IR length unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    /// Millimeter, the IR canonical length unit.
    Millimeter,
}

/// Unit declaration for stored document coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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

/// Maximum distance, in the document's length unit, between an evaluated
/// carrier point and the vertex position it must coincide with. Exact carriers
/// agree to rational-weight rounding (well under `1e-3` mm); real mismatches
/// from decoder defects start orders of magnitude above this bound. A decoder
/// that binds a carrier to topology applies the same bound, so a binding it
/// accepts is one the topology contract also accepts.
pub const COINCIDENCE_TOLERANCE: f64 = 0.01;

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
            linear: EPS_UNITS_DEFAULT_2_E6,
            angular: EPS_UNITS_DEFAULT_2_E10,
        }
    }
}
