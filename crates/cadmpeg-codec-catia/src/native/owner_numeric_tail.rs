// SPDX-License-Identifier: Apache-2.0
//! The fixed numeric tail of a class-`0x62` consolidated owner packet.

use serde::{Deserialize, Serialize};

/// Structurally decoded payload of a class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "CatiaOwnerNumericTailWire",
    into = "CatiaOwnerNumericTailWire"
)]
pub struct CatiaOwnerNumericTail {
    header: [u8; 5],
    lower: [f64; 2],
    upper: [f64; 2],
    bounds: [[f32; 2]; 3],
}

impl CatiaOwnerNumericTail {
    /// Builds a numeric tail whose binary64 box and binary32 bounds are finite
    /// and strictly increasing along every axis.
    pub fn new(
        header: [u8; 5],
        lower: [f64; 2],
        upper: [f64; 2],
        bounds: [[f32; 2]; 3],
    ) -> Option<Self> {
        (lower
            .iter()
            .zip(&upper)
            .all(|(lower, upper)| lower.is_finite() && upper.is_finite() && lower < upper)
            && bounds
                .iter()
                .all(|bound| bound[0].is_finite() && bound[1].is_finite() && bound[0] < bound[1]))
        .then_some(Self {
            header,
            lower,
            upper,
            bounds,
        })
    }

    /// Returns the five-byte class-specific header.
    #[cfg(test)]
    pub fn header(&self) -> [u8; 5] {
        self.header
    }

    /// Returns the lower coordinate pair of the binary64 box.
    pub fn lower(&self) -> [f64; 2] {
        self.lower
    }

    /// Returns the upper coordinate pair of the binary64 box.
    pub fn upper(&self) -> [f64; 2] {
        self.upper
    }

    /// Returns the three binary32 bounds in serialization order. In an
    /// all-compact owner these are the model-space X, Y, and Z bounds.
    pub fn bounds(&self) -> [[f32; 2]; 3] {
        self.bounds
    }
}

#[derive(Serialize, Deserialize)]
struct CatiaOwnerNumericTailWire {
    header: [u8; 5],
    lower: [f64; 2],
    upper: [f64; 2],
    bounds: [[f32; 2]; 3],
}

impl From<CatiaOwnerNumericTail> for CatiaOwnerNumericTailWire {
    fn from(value: CatiaOwnerNumericTail) -> Self {
        Self {
            header: value.header,
            lower: value.lower,
            upper: value.upper,
            bounds: value.bounds,
        }
    }
}

impl TryFrom<CatiaOwnerNumericTailWire> for CatiaOwnerNumericTail {
    type Error = String;

    fn try_from(wire: CatiaOwnerNumericTailWire) -> Result<Self, Self::Error> {
        Self::new(wire.header, wire.lower, wire.upper, wire.bounds).ok_or_else(|| {
            "owner numeric tail box and bounds must be finite and increasing".to_owned()
        })
    }
}
