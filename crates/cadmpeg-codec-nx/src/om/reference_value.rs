// SPDX-License-Identifier: Apache-2.0
//! Payloads of tagged OM references.

use serde::{Deserialize, Serialize};

/// The low 28 bits of a `c`-tagged word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct Tagged28(u32);

impl Tagged28 {
    pub(crate) fn from_word(word: u32) -> Self {
        Self(word & 0x0fff_ffff)
    }
}

impl TryFrom<u32> for Tagged28 {
    type Error = &'static str;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value <= 0x0fff_ffff {
            Ok(Self(value))
        } else {
            Err("value exceeds the tagged28 range")
        }
    }
}

impl From<Tagged28> for u32 {
    fn from(value: Tagged28) -> Self {
        value.0
    }
}

/// A reference that identifies its family without a count frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(crate) enum DirectReference {
    PersistentHandle(u32),
    Tagged28(Tagged28),
}

/// A bounded record reference and its same-section resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordReference<T> {
    Direct(DirectReference),
    RecordOrdinal16 { ordinal: u16, target: T },
}

/// A reference marker at its absolute source offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocatedReference<T> {
    pub(crate) offset: usize,
    pub(crate) value: T,
}
