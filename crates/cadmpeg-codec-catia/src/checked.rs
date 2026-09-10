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
