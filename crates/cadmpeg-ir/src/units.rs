// SPDX-License-Identifier: Apache-2.0
//! Units and length conversion.
//!
//! The IR's canonical length unit is the millimeter, matching the `.f3d`
//! exact-BREP contract (Fusion ASM `BinaryFile8` model-space lengths are
//! centimeters and are converted ×10 at decode time; see the f3d container
//! spec §6). The [`Units`] block records what unit the stored coordinates are
//! expressed in so an exporter can report or re-scale honestly.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A length unit. The IR canonicalizes to [`LengthUnit::Millimeter`]; other
/// variants exist so a decoder can record a source unit it has not yet
/// converted, and so validation can flag a non-canonical document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    /// Millimeter — the IR canonical unit.
    Millimeter,
    /// Centimeter (Fusion ASM native model-space unit before conversion).
    Centimeter,
    /// Meter.
    Meter,
    /// Inch.
    Inch,
}

impl LengthUnit {
    /// Scale factor to convert a length in `self` to millimeters.
    pub fn to_millimeters(self) -> f64 {
        match self {
            LengthUnit::Millimeter => 1.0,
            LengthUnit::Centimeter => 10.0,
            LengthUnit::Meter => 1000.0,
            LengthUnit::Inch => 25.4,
        }
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

/// Kernel tolerances preserved as metadata.
///
/// For `.f3d` these are the ASM header's `resabs` (absolute distance tolerance)
/// and `resnor` (normal tolerance). They are corpus-constant kernel defaults,
/// not per-model values, but a faithful IR preserves them (spec §5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tolerances {
    /// Absolute distance tolerance in the document's length unit.
    pub resabs: f64,
    /// Normal (angular) tolerance, dimensionless.
    pub resnor: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        // The ASM/ACIS kernel defaults observed so far.
        Tolerances {
            resabs: 1e-6,
            resnor: 1e-10,
        }
    }
}
