// SPDX-License-Identifier: Apache-2.0
//! Checked rolling-ball supports and source-admitted offset pairs.

use serde::{Deserialize, Serialize};
use crate::framing::xmt_reference::{NonNullXmt, XmtTarget};

const EPS_SOURCE_OFFSET_METRES: f64 = 1.0e-9;
const METRES_TO_MM: f64 = 1000.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
struct BlendOffsets([f64; 2]);

impl BlendOffsets {
    fn from_metres(offsets: [f64; 2]) -> Result<Self, &'static str> {
        if offsets.iter().any(|value| !value.is_finite() || *value == 0.0)
            || (offsets[0].abs() - offsets[1].abs()).abs() > EPS_SOURCE_OFFSET_METRES
        {
            return Err("offsets: require finite nonzero offsets with equal source magnitudes");
        }
        let millimetres = offsets.map(|value| value * METRES_TO_MM);
        if millimetres.iter().any(|value| !value.is_finite()) {
            return Err("offsets: model distances must be finite");
        }
        Ok(Self(millimetres))
    }
}

impl From<BlendOffsets> for [f64; 2] {
    fn from(offsets: BlendOffsets) -> Self { offsets.0 }
}

impl TryFrom<[f64; 2]> for BlendOffsets {
    type Error = &'static str;

    fn try_from(offsets: [f64; 2]) -> Result<Self, Self::Error> {
        if offsets.iter().any(|value| !value.is_finite() || *value == 0.0) {
            return Err("offsets: model distances must be finite and nonzero");
        }
        let (first_min, first_max) = source_interval(offsets[0].abs())
            .ok_or("offsets: first model distance has no source-float preimage")?;
        let (second_min, second_max) = source_interval(offsets[1].abs())
            .ok_or("offsets: second model distance has no source-float preimage")?;
        let distance = if first_max < second_min {
            second_min - first_max
        } else if second_max < first_min {
            first_min - second_max
        } else {
            0.0
        };
        if distance > EPS_SOURCE_OFFSET_METRES {
            return Err("offsets: no source pair has equal magnitudes within the source tolerance");
        }
        Ok(Self(offsets))
    }
}

// Positive finite f64 bit patterns have numeric order. Multiplication by the
// positive scale is monotone, so two lower bounds give every source float that
// rounds to the stored distance. Comparing the nearest interval endpoints uses
// the original metre-space subtraction and tolerance, without a scaled epsilon.
fn source_interval(millimetres: f64) -> Option<(f64, f64)> {
    let lower_bound = |strict: bool| {
        let mut low = 1;
        let mut high = f64::MAX.to_bits() + 1;
        while low < high {
            let middle = low + (high - low) / 2;
            let scaled = f64::from_bits(middle) * METRES_TO_MM;
            let before = if strict { scaled <= millimetres } else { scaled < millimetres };
            if before { low = middle + 1; } else { high = middle; }
        }
        low
    };
    let first = f64::from_bits(lower_bound(false));
    if first * METRES_TO_MM != millimetres { return None; }
    let last = f64::from_bits(lower_bound(true) - 1);
    Some((first, last))
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub(crate) struct BlendSurfaceState {
    supports: [NonNullXmt; 2],
    spine: Option<XmtTarget>,
    offsets: BlendOffsets,
    thumb_weights: [f64; 2],
}

impl BlendSurfaceState {
    pub(crate) fn from_metres(supports: [u32; 2], spine: u32, offsets: [f64; 2], thumb_weights: [f64; 2]) -> Result<Self, &'static str> {
        Self::new(supports, spine, BlendOffsets::from_metres(offsets)?, thumb_weights)
    }

    fn new(supports: [u32; 2], spine: u32, offsets: BlendOffsets, thumb_weights: [f64; 2]) -> Result<Self, &'static str> {
        let supports = [
            NonNullXmt::try_from(supports[0]).map_err(|_| "support_xmts: first support must be non-null")?,
            NonNullXmt::try_from(supports[1]).map_err(|_| "support_xmts: second support must be non-null")?,
        ];
        if thumb_weights.iter().any(|value| !value.is_finite()) {
            return Err("thumb_weights: must be finite");
        }
        Ok(Self { supports, spine: XmtTarget::from_wire(spine), offsets, thumb_weights })
    }

    pub(crate) fn support_xmts(&self) -> [u32; 2] { self.supports.map(u32::from) }
    pub(crate) fn spine_xmt(&self) -> u32 { XmtTarget::to_wire(self.spine) }
    pub(crate) fn offsets(&self) -> [f64; 2] { self.offsets.into() }
    #[cfg(test)]
    pub(crate) fn thumb_weights(&self) -> [f64; 2] { self.thumb_weights }
}

