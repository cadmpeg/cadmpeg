// SPDX-License-Identifier: Apache-2.0
//! Closed marker bytes for deltas state packets.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum ReferenceMarker {
    Form53,
    Form56,
}
impl TryFrom<u8> for ReferenceMarker {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            83 => Ok(Self::Form53),
            86 => Ok(Self::Form56),
            _ => Err("marker: must be 83 or 86"),
        }
    }
}
impl From<ReferenceMarker> for u8 {
    fn from(value: ReferenceMarker) -> Self {
        match value {
            ReferenceMarker::Form53 => 83,
            ReferenceMarker::Form56 => 86,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum Type150Marker {
    Form2b,
    Form2d,
}
impl TryFrom<u8> for Type150Marker {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            43 => Ok(Self::Form2b),
            45 => Ok(Self::Form2d),
            _ => Err("marker: must be 43 or 45"),
        }
    }
}
impl From<Type150Marker> for u8 {
    fn from(value: Type150Marker) -> Self {
        match value {
            Type150Marker::Form2b => 43,
            Type150Marker::Form2d => 45,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ReferenceMarker, Type150Marker};

    #[test]
    fn packet_markers_preserve_bytes_and_reject_other_values() {
        for byte in 0..=u8::MAX {
            let text = byte.to_string();
            let reference = serde_json::from_str::<ReferenceMarker>(&text);
            assert_eq!(reference.is_ok(), matches!(byte, 0x53 | 0x56));
            if let Ok(value) = reference {
                assert_eq!(serde_json::to_string(&value).unwrap(), text);
            }
            let state = serde_json::from_str::<Type150Marker>(&text);
            assert_eq!(state.is_ok(), matches!(byte, 0x2b | 0x2d));
            if let Ok(value) = state {
                assert_eq!(serde_json::to_string(&value).unwrap(), text);
            }
        }
    }
}
