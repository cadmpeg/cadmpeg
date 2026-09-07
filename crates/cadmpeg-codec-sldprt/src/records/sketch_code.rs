// SPDX-License-Identifier: Apache-2.0
//! Disjoint low marker codes and native extension codes.

use serde::{Deserialize, Serialize};

/// Low codes whose meaning is supplied by a geometry or handle layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LowMarkerCode {
    Zero,
    One,
    Two,
    Three,
}

impl LowMarkerCode {
    pub(crate) fn value(self) -> u32 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
        }
    }
}

impl std::fmt::Display for LowMarkerCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "native sketch code {} is reserved for a geometry or handle layout",
            self.value()
        )
    }
}

impl std::error::Error for LowMarkerCode {}

/// A native extension code outside the four low geometry codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct NativeSketchCode(u32);

impl NativeSketchCode {
    pub(crate) fn value(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for NativeSketchCode {
    type Error = LowMarkerCode;

    fn try_from(code: u32) -> Result<Self, Self::Error> {
        match code {
            0 => Err(LowMarkerCode::Zero),
            1 => Err(LowMarkerCode::One),
            2 => Err(LowMarkerCode::Two),
            3 => Err(LowMarkerCode::Three),
            code => Ok(Self(code)),
        }
    }
}

impl From<NativeSketchCode> for u32 {
    fn from(code: NativeSketchCode) -> Self {
        code.value()
    }
}

#[cfg(test)]
mod tests {
    use super::NativeSketchCode;

    #[test]
    fn extension_codes_exclude_low_layout_codes() {
        for code in 0..=3 {
            assert!(NativeSketchCode::try_from(code).is_err());
            assert!(serde_json::from_value::<NativeSketchCode>(serde_json::json!(code)).is_err());
        }
        for code in [4, 85, 86, u32::MAX] {
            let value = NativeSketchCode::try_from(code).unwrap();
            assert_eq!(value.value(), code);
            assert_eq!(
                serde_json::to_value(value).unwrap(),
                serde_json::json!(code)
            );
        }
    }
}
