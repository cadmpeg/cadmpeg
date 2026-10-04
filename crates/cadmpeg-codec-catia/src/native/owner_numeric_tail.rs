// SPDX-License-Identifier: Apache-2.0
//! The fixed numeric tail of a class-`0x62` consolidated owner packet.

use cadmpeg_ir::scalar::FiniteBinary32;
use cadmpeg_ir::topology::IncreasingParameterInterval;
use serde::{Deserialize, Serialize};

/// Structurally decoded payload of a class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "CatiaOwnerNumericTailWire",
    into = "CatiaOwnerNumericTailWire"
)]
pub(crate) struct CatiaOwnerNumericTail {
    header: [u8; 5],
    /// The binary64 box as one increasing interval per axis.
    axes: [IncreasingParameterInterval; 2],
    bounds: [[FiniteBinary32; 2]; 3],
}

impl CatiaOwnerNumericTail {
    /// Builds a numeric tail whose binary64 box and binary32 bounds are finite
    /// and strictly increasing along every axis.
    pub(crate) fn new(
        header: [u8; 5],
        lower: [f64; 2],
        upper: [f64; 2],
        bounds: [[f32; 2]; 3],
    ) -> Option<Self> {
        let axes = [
            IncreasingParameterInterval::new([lower[0], upper[0]]),
            IncreasingParameterInterval::new([lower[1], upper[1]]),
        ];
        let [Some(first), Some(second)] = axes else {
            return None;
        };
        let bounds = bounds.map(|[lower, upper]| {
            let lower = FiniteBinary32::new(lower)?;
            let upper = FiniteBinary32::new(upper)?;
            (lower < upper).then_some([lower, upper])
        });
        let [Some(x), Some(y), Some(z)] = bounds else {
            return None;
        };
        Some(Self {
            header,
            axes: [first, second],
            bounds: [x, y, z],
        })
    }

    /// Returns the five-byte class-specific header.
    #[cfg(test)]
    pub(crate) fn header(&self) -> [u8; 5] {
        self.header
    }

    /// Returns the lower coordinate pair of the binary64 box.
    pub(crate) fn lower(&self) -> [f64; 2] {
        self.axes.map(IncreasingParameterInterval::lower)
    }

    /// Returns the upper coordinate pair of the binary64 box.
    pub(crate) fn upper(&self) -> [f64; 2] {
        self.axes.map(IncreasingParameterInterval::upper)
    }

    /// Returns the three binary32 bounds in serialization order. In an
    /// all-compact owner these are the model-space X, Y, and Z bounds.
    pub(crate) fn bounds(&self) -> [[f32; 2]; 3] {
        self.bounds.map(|pair| pair.map(FiniteBinary32::get))
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
            lower: value.lower(),
            upper: value.upper(),
            bounds: value.bounds(),
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
