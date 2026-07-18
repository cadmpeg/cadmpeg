// SPDX-License-Identifier: Apache-2.0
//! Literals used by the NX object-model decoder.

use crate::om::ExpressionUnit;

/// Root OM entity marker.
pub const ROOT_MARKER: &[u8] = b"\x04\x01\x0eNX ";
/// Section marker required before numeric expressions are decoded.
pub const HOST_GLOBALS: &[u8] = b"hostglobalvariables";
/// Registered class-definition name prefix.
pub const CLASS_NAME_PREFIX: &[u8] = b"UGS::";
/// Numeric-expression payload prefix.
pub const NUMBER_PREFIX: &[u8] = b"(Number [";

/// Resolve a numeric-expression unit token.
#[must_use]
pub fn unit_for(token: &str) -> Option<ExpressionUnit> {
    match token {
        "mm" => Some(ExpressionUnit::Millimeter),
        "degrees" => Some(ExpressionUnit::Degree),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_supported_units() {
        assert_eq!(unit_for("mm"), Some(ExpressionUnit::Millimeter));
        assert_eq!(unit_for("degrees"), Some(ExpressionUnit::Degree));
        assert_eq!(unit_for("furlongs"), None);
    }
}
