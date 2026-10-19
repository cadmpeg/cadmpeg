// SPDX-License-Identifier: Apache-2.0
//! The actual authored pair controlling a helix, without a computed third copy.

use super::{DesignParameter, Dimension, HelixShape};
use crate::scalar::{Length, NonZeroLength, PositiveReal};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The two canonical dimensional controls used to author axial helix travel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelixLaw {
    /// Signed pitch and positive turn count are authored.
    PitchRevolutions {
        /// Canonical signed axial rise per turn.
        pitch: Dimension<NonZeroLength>,
        /// Canonical positive turn count.
        revolutions: Dimension<PositiveReal>,
    },
    /// Signed total height and signed pitch are authored.
    HeightPitch {
        /// Canonical signed total axial rise.
        height: Dimension<Length>,
        /// Canonical signed axial rise per turn.
        pitch: Dimension<NonZeroLength>,
    },
    /// Signed total height and positive turn count are authored.
    HeightRevolutions {
        /// Canonical signed total axial rise; zero states a planar curve.
        height: Dimension<Length>,
        /// Canonical positive turn count.
        revolutions: Dimension<PositiveReal>,
    },
}

/// A temporary evaluation of an authored helix law, never document state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HelixEvaluation {
    /// Evaluated signed axial rise per turn.
    pub pitch: Length,
    /// Evaluated signed total axial rise.
    pub height: Length,
    /// Evaluated positive turn count.
    pub revolutions: PositiveReal,
}

impl HelixLaw {
    /// Evaluate the complete law from the current typed canonical parameter arena.
    pub fn resolved_from(&self, parameters: &[DesignParameter]) -> Option<HelixEvaluation> {
        let (pitch, height, revolutions) = match self {
            Self::PitchRevolutions { pitch, revolutions } => {
                let pitch = Length::new(pitch.value_from(parameters)?.get())?;
                let revolutions = revolutions.value_from(parameters)?;
                (
                    pitch,
                    Length::new(pitch.get() * revolutions.get())?,
                    revolutions,
                )
            }
            Self::HeightPitch { height, pitch } => {
                let height = height.value_from(parameters)?;
                let pitch = Length::new(pitch.value_from(parameters)?.get())?;
                (
                    pitch,
                    height,
                    PositiveReal::new(height.get() / pitch.get())?,
                )
            }
            Self::HeightRevolutions {
                height,
                revolutions,
            } => {
                let height = height.value_from(parameters)?;
                let revolutions = revolutions.value_from(parameters)?;
                (
                    Length::new(height.get() / revolutions.get())?,
                    height,
                    revolutions,
                )
            }
        };
        Some(HelixEvaluation {
            pitch,
            height,
            revolutions,
        })
    }
    /// Resolve signed axial rise per turn, retaining zero for a planar height/turn law.
    pub fn resolved_pitch(&self, ir: &crate::CadIr) -> Option<Length> {
        match self {
            Self::PitchRevolutions { pitch, .. } | Self::HeightPitch { pitch, .. } => {
                Length::new(ir.dimension_value(pitch)?.get())
            }
            Self::HeightRevolutions {
                height,
                revolutions,
            } => Length::new(
                ir.dimension_value(height)?.get() / ir.dimension_value(revolutions)?.get(),
            ),
        }
    }
    /// Resolve signed total axial rise from the authored pair.
    pub fn resolved_height(&self, ir: &crate::CadIr) -> Option<Length> {
        match self {
            Self::HeightPitch { height, .. } | Self::HeightRevolutions { height, .. } => {
                ir.dimension_value(height)
            }
            Self::PitchRevolutions { pitch, revolutions } => Length::new(
                ir.dimension_value(pitch)?.get() * ir.dimension_value(revolutions)?.get(),
            ),
        }
    }
    /// Resolve the positive turn count from the authored pair.
    pub fn resolved_revolutions(&self, ir: &crate::CadIr) -> Option<PositiveReal> {
        match self {
            Self::PitchRevolutions { revolutions, .. }
            | Self::HeightRevolutions { revolutions, .. } => ir.dimension_value(revolutions),
            Self::HeightPitch { height, pitch } => PositiveReal::new(
                ir.dimension_value(height)?.get() / ir.dimension_value(pitch)?.get(),
            ),
        }
    }
}

impl HelixShape {
    /// The authored axial travel law, absent for a planar radial-growth spiral.
    pub const fn law(&self) -> Option<&HelixLaw> {
        match self {
            Self::Cylindrical { law } | Self::Conical { law, .. } => Some(law),
            Self::Spiral { .. } => None,
        }
    }
    /// Resolve the turn count without a second persisted evaluation.
    pub fn resolved_revolutions(&self, ir: &crate::CadIr) -> Option<PositiveReal> {
        match self {
            Self::Cylindrical { law } | Self::Conical { law, .. } => law.resolved_revolutions(ir),
            Self::Spiral { revolutions, .. } => ir.dimension_value(revolutions),
        }
    }
}

rewrite_enum!(HelixLaw,[];{
    PitchRevolutions{pitch,revolutions},
    HeightPitch{height,pitch},
    HeightRevolutions{height,revolutions},
});

#[cfg(test)]
mod tests;
