// SPDX-License-Identifier: Apache-2.0
//! Shared source and native inline-schema payloads.

use serde::{Deserialize, Serialize};
use super::xmt_reference::NonNullXmt;
use super::precision_state::PrecisionState;
use super::type101_state::Type101State;
use super::attdef_state::AttdefState;
use super::type70_state::Type70State;
use super::type38_state::Type38State;

/// Body of an inline schema declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "schema", rename_all = "snake_case")]
pub(crate) enum InlineSchemaFields {
    /// Type 12 `BODY` schema header without following instance state.
    BodyHeader,
    /// REGION declaration state.
    Region {
        /// Non-null stream-local declaration identity.
        xmt: NonNullXmt,
        /// Serialized big-endian state word.
        state_word: u32,
        /// Four ordered stream-local XMT references.
        references: [u32; 4],
    },
    /// `ATTDEF_LIST` declaration state.
    AttdefList {
        #[serde(flatten)]
        state: AttdefState,
    },
    /// Type 70 declaration state.
    Type70 {
        #[serde(flatten)]
        state: Type70State,
    },
    /// Type 100 declaration and its precision state.
    Type100 {
        #[serde(flatten)]
        state: PrecisionState,
    },
    /// Type 101 declaration and its schema-bound instance state.
    Type101 {
        #[serde(flatten)]
        state: Type101State,
    },
    /// Type 101 declaration with the compact fixed state.
    Type101Compact,
    /// Type 38 intersection-data declaration state.
    Type38 {
        #[serde(flatten)]
        state: Type38State,
    },
    /// Type 41 term-use declaration state.
    Type41 {
        /// Non-null stream-local term-use reference.
        reference: NonNullXmt,
        /// Eleven finite binary64 state values.
        numeric_values: TermUseValues,
    },
}

/// Schema-bound type-12 `BODY` instance state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum InlineBodyStateFields {
    /// Compact reference form followed by status zero.
    Compact {
        /// Non-null stream-local XMT reference.
        reference: NonNullXmt,
    },
    /// Revision form with a bounded opaque state tail.
    Revision {
        /// Monotonic kernel revision identity.
        node_id: u32,
        /// Eight ordered status-framed XMT references.
        references: [u32; 8],
        /// Exact state bytes following the reference prefix.
        state_bytes: BodyStateBytes,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 11]", into = "[f64; 11]")]
pub(crate) struct TermUseValues([f64; 11]);

impl TryFrom<[f64; 11]> for TermUseValues {
    type Error = &'static str;
    fn try_from(values: [f64; 11]) -> Result<Self, Self::Error> {
        if values.iter().any(|value| !value.is_finite()) {
            return Err("numeric_values: require eleven finite values");
        }
        Ok(Self(values))
    }
}
impl From<TermUseValues> for [f64; 11] {
    fn from(values: TermUseValues) -> Self { values.0 }
}

#[cfg(test)]
mod tests {
    use super::{InlineSchemaFields, TermUseValues};

    #[test]
    fn term_use_wire_preserves_values_and_rejects_nonfinite_construction() {
        let json = r#"{"schema":"type41","reference":86,"numeric_values":[0.5,-0.25,1.0,2.0,3.0,4.0,5.0,6.0,7.0,-0.0,9.0]}"#;
        let fields: InlineSchemaFields = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&fields).unwrap(), json);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(TermUseValues::try_from([value; 11]).unwrap_err().contains("numeric_values"));
        }
    }
}

/// Nonempty opaque revision state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<u8>", into = "Vec<u8>")]
pub(crate) struct BodyStateBytes(Vec<u8>);

impl TryFrom<Vec<u8>> for BodyStateBytes {
    type Error = &'static str;
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        if bytes.is_empty() { return Err("state_bytes: require a nonempty revision state"); }
        Ok(Self(bytes))
    }
}
impl From<BodyStateBytes> for Vec<u8> {
    fn from(bytes: BodyStateBytes) -> Self { bytes.0 }
}

#[cfg(test)]
mod body_state_tests {
    use super::InlineBodyStateFields;
    #[test]
    fn body_wire_rejects_null_compact_references_and_empty_revisions() {
        for json in [
            r#"{"form":"compact","reference":9}"#,
            r#"{"form":"revision","node_id":7,"references":[8,1,2,3,4,5,6,7],"state_bytes":[170,187]}"#,
        ] {
            let state: InlineBodyStateFields = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
        }
        for reference in [0, 1] {
            assert!(serde_json::from_value::<InlineBodyStateFields>(serde_json::json!({"form":"compact","reference":reference})).is_err());
        }
        let error = serde_json::from_str::<InlineBodyStateFields>(r#"{"form":"revision","node_id":7,"references":[8,1,2,3,4,5,6,7],"state_bytes":[]}"#).unwrap_err();
        assert!(error.to_string().contains("state_bytes"));
    }
}
