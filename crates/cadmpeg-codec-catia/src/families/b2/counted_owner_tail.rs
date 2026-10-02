//! Nonempty class-0x62 counted-owner tail bytes.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CountedOwnerTail(Vec<u8>);

impl CountedOwnerTail {
    pub(crate) fn new(bytes: Vec<u8>) -> Option<Self> {
        (!bytes.is_empty()).then_some(Self(bytes))
    }
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl serde::Serialize for CountedOwnerTail {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        cadmpeg_ir::bytes::serialize(self.as_slice(), serializer)
    }
}

impl<'de> serde::Deserialize<'de> for CountedOwnerTail {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(cadmpeg_ir::bytes::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("counted-owner tail is empty"))
    }
}
