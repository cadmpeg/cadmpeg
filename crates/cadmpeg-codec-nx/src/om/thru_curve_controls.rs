// SPDX-License-Identifier: Apache-2.0
//! `THRU_CURVE` envelope controls with a fixed terminal marker.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "[u8; 9]", into = "[u8; 9]")]
pub(crate) struct ThruCurveControls(pub(crate) [u8; 8]);

impl TryFrom<[u8; 9]> for ThruCurveControls {
    type Error = &'static str;

    // Names follow the ordered source slots in this fixed-width lane.
    #[allow(clippy::many_single_char_names)]
    fn try_from(bytes: [u8; 9]) -> Result<Self, Self::Error> {
        let [a, b, c, d, e, f, g, h, 7] = bytes else {
            return Err("controls must end with marker 7");
        };
        Ok(Self([a, b, c, d, e, f, g, h]))
    }
}

impl From<ThruCurveControls> for [u8; 9] {
    // Names follow the ordered source slots in this fixed-width lane.
    #[allow(clippy::many_single_char_names)]
    fn from(value: ThruCurveControls) -> Self {
        let ThruCurveControls([a, b, c, d, e, f, g, h]) = value;
        [a, b, c, d, e, f, g, h, 7]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_keep_variable_bytes_and_derive_the_terminal_marker() {
        let json = "[2,3,3,4,1,1,1,1,7]";
        let controls: ThruCurveControls = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&controls).unwrap(), json);
        for invalid in ["[2,3,3,4,1,1,1,1,6]", "[2,3,3,4,1,1,1,7]"] {
            assert!(serde_json::from_str::<ThruCurveControls>(invalid).is_err());
        }
    }
}
