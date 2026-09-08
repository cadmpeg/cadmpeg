// SPDX-License-Identifier: Apache-2.0
//! Three-byte values in zero-prefixed control words.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct ControlWord24([u8; 3]);

impl ControlWord24 {
    pub(crate) fn new(bytes: [u8; 3]) -> Self {
        Self(bytes)
    }
    pub(crate) fn value(self) -> u32 {
        u32::from(self.0[0]) | (u32::from(self.0[1]) << 8) | (u32::from(self.0[2]) << 16)
    }
}

impl TryFrom<u32> for ControlWord24 {
    type Error = &'static str;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value > 0x00ff_ffff {
            return Err("value exceeds the unsigned 24-bit control-word range");
        }
        Ok(Self([value as u8, (value >> 8) as u8, (value >> 16) as u8]))
    }
}
impl From<ControlWord24> for u32 {
    fn from(value: ControlWord24) -> Self {
        value.value()
    }
}

#[cfg(test)]
mod tests {
    use super::ControlWord24;
    #[test]
    fn control_word_preserves_numeric_wire_and_rejects_overflow() {
        for value in [0, 0x1234, 0x00ff_ffff] {
            let word = ControlWord24::try_from(value).unwrap();
            let json = value.to_string();
            assert_eq!(serde_json::to_string(&word).unwrap(), json);
            assert_eq!(serde_json::from_str::<ControlWord24>(&json).unwrap(), word);
        }
        assert!(serde_json::from_str::<ControlWord24>("16777216")
            .unwrap_err()
            .to_string()
            .contains("value"));
    }
}
