// SPDX-License-Identifier: Apache-2.0
//! Complete source-byte ownership accounting.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Classification of one owned source span.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ByteSpanClass {
    /// Bytes whose format semantics were decoded.
    Typed,
    /// Framing, delimiters, padding, and other structural bytes.
    Structural,
    /// Bytes retained as part of a named native or unknown record.
    Opaque,
}

/// One nonempty half-open source-byte span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ByteSpan {
    /// Inclusive zero-based byte offset.
    pub start: u64,
    /// Exclusive zero-based byte offset.
    pub end: u64,
    /// Ownership classification.
    pub class: ByteSpanClass,
    /// Stable format-owned record identity.
    pub owner: String,
    /// Stable machine-readable field or framing name.
    pub meaning: String,
    /// Native or unknown record identity retaining an opaque span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_record: Option<String>,
}

/// Complete ownership of one source stream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ByteLedger {
    /// Source length in bytes.
    pub source_length: u64,
    /// Nonoverlapping spans in ascending source order.
    pub spans: Vec<ByteSpan>,
}

impl ByteLedger {
    /// Sort spans by source position and coalesce equivalent adjacent ownership.
    pub fn finalize(&mut self) {
        self.spans.sort_by(|left, right| {
            (
                left.start,
                left.end,
                left.class,
                &left.owner,
                &left.meaning,
                &left.retained_record,
            )
                .cmp(&(
                    right.start,
                    right.end,
                    right.class,
                    &right.owner,
                    &right.meaning,
                    &right.retained_record,
                ))
        });
        self.spans.dedup_by(|right, left| {
            if left.end == right.start
                && left.class == right.class
                && left.owner == right.owner
                && left.meaning == right.meaning
                && left.retained_record == right.retained_record
            {
                left.end = right.end;
                true
            } else {
                false
            }
        });
    }
}
