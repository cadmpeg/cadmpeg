// SPDX-License-Identifier: Apache-2.0

/// A model-space coordinate axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// The model axes in coordinate order.
    pub(crate) const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];
    /// The array index of the axis.
    pub(crate) const fn index(self) -> usize {
        self as usize
    }
    /// The two perpendicular model axes.
    pub(crate) const fn complement(self) -> [Self; 2] {
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
    /// The opposite direction.
    pub(super) const fn reversed(self) -> Self {
        match self {
            Self::Positive => Self::Negative,
            Self::Negative => Self::Positive,
        }
    }
    /// The direction of a coordinate component, taking the IEEE sign bit.
    pub(super) fn of_component(component: f64) -> Self {
        if component.is_sign_negative() {
            Self::Negative
        } else {
            Self::Positive
        }
    }
}

impl From<crate::curve::ParameterSense> for Sign {
    fn from(sense: crate::curve::ParameterSense) -> Self {
        match sense {
            crate::curve::ParameterSense::Increasing => Self::Positive,
            crate::curve::ParameterSense::Decreasing => Self::Negative,
        }
    }
}
