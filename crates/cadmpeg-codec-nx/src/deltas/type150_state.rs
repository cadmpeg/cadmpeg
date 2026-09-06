// SPDX-License-Identifier: Apache-2.0
//! Complete type-150 state payload.

use serde::{Deserialize, Serialize};

use super::packet_marker::Type150Marker;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub(crate) struct Type150State {
    references: [u32; 4],
    pub(crate) marker: Type150Marker,
    values: [f64; 9],
}

impl Type150State {
    pub(crate) fn new(references: [u32; 5], marker: Type150Marker, values: [f64; 9]) -> Result<Self, &'static str> {
        let [null, a, b, c, d] = references;
        if null != 1 || [a, b, c, d].iter().any(|reference| *reference <= 1) {
            return Err("references: require one null followed by four non-null references");
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Err("values: require nine finite state values");
        }
        Ok(Self { references: [a, b, c, d], marker, values })
    }

    pub(crate) fn references(&self) -> [u32; 5] {
        let [a, b, c, d] = self.references;
        [1, a, b, c, d]
    }

    pub(crate) fn values(&self) -> &[f64; 9] { &self.values }
}

#[derive(Clone, Serialize, Deserialize)]
struct StateWire {
    references: [u32; 5],
    marker: Type150Marker,
    values: [f64; 9],
}

impl From<Type150State> for StateWire {
    fn from(value: Type150State) -> Self {
        Self { references: value.references(), marker: value.marker, values: *value.values() }
    }
}

impl TryFrom<StateWire> for Type150State {
    type Error = &'static str;
    fn try_from(wire: StateWire) -> Result<Self, Self::Error> {
        Self::new(wire.references, wire.marker, wire.values)
    }
}

#[cfg(test)]
mod tests {
    use super::{Type150Marker, Type150State};

    #[test]
    fn state_wire_preserves_fields_and_rejects_null_references() {
        let json = r#"{"references":[1,3,6192,6193,6194],"marker":43,"values":[-0.025,-0.05,0.25,0.0,1.0,0.0,0.0,-0.0,1.0]}"#;
        let state: Type150State = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&state).unwrap(), json);
        for references in [[2, 3, 4, 5, 6], [1, 0, 4, 5, 6], [1, 3, 4, 1, 6]] {
            let mut wire = serde_json::to_value(&state).unwrap();
            wire["references"] = serde_json::to_value(references).unwrap();
            assert!(serde_json::from_value::<Type150State>(wire).unwrap_err().to_string().contains("references"));
        }
        for nonfinite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(Type150State::new([1, 3, 4, 5, 6], Type150Marker::Form2b, [nonfinite; 9]).unwrap_err().contains("values"));
        }
    }
}
