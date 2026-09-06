// SPDX-License-Identifier: Apache-2.0
//! Non-null stream-local XMT identity.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct NonNullXmt(u32);

impl TryFrom<u32> for NonNullXmt {
    type Error = &'static str;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value <= 1 { return Err("xmt reference: must exceed one"); }
        Ok(Self(value))
    }
}

impl From<NonNullXmt> for u32 {
    fn from(value: NonNullXmt) -> Self { value.0 }
}

/// A retained reference target other than the null token. Zero remains
/// representable for unresolved references; record identities require NonNullXmt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct XmtTarget(u32);

impl XmtTarget {
    pub(crate) fn from_wire(value: u32) -> Option<Self> {
        if value == 1 { None } else { Some(Self(value)) }
    }

    pub(crate) fn to_wire(value: Option<Self>) -> u32 {
        value.map_or(1, |target| target.0)
    }
}

impl From<XmtTarget> for u32 {
    fn from(target: XmtTarget) -> Self { target.0 }
}

#[cfg(test)]
mod tests {
    use super::{NonNullXmt, XmtTarget};

    #[test]
    fn optional_targets_preserve_zero_and_reserve_only_the_null_token() {
        for value in [0, 1, 2, u32::MAX] {
            let target = XmtTarget::from_wire(value);
            assert_eq!(target.is_none(), value == 1);
            assert_eq!(XmtTarget::to_wire(target), value);
        }
    }

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
