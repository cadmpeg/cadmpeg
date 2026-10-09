// SPDX-License-Identifier: Apache-2.0
//! `THRU_CURVE` envelope controls with a fixed terminal marker.

use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NativeBytes<[u8; 9]>", into = "NativeBytes<[u8; 9]>")]
pub(crate) struct ThruCurveControls(pub(crate) [u8; 8]);

impl TryFrom<[u8; 9]> for ThruCurveControls {
    type Error = &'static str;

    fn try_from(bytes: [u8; 9]) -> Result<Self, Self::Error> {
        let [first, second, third, fourth, fifth, sixth, seventh, eighth, 7] = bytes else {
            return Err("controls must end with marker 7");
        };
        Ok(Self([
            first, second, third, fourth, fifth, sixth, seventh, eighth,
        ]))
    }
}

impl From<ThruCurveControls> for [u8; 9] {
    fn from(value: ThruCurveControls) -> Self {
        let ThruCurveControls([first, second, third, fourth, fifth, sixth, seventh, eighth]) =
            value;
        [
            first, second, third, fourth, fifth, sixth, seventh, eighth, 7,
        ]
    }
}

impl TryFrom<NativeBytes<[u8; 9]>> for ThruCurveControls {
    type Error = &'static str;
    fn try_from(bytes: NativeBytes<[u8; 9]>) -> Result<Self, Self::Error> {
        Self::try_from(bytes.into_inner())
    }
}

impl From<ThruCurveControls> for NativeBytes<[u8; 9]> {
    fn from(value: ThruCurveControls) -> Self {
        Self::from(<[u8; 9]>::from(value))
    }
}

#[cfg(test)]
mod tests {
    use super::ThruCurveControls;

    #[test]
    fn controls_keep_variable_bytes_and_derive_the_terminal_marker() {
        let json = "\"020303040101010107\"";
        let controls: ThruCurveControls = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&controls).unwrap(), json);
        for invalid in ["\"020303040101010106\"", "\"0203030401010107\""] {
            assert!(serde_json::from_str::<ThruCurveControls>(invalid).is_err());
        }
    }
}
