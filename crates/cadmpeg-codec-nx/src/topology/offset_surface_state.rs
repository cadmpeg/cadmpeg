// SPDX-License-Identifier: Apache-2.0
//! Offset-surface support and finite signed distance in model millimetres.

use serde::{Deserialize, Serialize};
use crate::framing::xmt_reference::NonNullXmt;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub(crate) struct OffsetSurfaceState {
    support: NonNullXmt,
    distance: f64,
}

impl OffsetSurfaceState {
    pub(crate) fn new(support: u32, distance: f64) -> Result<Self, &'static str> {
        if !distance.is_finite() { return Err("distance: must be finite"); }
        Ok(Self {
            support: support.try_into().map_err(|_| "support_xmt: must exceed one")?,
            distance,
        })
    }
    pub(crate) fn support(self) -> u32 { self.support.into() }
    pub(crate) fn distance(self) -> f64 { self.distance }
}

#[derive(Serialize, Deserialize)]
struct StateWire {
    support_xmt: u32,
    distance: f64,
}

impl From<OffsetSurfaceState> for StateWire {
    fn from(state: OffsetSurfaceState) -> Self {
        Self { support_xmt: state.support(), distance: state.distance() }
    }
}

impl TryFrom<StateWire> for OffsetSurfaceState {
    type Error = &'static str;
    fn try_from(wire: StateWire) -> Result<Self, Self::Error> {
        Self::new(wire.support_xmt, wire.distance)
    }
}

#[cfg(test)]
mod tests {
    use super::OffsetSurfaceState;

    #[test]
    fn offset_state_preserves_signed_distance_and_checks_support() {
        for distance in [0.0, -0.0, -2.5, f64::MAX] {
            let state = OffsetSurfaceState::new(6, distance).unwrap();
            let wire = serde_json::to_string(&state).unwrap();
            assert_eq!(wire, format!("{{\"support_xmt\":6,\"distance\":{}}}", serde_json::to_string(&distance).unwrap()));
            let decoded: OffsetSurfaceState = serde_json::from_str(&wire).unwrap();
            assert_eq!(decoded.distance().to_bits(), distance.to_bits());
        }
        for support in [0, 1] {
            let wire = format!("{{\"support_xmt\":{support},\"distance\":2.5}}");
            assert!(serde_json::from_str::<OffsetSurfaceState>(&wire).unwrap_err().to_string().contains("support_xmt"));
        }
        for distance in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(OffsetSurfaceState::new(6, distance).unwrap_err().contains("distance"));
        }
    }
}
