// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::VariableType;

/// A sketch-section coordinate axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SectionAxis {
    U,
    V,
}

impl SectionAxis {
    /// The section axes in coordinate order.
    pub(crate) const ALL: [Self; 2] = [Self::U, Self::V];
    /// The array index of the coordinate.
    pub(crate) const fn index(self) -> usize {
        self as usize
    }
    /// The other section axis.
    pub(crate) const fn other(self) -> Self {
        match self {
            Self::U => Self::V,
            Self::V => Self::U,
        }
    }
    /// The axis selected by a coordinate variable type.
    pub(crate) fn from_variable(variable: VariableType) -> Option<Self> {
        match variable {
            VariableType::U => Some(Self::U),
            VariableType::V => Some(Self::V),
            _ => None,
        }
    }
    /// The axis selected by a stored U/V selector.
    pub(crate) fn from_selector(selector: u32) -> Option<Self> {
        match selector {
            0 => Some(Self::U),
            1 => Some(Self::V),
            _ => None,
        }
    }
}
