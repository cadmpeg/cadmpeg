// SPDX-License-Identifier: Apache-2.0
//! Non-null trim basis and finite endpoint payloads.

use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Endpoint {
    point: FinitePoint3,
    parameter: FiniteReal,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub(crate) struct TrimmedCurveState {
    basis: NonNullXmt,
    endpoints: [Endpoint; 2],
}
impl TrimmedCurveState {
    fn new(basis: u32, points: [[f64; 3]; 2], parameters: [f64; 2]) -> Result<Self, &'static str> {
        let [Some(first), Some(second)] =
            points.map(|point| FinitePoint3::new(Point3::from(point)))
        else {
            return Err("points: trim coordinates must be finite");
        };
        let [Some(start), Some(end)] = parameters.map(FiniteReal::new) else {
            return Err("parameters: trim parameters must be finite");
        };
        Ok(Self {
            basis: basis.try_into().map_err(|_| "basis_xmt: must exceed one")?,
            endpoints: [
                Endpoint {
                    point: first,
                    parameter: start,
                },
                Endpoint {
                    point: second,
                    parameter: end,
                },
            ],
        })
    }
    pub(super) fn from_metres(
        basis: u32,
        points: [[f64; 3]; 2],
        parameters: [f64; 2],
    ) -> Result<Self, &'static str> {
        Self::new(
            basis,
            points.map(|point| point.map(|value| value * 1000.0)),
            parameters,
        )
    }
    pub(crate) fn basis(self) -> u32 {
        self.basis.into()
    }
    pub(crate) fn points(self) -> [[f64; 3]; 2] {
        self.endpoints.map(|endpoint| endpoint.point.get().into())
    }
    pub(crate) fn parameters(self) -> [f64; 2] {
        self.endpoints.map(|endpoint| endpoint.parameter.get())
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct StateWire {
    basis_xmt: u32,
    points: [[f64; 3]; 2],
    parameters: [f64; 2],
}
impl From<TrimmedCurveState> for StateWire {
    fn from(state: TrimmedCurveState) -> Self {
        Self {
            basis_xmt: state.basis(),
            points: state.points(),
            parameters: state.parameters(),
        }
    }
}
impl TryFrom<StateWire> for TrimmedCurveState {
    type Error = &'static str;
    fn try_from(wire: StateWire) -> Result<Self, Self::Error> {
        Self::new(wire.basis_xmt, wire.points, wire.parameters)
    }
}

#[cfg(test)]
mod tests {
    use super::TrimmedCurveState;

    #[test]
    fn trim_wire_preserves_endpoint_order_and_signed_zero() {
        let json =
            r#"{"basis_xmt":9,"points":[[-0.0,1.0,2.0],[3.0,4.0,5.0]],"parameters":[7.0,-2.0]}"#;
        let state: TrimmedCurveState = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&state).unwrap(), json);
        for basis in [0, 1] {
            let mut wire = serde_json::to_value(state).unwrap();
            wire["basis_xmt"] = basis.into();
            assert!(serde_json::from_value::<TrimmedCurveState>(wire)
                .unwrap_err()
                .to_string()
                .contains("basis_xmt"));
        }
    }

    #[test]
    fn trim_construction_rejects_nonfinite_payloads_and_unit_overflow() {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(TrimmedCurveState::new(9, [[invalid; 3]; 2], [0.0, 1.0])
                .unwrap_err()
                .contains("points"));
            assert!(TrimmedCurveState::new(9, [[0.0; 3]; 2], [0.0, invalid])
                .unwrap_err()
                .contains("parameters"));
        }
        assert!(
            TrimmedCurveState::from_metres(9, [[f64::MAX; 3]; 2], [0.0, 1.0])
                .unwrap_err()
                .contains("points")
        );
        let state = TrimmedCurveState::from_metres(9, [[1.0, -0.0, -2.0]; 2], [3.0, -4.0]).unwrap();
        assert_eq!(state.points(), [[1000.0, -0.0, -2000.0]; 2]);
        assert_eq!(state.points()[0][1].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(state.parameters(), [3.0, -4.0]);
    }
}
