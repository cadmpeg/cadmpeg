// SPDX-License-Identifier: Apache-2.0
//! Checked scalar owners for native record invariants.

use serde::{Deserialize, Serialize};

/// A finite, strictly positive scalar.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct PositiveFinite(f64);

impl PositiveFinite {
    /// Constructs a finite, strictly positive scalar.
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value > 0.0).then_some(Self(value))
    }

    /// Returns the scalar.
    pub fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for PositiveFinite {
    type Error = String;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value).ok_or_else(|| format!("{value} is not finite and positive"))
    }
}

impl From<PositiveFinite> for f64 {
    fn from(value: PositiveFinite) -> Self {
        value.0
    }
}

/// A finite direction whose squared length is one within a stated tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]", into = "[f64; 3]")]
pub struct UnitVector3([f64; 3]);

impl UnitVector3 {
    /// Squared-length tolerance of the exactly stored record directions.
    pub const EXACT_TOLERANCE: f64 = 1.0e-12;

    /// Squared-length tolerance of the loosely stored record directions.
    pub const RELAXED_TOLERANCE: f64 = 1.0e-9;

    /// Constructs a unit direction whose squared length is one within `tolerance`.
    pub fn new(value: [f64; 3], tolerance: f64) -> Option<Self> {
        let squared_length = value
            .iter()
            .map(|component| component * component)
            .sum::<f64>();
        (value.iter().all(|component| component.is_finite())
            && (squared_length - 1.0).abs() <= tolerance)
            .then_some(Self(value))
    }

    /// Constructs a unit direction whose length is one within `tolerance`.
    pub fn from_length(value: [f64; 3], tolerance: f64) -> Option<Self> {
        let length = value[0].hypot(value[1]).hypot(value[2]);
        (value.iter().all(|component| component.is_finite()) && (length - 1.0).abs() <= tolerance)
            .then_some(Self(value))
    }

    /// Returns the direction components.
    pub fn get(self) -> [f64; 3] {
        self.0
    }
}

impl TryFrom<[f64; 3]> for UnitVector3 {
    type Error = String;

    fn try_from(value: [f64; 3]) -> Result<Self, Self::Error> {
        Self::new(value, Self::RELAXED_TOLERANCE)
            .ok_or_else(|| "direction is not a finite unit vector".to_owned())
    }
}

impl From<UnitVector3> for [f64; 3] {
    fn from(value: UnitVector3) -> Self {
        value.0
    }
}

/// A finite, strictly increasing scalar interval.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub struct OrderedInterval([f64; 2]);

impl OrderedInterval {
    /// Constructs a finite interval whose lower bound is below its upper bound.
    pub fn new(value: [f64; 2]) -> Option<Self> {
        (value.iter().all(|bound| bound.is_finite()) && value[0] < value[1]).then_some(Self(value))
    }

    /// Returns the interval bounds.
    pub fn get(self) -> [f64; 2] {
        self.0
    }

    /// Returns the lower bound.
    pub fn lower(self) -> f64 {
        self.0[0]
    }

    /// Returns the upper bound.
    pub fn upper(self) -> f64 {
        self.0[1]
    }
}

impl TryFrom<[f64; 2]> for OrderedInterval {
    type Error = String;

    fn try_from(value: [f64; 2]) -> Result<Self, Self::Error> {
        Self::new(value).ok_or_else(|| "interval is not finite and increasing".to_owned())
    }
}

impl From<OrderedInterval> for [f64; 2] {
    fn from(value: OrderedInterval) -> Self {
        value.0
    }
}
