// SPDX-License-Identifier: Apache-2.0
//! Shared source and native inline-schema payloads.

use serde::{Deserialize, Serialize};

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
        /// Non-null stream-local declaration identity.
        xmt: u32,
        /// Ordered precision-state references.
        references: [u32; 3],
        /// Serialized affine state.
        transform: [f64; 13],
    },
    /// Type 101 declaration and its schema-bound instance state.
    Type101 {
        /// Four ordered stream-local XMT references.
        references: [u32; 4],
        /// Optional non-null reference following the zero sentinel.
        anchor_reference: Option<u32>,
        /// Three serialized big-endian state words.
        state_words: [u32; 3],
        /// Terminal unsigned 40-bit state value.
        terminal_value: u64,
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
        numeric_values: Option<[f64; 11]>,
    },
    /// Type 41 term-use declaration state.
    Type41 {
        /// Non-null stream-local term-use reference.
        reference: u32,
        /// Eleven finite binary64 state values.
        numeric_values: [f64; 11],
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

