// SPDX-License-Identifier: Apache-2.0
//! Decode-local byte storage whose document form contains evidence only.

use serde::{Serialize, Serializer};
use std::ops::{Deref, DerefMut};

/// Source bytes used during decoding. The sidecar retains their source image;
/// native records serialize only the length and digest of each source extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceBytes<T = Vec<u8>> {
    data: T,
    untyped: u8,
}
impl<T: Default> Default for SourceBytes<T> {
    fn default() -> Self {
        Self {
            data: T::default(),
            untyped: 0,
        }
    }
}
impl<T> SourceBytes<T> {
    pub(crate) fn with_untyped(mut self, class: cadmpeg_ir::ArenaClass) -> Self {
        self.untyped |= match class {
            cadmpeg_ir::ArenaClass::Model => 1,
            cadmpeg_ir::ArenaClass::Design => 2,
        };
        self
    }
}

impl<T> From<T> for SourceBytes<T> {
    fn from(bytes: T) -> Self {
        Self {
            data: bytes,
            untyped: 0,
        }
    }
}
impl<'a> From<&'a Vec<u8>> for SourceBytes<&'a [u8]> {
    fn from(bytes: &'a Vec<u8>) -> Self {
        Self::from(bytes.as_slice())
    }
}
impl<'a, const N: usize> From<&'a [u8; N]> for SourceBytes<&'a [u8]> {
    fn from(bytes: &'a [u8; N]) -> Self {
        Self::from(bytes.as_slice())
    }
}
impl<T: AsRef<[u8]>, const N: usize> PartialEq<[u8; N]> for SourceBytes<T> {
    fn eq(&self, other: &[u8; N]) -> bool {
        self.as_ref() == other
    }
}
impl<T: AsRef<[u8]>> PartialEq<Vec<u8>> for SourceBytes<T> {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.as_ref() == other.as_slice()
    }
}
impl<'a, T: AsRef<[u8]>> From<&'a SourceBytes<T>> for SourceBytes<&'a [u8]> {
    fn from(bytes: &'a SourceBytes<T>) -> Self {
        Self {
            data: bytes.as_ref(),
            untyped: bytes.untyped,
        }
    }
}
impl<T: AsRef<[u8]>> AsRef<[u8]> for SourceBytes<T> {
    fn as_ref(&self) -> &[u8] {
        self.data.as_ref()
    }
}
impl<T> Deref for SourceBytes<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.data
    }
}
impl<T> DerefMut for SourceBytes<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}
impl<T: AsRef<[u8]>> Serialize for SourceBytes<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut record = serializer.serialize_struct(
            "SourceRecordEvidence",
            if self.untyped == 0 { 2 } else { 3 },
        )?;
        record.serialize_field(
            "byte_len",
            &cadmpeg_core::decode::u64_from_index(self.as_ref().len()),
        )?;
        record.serialize_field(
            "sha256",
            &cadmpeg_ir::hash::digest::Sha256Digest::digest(self.as_ref()),
        )?;
        if self.untyped != 0 {
            let mut classes = cadmpeg_ir::ArenaClasses::default();
            if self.untyped & 1 != 0 {
                classes.insert(cadmpeg_ir::ArenaClass::Model);
            }
            if self.untyped & 2 != 0 {
                classes.insert(cadmpeg_ir::ArenaClass::Design);
            }
            record.serialize_field("source_untyped", &classes)?;
        }
        record.end()
    }
}
