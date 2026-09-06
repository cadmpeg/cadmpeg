// SPDX-License-Identifier: Apache-2.0
//! Complete blend-bound bridge payload and its integer wire fields.

use serde::{Deserialize, Serialize};
use crate::deltas::xmt_reference::NonNullXmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Boundary { First, Second }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub(crate) struct BlendBoundState {
    xmt: NonNullXmt,
    header: [u32; 4],
    sense: bool,
    boundary: Boundary,
    surface: NonNullXmt,
}

impl BlendBoundState {
    pub(crate) fn new(xmt: u32, header: [u32; 5], sense: bool, boundary: u32, surface: u32) -> Result<Self, &'static str> {
        let [first, a, b, c, d] = header;
        if first != 1 { return Err("header_references: first reference must be one"); }
        let boundary = match boundary {
            0 => Boundary::First,
            1 => Boundary::Second,
            _ => return Err("boundary_index: must be zero or one"),
        };
        Ok(Self {
            xmt: xmt.try_into()?, header: [a, b, c, d], sense, boundary,
            surface: surface.try_into().map_err(|_| "blend_surface_xmt: must exceed one")?,
        })
    }
    pub(crate) fn xmt(self) -> u32 { self.xmt.into() }
    pub(crate) fn header_references(self) -> [u32; 5] {
        let [a, b, c, d] = self.header;
        [1, a, b, c, d]
    }
    pub(crate) fn sense(self) -> bool { self.sense }
    pub(crate) fn boundary_index(self) -> u32 {
        match self.boundary { Boundary::First => 0, Boundary::Second => 1 }
    }
    pub(crate) fn blend_surface(self) -> u32 { self.surface.into() }
}

#[derive(Clone, Serialize, Deserialize)]
struct StateWire {
    xmt: u32,
    header_references: [u32; 5],
    sense: bool,
    boundary_index: u32,
    blend_surface_xmt: u32,
}
impl From<BlendBoundState> for StateWire {
    fn from(state: BlendBoundState) -> Self {
        Self { xmt: state.xmt(), header_references: state.header_references(), sense: state.sense(), boundary_index: state.boundary_index(), blend_surface_xmt: state.blend_surface() }
    }
}
impl TryFrom<StateWire> for BlendBoundState {
    type Error = &'static str;
    fn try_from(wire: StateWire) -> Result<Self, Self::Error> {
        Self::new(wire.xmt, wire.header_references, wire.sense, wire.boundary_index, wire.blend_surface_xmt)
    }
}

#[cfg(test)]
mod tests {
    use super::BlendBoundState;

    #[test]
    fn bridge_wire_preserves_both_boundaries_and_rejects_invalid_controls() {
        for boundary in [0, 1] {
            let json = format!(r#"{{"xmt":2,"header_references":[1,0,3,4,5],"sense":false,"boundary_index":{boundary},"blend_surface_xmt":6}}"#);
            let state: BlendBoundState = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
            for (field, invalid) in [
                ("xmt", serde_json::json!(0)), ("xmt", serde_json::json!(1)),
                ("header_references", serde_json::json!([0,0,3,4,5])),
                ("boundary_index", serde_json::json!(2)),
                ("blend_surface_xmt", serde_json::json!(0)), ("blend_surface_xmt", serde_json::json!(1)),
            ] {
                let mut wire = serde_json::to_value(state).unwrap();
                wire[field] = invalid;
                let error = serde_json::from_value::<BlendBoundState>(wire).unwrap_err();
                assert!(error.to_string().contains(field), "{error}");
            }
        }
    }
}
