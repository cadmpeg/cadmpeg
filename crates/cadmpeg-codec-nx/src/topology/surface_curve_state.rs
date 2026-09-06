// SPDX-License-Identifier: Apache-2.0
//! Surface-curve references and finite source tolerance.

use serde::{Deserialize, Serialize};
use crate::framing::xmt_reference::NonNullXmt;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub(crate) struct SurfaceCurveState {
    surface: NonNullXmt,
    pcurve: NonNullXmt,
    original: Option<u32>,
    tolerance: f64,
}
impl SurfaceCurveState {
    pub(crate) fn new(surface: u32, pcurve: u32, original: u32, tolerance: f64) -> Result<Self, &'static str> {
        if !tolerance.is_finite() { return Err("tolerance_to_original: must be finite"); }
        Ok(Self {
            surface: surface.try_into().map_err(|_| "surface_xmt: must exceed one")?,
            pcurve: pcurve.try_into().map_err(|_| "pcurve_xmt: must exceed one")?,
            original: (original != 1).then_some(original),
            tolerance,
        })
    }
    pub(crate) fn surface(self) -> u32 { self.surface.into() }
    pub(crate) fn pcurve(self) -> u32 { self.pcurve.into() }
    pub(crate) fn original(self) -> Option<u32> { self.original }
    pub(crate) fn tolerance(self) -> f64 { self.tolerance }
}

#[derive(Clone, Serialize, Deserialize)]
struct StateWire {
    surface_xmt: u32,
    pcurve_xmt: u32,
    original_curve_xmt: u32,
    tolerance_to_original: f64,
}
impl From<SurfaceCurveState> for StateWire {
    fn from(state: SurfaceCurveState) -> Self {
        Self {
            surface_xmt: state.surface(), pcurve_xmt: state.pcurve(),
            original_curve_xmt: state.original().unwrap_or(1),
            tolerance_to_original: state.tolerance(),
        }
    }
}
impl TryFrom<StateWire> for SurfaceCurveState {
    type Error = &'static str;
    fn try_from(wire: StateWire) -> Result<Self, Self::Error> {
        Self::new(wire.surface_xmt, wire.pcurve_xmt, wire.original_curve_xmt, wire.tolerance_to_original)
    }
}

#[cfg(test)]
mod tests {
    use super::SurfaceCurveState;

    #[test]
    fn surface_curve_wire_preserves_absence_and_independent_tolerance() {
        for (original, expected) in [(1, None), (0, Some(0)), (9, Some(9))] {
            let json = format!(r#"{{"surface_xmt":6,"pcurve_xmt":9,"original_curve_xmt":{original},"tolerance_to_original":-0.0}}"#);
            let state: SurfaceCurveState = serde_json::from_str(&json).unwrap();
            assert_eq!(state.original(), expected);
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
            for field in ["surface_xmt", "pcurve_xmt"] {
                for invalid in [0, 1] {
                    let mut wire = serde_json::to_value(state).unwrap();
                    wire[field] = invalid.into();
                    assert!(serde_json::from_value::<SurfaceCurveState>(wire).unwrap_err().to_string().contains(field));
                }
            }
        }
        for tolerance in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(SurfaceCurveState::new(6, 9, 1, tolerance).unwrap_err().contains("tolerance_to_original"));
        }
        assert_eq!(SurfaceCurveState::new(6, 9, 1, -2.0).unwrap().tolerance(), -2.0);
    }
}