#[derive(Serialize, Deserialize)]
struct StateWire {
    support_xmts: [u32; 2],
    spine_xmt: u32,
    offsets: BlendOffsets,
    thumb_weights: [f64; 2],
}

impl From<BlendSurfaceState> for StateWire {
    fn from(state: BlendSurfaceState) -> Self {
        Self { support_xmts: state.support_xmts(), spine_xmt: state.spine_xmt(), offsets: state.offsets, thumb_weights: state.thumb_weights }
    }
}

impl TryFrom<StateWire> for BlendSurfaceState {
    type Error = &'static str;
    fn try_from(wire: StateWire) -> Result<Self, Self::Error> {
        Self::new(wire.support_xmts, wire.spine_xmt, wire.offsets, wire.thumb_weights)
    }
}

#[cfg(test)]
mod tests {
    use super::{BlendOffsets, BlendSurfaceState, EPS_SOURCE_OFFSET_METRES, METRES_TO_MM, source_interval};

    #[test]
    fn model_offsets_retain_source_tolerance_after_rounding() {
        let source = [0.003, 0.003 + EPS_SOURCE_OFFSET_METRES];
        let offsets = BlendOffsets::from_metres(source).unwrap();
        let model: [f64; 2] = offsets.into();
        assert!((model[0] - model[1]).abs() > EPS_SOURCE_OFFSET_METRES * METRES_TO_MM);
        let json = serde_json::to_string(&offsets).unwrap();
        assert_eq!(serde_json::from_str::<BlendOffsets>(&json).unwrap(), offsets);
    }

    #[test]
    fn inverse_scale_covers_subnormal_and_large_source_values() {
        for value in [f64::from_bits(1), f64::MIN_POSITIVE, 0.003, 1.0, 1.0e300] {
            let model = value * METRES_TO_MM;
            let (first, last) = source_interval(model).unwrap();
            assert!(first <= value && value <= last);
            assert_eq!(first * METRES_TO_MM, model);
            assert_eq!(last * METRES_TO_MM, model);
            assert!(first.to_bits() == 1 || f64::from_bits(first.to_bits() - 1) * METRES_TO_MM < model);
            assert!(f64::from_bits(last.to_bits() + 1) * METRES_TO_MM > model);
        }
        assert!(BlendOffsets::try_from([f64::from_bits(1); 2]).is_err());
        assert!(BlendOffsets::try_from([3.0, 4.0]).is_err());
    }

    #[test]
    fn surface_wire_preserves_open_spines_and_rejects_invalid_payloads() {
        for spine in [0, 1, 2] {
            let state = BlendSurfaceState::from_metres([6, 7], spine, [-0.003, 0.003], [-0.0, -2.0]).unwrap();
            let json = format!(r#"{{"support_xmts":[6,7],"spine_xmt":{spine},"offsets":[-3.0,3.0],"thumb_weights":[-0.0,-2.0]}}"#);
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
            assert_eq!(serde_json::from_str::<BlendSurfaceState>(&json).unwrap(), state);
        }
        for (supports, offsets, weights, field) in [
            ([1, 7], [-0.003, 0.003], [1.0, 1.0], "support_xmts"),
            ([6, 7], [0.0, 0.003], [1.0, 1.0], "offsets"),
            ([6, 7], [f64::MAX; 2], [1.0, 1.0], "offsets"),
            ([6, 7], [-0.003, 0.003], [f64::NAN, 1.0], "thumb_weights"),
        ] {
            assert!(BlendSurfaceState::from_metres(supports, 1, offsets, weights).unwrap_err().contains(field));
        }
    }
}
