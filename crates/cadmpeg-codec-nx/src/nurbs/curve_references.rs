// SPDX-License-Identifier: Apache-2.0
//! Reference layouts carried by a B_CURVE_DESCRIPTOR.

use crate::framing::xmt_reference::NonNullXmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurveDescriptorReferences {
    Compact([u32; 2]),
    Status([NonNullXmt; 3]),
}

impl CurveDescriptorReferences {
    pub(crate) fn multiplicities(self) -> u32 {
        match self {
            Self::Compact([multiplicities, _]) => multiplicities,
            Self::Status([_, multiplicities, _]) => multiplicities.into(),
        }
    }

    pub(crate) fn knots(self) -> u32 {
        match self {
            Self::Compact([_, knots]) => knots,
            Self::Status([_, _, knots]) => knots.into(),
        }
    }

    pub(crate) fn values(self) -> Vec<u32> {
        match self {
            Self::Compact(references) => references.to_vec(),
            Self::Status(references) => references.into_iter().map(u32::from).collect(),
        }
    }
}

impl TryFrom<Vec<u32>> for CurveDescriptorReferences {
    type Error = &'static str;

    fn try_from(references: Vec<u32>) -> Result<Self, Self::Error> {
        match references.as_slice() {
            &[multiplicities, knots] => Ok(Self::Compact([multiplicities, knots])),
            &[prefix, multiplicities, knots] => Ok(Self::Status([
                NonNullXmt::try_from(prefix)?,
                NonNullXmt::try_from(multiplicities)?,
                NonNullXmt::try_from(knots)?,
            ])),
            _ => Err("references: B_CURVE_DESCRIPTOR requires two compact or three status references"),
        }
    }
}
