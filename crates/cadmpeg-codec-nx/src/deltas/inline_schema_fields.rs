// SPDX-License-Identifier: Apache-2.0
//! Shared source and native inline-schema payloads.

use serde::{Deserialize, Serialize};
use super::xmt_reference::NonNullXmt;
use super::precision_state::PrecisionState;
use super::type101_state::Type101State;

/// Body of an inline schema declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "schema", rename_all = "snake_case")]
pub(crate) enum InlineSchemaFields {
    /// Type 12 `BODY` schema header without following instance state.
    BodyHeader,
    /// REGION declaration state.
    Region {
        /// Non-null stream-local declaration identity.
        xmt: u32,
        /// Serialized big-endian state word.
        state_word: u32,
        /// Four ordered stream-local XMT references.
        references: [u32; 4],
    },
    /// `ATTDEF_LIST` declaration state.
    AttdefList {
        /// Non-null stream-local declaration identity.
        xmt: u32,
        /// Number of serialized reference slots.
        slot_count: u32,
        /// Number of leading non-null slots.
        active_count: u32,
        /// Slot references, excluding the null sentinel.
        references: Vec<u32>,
    },
    /// Type 70 declaration state.
    Type70 {
        /// Non-null stream-local declaration identity.
        xmt: u32,
        /// Serialized node identity.
        node_id: u32,
        /// Four ordered body references.
        references: [u32; 4],
        /// Serialized declaration count.
        count: u16,
        /// Repeated terminal non-null reference.
        trailing_reference: u32,
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
        /// Non-null stream-local declaration identity.
        xmt: u32,
        /// Serialized node identity.
        node_id: u32,
        /// Five leading XMT references.
        leading_references: [u32; 5],
        /// Serialized statuses of the five leading references.
        #[serde(
            default = "default_type38_leading_statuses",
            deserialize_with = "deserialize_type38_leading_statuses",
            skip_serializing_if = "type38_leading_statuses_are_default"
        )]
        leading_statuses: [u8; 5],
        /// Intersection-state discriminator.
        marker: u8,
        /// Non-null status-one XMT references selected by the marker.
        linked_references: Vec<u32>,
        /// Non-null status-zero declaration-state references.
        state_references: Vec<u32>,
        /// Eleven finite binary64 values from the optional nested term-use state.
        numeric_values: Option<TermUseValues>,
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
        reference: u32,
    },
    /// Revision form with a bounded opaque state tail.
    Revision {
        /// Monotonic kernel revision identity.
        node_id: u32,
        /// Eight ordered status-framed XMT references.
        references: [u32; 8],
        /// Exact state bytes following the reference prefix.
        state_bytes: Vec<u8>,
    },
}

fn default_type38_leading_statuses() -> [u8; 5] {
    [1; 5]
}

fn type38_leading_statuses_are_default(statuses: &[u8; 5]) -> bool {
    *statuses == default_type38_leading_statuses()
}

fn deserialize_type38_leading_statuses<'de, D>(deserializer: D) -> Result<[u8; 5], D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<[u8; 5]>::deserialize(deserializer)?
        .unwrap_or_else(default_type38_leading_statuses))
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
