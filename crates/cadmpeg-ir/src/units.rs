// SPDX-License-Identifier: Apache-2.0
//! Canonical units and tolerances.
//!
//! The IR's canonical length unit is the millimeter, matching the `.f3d`
//! exact-BREP contract (Fusion ASM `BinaryFile8` model-space lengths are
//! centimeters and are converted ×10 at decode time; see the f3d container
//! spec §6). The [`Units`] block records what unit the stored coordinates are
//! expressed in so an exporter can report or re-scale honestly.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The canonical IR length unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    /// Millimeter — the IR canonical unit.
    Millimeter,
}

impl LengthUnit {
    /// Scale factor to convert a length in `self` to millimeters.
    pub fn to_millimeters(self) -> f64 {
        1.0
    }
}

/// Unit declaration for a document. Presence of this block is required by
/// validation: an IR document with no declared units cannot be exported safely.
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
