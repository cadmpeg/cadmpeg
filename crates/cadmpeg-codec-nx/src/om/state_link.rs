// SPDX-License-Identifier: Apache-2.0
//! Status-link discriminator bytes outside the reserved payload markers.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) struct StateLinkCode(u8);

impl TryFrom<u8> for StateLinkCode {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if matches!(value, 0x02 | 0x03 | 0x1e | 0x3f | 0xff) {
            return Err("link_code: reserved for another status payload form");
        }
        Ok(Self(value))
    }
}

impl From<StateLinkCode> for u8 {
    fn from(value: StateLinkCode) -> Self { value.0 }
}

#[cfg(test)]
mod tests {
    use super::StateLinkCode;

    #[test]
    fn link_codes_preserve_byte_values_and_reject_reserved_markers() {
        for code in [0, 1, 0x45, 0xfe] {
            let json = code.to_string();
            let value: StateLinkCode = serde_json::from_str(&json).unwrap();
            assert_eq!(u8::from(value), code);
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
        }
        for code in [0x02, 0x03, 0x1e, 0x3f, 0xff] {
            assert!(serde_json::from_str::<StateLinkCode>(&code.to_string())
                .unwrap_err().to_string().contains("link_code"));
        }
    }
}
