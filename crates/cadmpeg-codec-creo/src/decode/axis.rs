// SPDX-License-Identifier: Apache-2.0

/// A model-space coordinate axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// The model axes in coordinate order.
    pub(super) const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];
    /// The array index of the axis.
    pub(super) const fn index(self) -> usize {
        self as usize
    }
    /// The two perpendicular model axes.
    pub(super) const fn complement(self) -> [Self; 2] {
        match self {
            Self::X => [Self::Y, Self::Z],
            Self::Y => [Self::X, Self::Z],
            Self::Z => [Self::X, Self::Y],
        }
    }
}

/// The direction of a signed model axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Sign {
    Positive,
    Negative,
}

impl Sign {
    /// The unit scale for the direction.
    pub(super) const fn scale(self) -> f64 {
        match self {
            Self::Positive => 1.0,
            Self::Negative => -1.0,
        }
    }
}
