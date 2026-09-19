// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::VariableType;

/// A sketch-section coordinate axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::decode) enum SectionAxis {
    U,
    V,
}

impl SectionAxis {
    /// The section axes in coordinate order.
    pub(super) const ALL: [Self; 2] = [Self::U, Self::V];
    /// The array index of the coordinate.
    pub(in crate::decode) const fn index(self) -> usize {
        self as usize
    }
    /// The other section axis.
    pub(super) const fn other(self) -> Self {
        match self {
            Self::U => Self::V,
            Self::V => Self::U,
        }
    }
    /// The axis selected by a coordinate variable type.
    pub(super) fn from_variable(variable: VariableType) -> Option<Self> {
        match variable {
            VariableType::U => Some(Self::U),
            VariableType::V => Some(Self::V),
            _ => None,
        }
    }
    /// The axis selected by a stored U/V selector.
    pub(super) fn from_selector(selector: u32) -> Option<Self> {
        match selector {
            0 => Some(Self::U),
            1 => Some(Self::V),
            _ => None,
        }
    }
}
