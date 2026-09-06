// SPDX-License-Identifier: Apache-2.0
//! Type-100 identity, derived references, and translation-only state.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PrecisionWire", into = "PrecisionWire")]
pub(crate) struct PrecisionState {
    xmt: u32,
    translation: [f64; 3],
}
impl PrecisionState {
    pub(crate) fn new(
        xmt: u32,
        references: [u32; 3],
        transform: [f64; 13],
    ) -> Result<Self, &'static str> {
        let successor = xmt
            .checked_add(1)
            .filter(|_| xmt > 1)
            .ok_or("xmt: require a non-null identity with a successor")?;
        if references != [2, successor, 1] {
            return Err("references: require [2, xmt + 1, 1]");
        }
        for (ordinal, value) in transform.iter().enumerate() {
            let valid = match ordinal {
                0 | 4 | 8 | 12 => value.to_bits() == 1.0_f64.to_bits(),
                9..=11 => value.is_finite(),
                _ => value.to_bits() == 0.0_f64.to_bits(),
            };
            if !valid {
                return Err(
                    "transform: require a finite translation with identity rotation and unit scale",
                );
            }
        }
        Ok(Self {
            xmt,
            translation: [transform[9], transform[10], transform[11]],
        })
    }
    #[cfg(test)]
    pub(crate) fn xmt(&self) -> u32 {
        self.xmt
    }
    pub(crate) fn references(&self) -> [u32; 3] {
        [2, self.xmt + 1, 1]
    }
    pub(crate) fn transform(&self) -> [f64; 13] {
        let [x, y, z] = self.translation;
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, x, y, z, 1.0]
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct PrecisionWire {
    xmt: u32,
    references: [u32; 3],
    transform: [f64; 13],
}
impl From<PrecisionState> for PrecisionWire {
    fn from(state: PrecisionState) -> Self {
        Self {
            xmt: state.xmt,
            references: state.references(),
            transform: state.transform(),
        }
    }
}
impl TryFrom<PrecisionWire> for PrecisionState {
    type Error = &'static str;
    fn try_from(wire: PrecisionWire) -> Result<Self, Self::Error> {
        Self::new(wire.xmt, wire.references, wire.transform)
    }
}

#[cfg(test)]
mod tests {
    use super::PrecisionState;

    #[test]
    fn precision_wire_preserves_translation_and_rejects_nonidentity_state() {
        let json = r#"{"xmt":53,"references":[2,54,1],"transform":[1.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,1.0,-0.0,-0.0,1.25,1.0]}"#;
        let state: PrecisionState = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&state).unwrap(), json);
        for (index, value) in [
            (0, 2.0),
            (1, -0.0),
            (9, f64::NAN),
            (10, f64::INFINITY),
            (12, 0.0),
        ] {
            let mut transform = state.transform();
            transform[index] = value;
            assert!(PrecisionState::new(53, [2, 54, 1], transform)
                .unwrap_err()
                .contains("transform"));
        }
        assert!(PrecisionState::new(53, [2, 55, 1], state.transform())
            .unwrap_err()
            .contains("references"));
        assert!(PrecisionState::new(u32::MAX, [2, 0, 1], state.transform())
            .unwrap_err()
            .contains("xmt"));
    }
}
