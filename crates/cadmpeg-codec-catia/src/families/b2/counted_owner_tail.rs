// SPDX-License-Identifier: Apache-2.0
//! Nonempty class-0x62 counted-owner tail bytes.

use cadmpeg_ir::native::bytes::NativeBytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CountedOwnerTail(NativeBytes);

impl CountedOwnerTail {
    pub(crate) fn new(bytes: Vec<u8>) -> Option<Self> {
        (!bytes.is_empty()).then_some(Self(bytes.into()))
    }
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl serde::Serialize for CountedOwnerTail {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for CountedOwnerTail {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(NativeBytes::<Vec<u8>>::deserialize(deserializer)?.into_inner())
            .ok_or_else(|| serde::de::Error::custom("counted-owner tail is empty"))
    }
}
