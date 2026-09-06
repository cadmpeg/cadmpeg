// SPDX-License-Identifier: Apache-2.0
//! Non-null stream-local XMT identity.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct NonNullXmt(u32);

impl TryFrom<u32> for NonNullXmt {
    type Error = &'static str;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value <= 1 { return Err("reference: must exceed one"); }
        Ok(Self(value))
    }
}

impl From<NonNullXmt> for u32 {
    fn from(value: NonNullXmt) -> Self { value.0 }
}

#[cfg(test)]
mod tests {
    use super::NonNullXmt;

    #[test]
    fn non_null_xmt_preserves_integer_wire_and_rejects_sentinels() {
        for value in [2, 40_000, u32::MAX] {
            let json = value.to_string();
            let reference: NonNullXmt = serde_json::from_str(&json).unwrap();
            assert_eq!(u32::from(reference), value);
            assert_eq!(serde_json::to_string(&reference).unwrap(), json);
        }
        for json in ["0", "1"] {
            assert!(serde_json::from_str::<NonNullXmt>(json).unwrap_err().to_string().contains("reference"));
        }
    }
}
