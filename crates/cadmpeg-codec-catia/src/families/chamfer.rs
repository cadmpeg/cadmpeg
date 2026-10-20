// SPDX-License-Identifier: Apache-2.0
//! Class-`3b` chamfer controls shared by the compact and object-id dialects.

use cadmpeg_ir::scalar::{FiniteReal, PositiveAngle, PositiveLength};
use serde::{Deserialize, Serialize};

/// Canonical representation of the exact result carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChamferResultKind {
    Cone,
    Plane,
}

/// Order of support-0 and support-1 contacts on the transverse result axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChamferContactOrder {
    LowerUpper,
    UpperLower,
}

/// Halfspace containing the result interior relative to a native support normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChamferSide {
    Negative,
    Positive,
}

/// Positive acute angle of the result relative to support 0.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PositiveAngle", into = "PositiveAngle")]
pub(crate) struct ChamferAngle(PositiveAngle);

impl TryFrom<PositiveAngle> for ChamferAngle {
    type Error = &'static str;

    fn try_from(value: PositiveAngle) -> Result<Self, Self::Error> {
        if value.get() >= std::f64::consts::FRAC_PI_2 {
            return Err("chamfer angle must be acute");
        }
        Ok(Self(value))
    }
}

impl From<ChamferAngle> for PositiveAngle {
    fn from(value: ChamferAngle) -> Self {
        value.0
    }
}

/// Distances or distance and angle that define the two-support construction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ChamferDefinition {
    TwoLengths {
        length1: PositiveLength,
        length2: PositiveLength,
    },
    LengthAngle {
        length1: PositiveLength,
        angle: ChamferAngle,
    },
}

/// Complete six-control and two-scalar class-`3b` parameter production.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChamferParameters {
    pub(crate) result_kind: ChamferResultKind,
    pub(crate) contact_order: ChamferContactOrder,
    pub(crate) support_sides: [ChamferSide; 2],
    pub(crate) definition: ChamferDefinition,
}

impl ChamferParameters {
    pub(crate) fn decode(controls: [u8; 6], scalars: [FiniteReal; 2]) -> Option<Self> {
        let result_kind = match controls[0] {
            0x05 => ChamferResultKind::Cone,
            0x09 => ChamferResultKind::Plane,
            _ => return None,
        };
        let contact_order = match controls[1] {
            0x01 => ChamferContactOrder::LowerUpper,
            0x05 => ChamferContactOrder::UpperLower,
            _ => return None,
        };
        if controls[2] != 0x01 {
            return None;
        }
        let side = |code| match code {
            0x03 => Some(ChamferSide::Negative),
            0x05 => Some(ChamferSide::Positive),
            _ => None,
        };
        let support_sides = [side(controls[3])?, side(controls[4])?];
        let length1 = PositiveLength::new(scalars[0].get())?;
        let definition = match controls[5] {
            0x0d => ChamferDefinition::TwoLengths {
                length1,
                length2: PositiveLength::new(scalars[1].get())?,
            },
            0x11 => ChamferDefinition::LengthAngle {
                length1,
                angle: ChamferAngle::try_from(PositiveAngle::new(scalars[1].get())?).ok()?,
            },
            _ => return None,
        };
        Some(Self {
            result_kind,
            contact_order,
            support_sides,
            definition,
        })
    }
}
