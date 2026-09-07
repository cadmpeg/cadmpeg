// SPDX-License-Identifier: Apache-2.0

/// A model-space coordinate axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub(super) const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];
    pub(super) const fn index(self) -> usize {
        self as usize
    }
    pub(super) const fn complement(self) -> [Self; 2] {
        match self {
            Self::X => [Self::Y, Self::Z],
            Self::Y => [Self::X, Self::Z],
            Self::Z => [Self::X, Self::Y],
        }
    }
}
